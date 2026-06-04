//! Task 2.5 — the live terminal canvas renderer.
//!
//! [`TermView`] is an iced [`canvas::Program`] built fresh each frame from the
//! active tab's shared `Term` handle. It renders the renderable grid:
//! batched background fills, per-cell glyphs, the cursor, and an existing
//! selection. Geometry is cached in `App`'s [`canvas::Cache`] and invalidated
//! on PTY output (see `main.rs`), so a redraw only happens when the grid
//! actually changed — this is what kept the spike at ~0.12 ms/frame.
//!
//! The renderer is read-only: it locks the term, snapshots
//! `renderable_content()`, draws, and releases. Mouse-driven selection and
//! copy are Task 4.1; the palette is hardcoded here and Task 4.4 will drive it
//! from the bus theme.

use std::sync::Arc;

use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{LineHeight, Shaping};
use iced::{Color, Font, Point, Rectangle, Renderer, Size, Theme, mouse};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{RenderableContent, Term};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb};

use crate::emulator::Listener;
use sola_kit::fonts;

/// Padding around the grid, in px. Matches the spike.
const PAD: f32 = 6.0;

/// Per-cell geometry for the monospace grid.
///
/// `cell_w`/`cell_h` are the advance box of one cell; `font_size` is the glyph
/// point size fed to `fill_text`. The dimensions are derived from the font's
/// real metrics via [`CellMetrics::for_font`] so the grid box always
/// matches the glyphs that fill it — a mismatch here reads as uneven kerning.
#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub cell_w: f32,
    pub cell_h: f32,
    pub font_size: f32,
}

impl CellMetrics {
    /// Derive cell geometry from a glyph point size and the active mono font's
    /// real metrics (`sola_kit::fonts::mono_metrics()` reads them off the TTF),
    /// snapping to integer pixels so glyphs land on the grid crisply (fractional
    /// cell origins rasterize unevenly). The advance/line ratios are NO LONGER
    /// hardcoded to JetBrains Mono — they come from whatever family `mono()`
    /// currently resolves to, so changing the kit's mono font reshapes the cell
    /// box to match. The JetBrains Mono ratios survive only as
    /// [`FontMetrics::default`], the fallback when the font can't be parsed.
    pub fn for_font(px: f32, m: sola_kit::fonts::FontMetrics) -> Self {
        Self {
            cell_w: (m.advance_per_em * px).round().max(1.0),
            // ceil for height so descenders never clip when the ratio lands
            // just over an integer.
            cell_h: (m.line_per_em * px).ceil().max(1.0),
            font_size: px,
        }
    }
}

impl Default for CellMetrics {
    fn default() -> Self {
        // 15px with the JetBrains Mono fallback ratios gives an exact integer
        // advance: 0.6·15 = 9.0, and 1.32·15 = 19.8 → 20. cell_w=9 is what the
        // layout already assumes when no font has been resolved yet.
        Self::for_font(15.0, sola_kit::fonts::FontMetrics::default())
    }
}

/// Columns and rows that fit a pane of `size` at the given cell metrics.
///
/// `floor((w - 2·PAD) / cell_w)` × `floor((h - 2·PAD) / cell_h)`, clamped to
/// ≥ 1 in each axis. Task 2.6 calls this from the resize path; Task 2.5 keeps
/// the default 80×24 grid (no resize wired yet).
pub fn cols_rows_for(size: iced::Size, metrics: CellMetrics) -> (u16, u16) {
    let usable_w = (size.width - PAD * 2.0).max(0.0);
    let usable_h = (size.height - PAD * 2.0).max(0.0);
    let cols = (usable_w / metrics.cell_w).floor().max(1.0) as u16;
    let rows = (usable_h / metrics.cell_h).floor().max(1.0) as u16;
    (cols, rows)
}

/// Map a pixel position inside the canvas to a visible grid cell `(col, row)`.
///
/// Inverse of [`TermView::cell_xy`]: subtract the padding, divide by the cell
/// advance, floor, and clamp into `0..cols` / `0..rows`. Both axes clamp so a
/// drag that leaves the pane still resolves to an edge cell (matches how
/// terminals extend a selection past the grid border). Returns visible-grid
/// coordinates — [`viewport_cell_to_point`] maps those onto buffer space.
pub fn pixel_to_cell(
    x: f32,
    y: f32,
    metrics: CellMetrics,
    cols: u16,
    rows: u16,
) -> (usize, usize) {
    let col = ((x - PAD) / metrics.cell_w).floor();
    let row = ((y - PAD) / metrics.cell_h).floor();
    let col = col.max(0.0) as usize;
    let row = row.max(0.0) as usize;
    let col = col.min(cols.saturating_sub(1) as usize);
    let row = row.min(rows.saturating_sub(1) as usize);
    (col, row)
}

/// Which half of a cell the pixel `x` falls in — the selection `Side`.
///
/// Left half → `Side::Left` (caret before the glyph), right half → `Side::Right`
/// (caret after it). alacritty uses this to decide whether the cell under the
/// cursor is included in a `Simple` selection.
pub fn cell_side(x: f32, col: usize, metrics: CellMetrics) -> Side {
    let cell_start = PAD + col as f32 * metrics.cell_w;
    if x - cell_start < metrics.cell_w * 0.5 {
        Side::Left
    } else {
        Side::Right
    }
}

/// Map a visible-grid cell `(col, row)` to a buffer [`GridPoint`].
///
/// The visible grid is the window into the scrollback at the current
/// `display_offset`: visible row 0 is buffer line `-display_offset`. This is the
/// inverse of the renderer's `vis = buf_line + display_offset`, and mirrors
/// alacritty's own `viewport_to_point`. Selections are stored in BUFFER
/// coordinates, so this MUST be applied or the selection drifts by
/// `display_offset` rows whenever the grid is scrolled.
pub fn viewport_cell_to_point(col: usize, row: usize, display_offset: usize) -> GridPoint {
    GridPoint::new(Line(row as i32 - display_offset as i32), Column(col))
}

/// The 16/256-colour table + the named defaults the embedder supplies.
///
/// `renderable_content().colors` is `&Colors([Option<Rgb>; …])` and every
/// entry is `None` by default — alacritty ships NO palette, so this struct is
/// the source of truth for colour resolution.
///
/// [`Palette::default`] is the built-in dark theme, used as the FALLBACK before
/// any bus theme arrives. [`Palette::from_kit_theme`] (Task 4.4) derives the
/// live palette from the kit/bus theme so the terminal matches the rest of
/// Sola and updates on `Topic::Theme`. Only the 16-entry ANSI table plus the
/// `fg`/`bg`/`cursor`/`selection` defaults are themed — truecolor
/// (`Color::Spec`) and `Color::Indexed` cells still resolve literally in
/// [`Palette::resolve`].
#[derive(Debug, Clone)]
pub struct Palette {
    /// Default foreground (NamedColor::Foreground / unset fg).
    pub fg: Color,
    /// Default background (NamedColor::Background / unset bg).
    pub bg: Color,
    /// Block-cursor colour.
    pub cursor: Color,
    /// Selection highlight background.
    pub selection: Color,
    /// The 16 ANSI base colours (0..=7 normal, 8..=15 bright).
    pub ansi: [Color; 16],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            bg: rgb(0x0a, 0x0c, 0x10),
            fg: rgb(0xc8, 0xcd, 0xd6),
            // Deep gold block cursor — fixed, not theme-derived, so it stays a
            // warm gold (never brown) on any palette. See `from_kit_theme`.
            cursor: rgb(0xff, 0xb8, 0x00),
            selection: Color::from_rgba8(0x33, 0x66, 0xcc, 0.35),
            ansi: [
                rgb(0x00, 0x00, 0x00), // 0  black
                rgb(0xcc, 0x33, 0x33), // 1  red
                rgb(0x33, 0xaa, 0x33), // 2  green
                rgb(0xcc, 0xaa, 0x33), // 3  yellow
                rgb(0x33, 0x66, 0xcc), // 4  blue
                rgb(0xaa, 0x33, 0xaa), // 5  magenta
                rgb(0x33, 0xaa, 0xaa), // 6  cyan
                rgb(0xcc, 0xcc, 0xcc), // 7  white
                rgb(0x55, 0x55, 0x55), // 8  bright black
                rgb(0xff, 0x55, 0x55), // 9  bright red
                rgb(0x55, 0xff, 0x55), // 10 bright green
                rgb(0xff, 0xff, 0x55), // 11 bright yellow
                rgb(0x55, 0x88, 0xff), // 12 bright blue
                rgb(0xff, 0x55, 0xff), // 13 bright magenta
                rgb(0x55, 0xff, 0xff), // 14 bright cyan
                rgb(0xff, 0xff, 0xff), // 15 bright white
            ],
        }
    }
}

impl Palette {
    /// Derive the terminal palette from the kit's theme [`Atoms`] (the bus
    /// theme's colour tokens, read via `sola_kit::theme::atoms_from_bus_theme`).
    ///
    /// We source from `Atoms` rather than the resulting `iced::Theme`'s
    /// `extended_palette()` because `Atoms` carries every semantic token we
    /// need directly — `warning` (which iced's reduced `Extended` palette does
    /// not surface as a distinct slot), `accent`, all four background tiers
    /// (`bg`/`bg_raised`/`bg_hover`/`border`), `fg`, and `fg_muted` — with no
    /// lossy derivation. It is also a pure value, so the mapping is trivially
    /// unit-testable.
    ///
    /// ## Mapping
    ///
    /// | terminal slot      | kit atom                                  |
    /// | ------------------ | ----------------------------------------- |
    /// | default bg         | `bg` (bg-primary)                         |
    /// | default fg         | `fg` (text-primary)                       |
    /// | cursor             | `accent`                                  |
    /// | selection bg       | `accent` @ 35% alpha (muted accent wash)  |
    ///
    /// ### ANSI table (0..=7 normal, 8..=15 bright)
    ///
    /// | idx | name           | source                                   |
    /// | --- | -------------- | ---------------------------------------- |
    /// | 0   | black          | `bg_hover` (darkest visible bg tier)     |
    /// | 1   | red            | `danger`                                 |
    /// | 2   | green          | `success`                                |
    /// | 3   | yellow         | `warning`                                |
    /// | 4   | blue           | `accent`                                 |
    /// | 5   | magenta        | standard `#aa33aa` (no kit atom)         |
    /// | 6   | cyan           | standard `#33aaaa` (no kit atom)         |
    /// | 7   | white          | `fg_muted` (muted light grey)            |
    /// | 8   | bright black   | mix(`bg_hover`, `fg_muted`) — a grey     |
    /// | 9   | bright red     | lighten(`danger`)                        |
    /// | 10  | bright green   | lighten(`success`)                       |
    /// | 11  | bright yellow  | lighten(`warning`)                       |
    /// | 12  | bright blue    | lighten(`accent`)                        |
    /// | 13  | bright magenta | lighten standard magenta                 |
    /// | 14  | bright cyan    | lighten standard cyan                    |
    /// | 15  | bright white   | `fg` (full foreground)                   |
    ///
    /// The four semantic ANSI slots (red/green/yellow/blue) are kept true to
    /// their meaning so `ls --color`, `git`, and `btop` read correctly; the two
    /// hueless kit-less slots (magenta/cyan) keep tasteful standard values. Each
    /// bright variant is a lightened version of its normal counterpart, so the
    /// bright row is always visibly brighter.
    pub fn from_kit_theme(atoms: &sola_kit::theme::Atoms) -> Self {
        // Standard magenta/cyan — the kit has no atom for these hues, so we
        // keep recognisable values rather than inventing a tint.
        let magenta = rgb(0xaa, 0x33, 0xaa);
        let cyan = rgb(0x33, 0xaa, 0xaa);
        Self {
            bg: atoms.bg,
            fg: atoms.fg,
            // Fixed deep gold, independent of the theme accent — a warm gold
            // cursor reads clearly on every palette and never muddies to brown.
            cursor: rgb(0xff, 0xb8, 0x00),
            // A muted accent wash, legible over any bg (alpha-blended), instead
            // of a flat fill that could clash with a light theme.
            selection: with_alpha(atoms.accent, 0.35),
            ansi: [
                atoms.bg_hover,        // 0  black
                atoms.danger,          // 1  red
                atoms.success,         // 2  green
                atoms.warning,         // 3  yellow
                atoms.accent,          // 4  blue
                magenta,               // 5  magenta
                cyan,                  // 6  cyan
                atoms.fg_muted,        // 7  white
                mix(atoms.bg_hover, atoms.fg_muted, 0.5), // 8  bright black
                lighten(atoms.danger),  // 9  bright red
                lighten(atoms.success), // 10 bright green
                lighten(atoms.warning), // 11 bright yellow
                lighten(atoms.accent),  // 12 bright blue
                lighten(magenta),       // 13 bright magenta
                lighten(cyan),          // 14 bright cyan
                atoms.fg,               // 15 bright white
            ],
        }
    }

    /// Resolve an alacritty cell colour to an iced `Color`.
    ///
    /// `colors` is the live (mostly-`None`) embedder palette from
    /// `renderable_content`; a `Some` entry there wins (apps can override the
    /// palette via OSC 4), otherwise we fall back to our own table.
    fn resolve(&self, color: AnsiColor, colors: &Colors) -> Color {
        match color {
            AnsiColor::Spec(rgb) => rgb_to_iced(rgb),
            AnsiColor::Named(named) => {
                if let Some(rgb) = colors[named] {
                    return rgb_to_iced(rgb);
                }
                match named {
                    NamedColor::Foreground => self.fg,
                    NamedColor::Background => self.bg,
                    NamedColor::Cursor => self.cursor,
                    // The 16 base/bright names map onto our ANSI table by their
                    // `NamedColor as usize` index (0..=15). Discriminants are
                    // exactly 0..=15 so the cast is always safe.
                    NamedColor::Black => self.indexed(0),
                    NamedColor::Red => self.indexed(1),
                    NamedColor::Green => self.indexed(2),
                    NamedColor::Yellow => self.indexed(3),
                    NamedColor::Blue => self.indexed(4),
                    NamedColor::Magenta => self.indexed(5),
                    NamedColor::Cyan => self.indexed(6),
                    NamedColor::White => self.indexed(7),
                    NamedColor::BrightBlack => self.indexed(8),
                    NamedColor::BrightRed => self.indexed(9),
                    NamedColor::BrightGreen => self.indexed(10),
                    NamedColor::BrightYellow => self.indexed(11),
                    NamedColor::BrightBlue => self.indexed(12),
                    NamedColor::BrightMagenta => self.indexed(13),
                    NamedColor::BrightCyan => self.indexed(14),
                    NamedColor::BrightWhite => self.indexed(15),
                    // Dim* / BrightForeground / DimForeground: alacritty 0.26
                    // doesn't route these into cell colours, but their
                    // discriminants (259+) would truncate badly under `as u8`.
                    // Return the default foreground as a safe, visible fallback.
                    _ => self.fg,
                }
            }
            AnsiColor::Indexed(idx) => {
                if let Some(rgb) = colors[idx as usize] {
                    return rgb_to_iced(rgb);
                }
                self.indexed(idx)
            }
        }
    }

    /// xterm 256-colour resolution: 0..=15 from our ANSI table, 16..=231 from
    /// the 6×6×6 cube, 232..=255 from the grayscale ramp.
    fn indexed(&self, idx: u8) -> Color {
        let i = idx as usize;
        if i < 16 {
            return self.ansi[i];
        }
        if i < 232 {
            let i = i - 16;
            let r = (i / 36) as u8;
            let g = ((i / 6) % 6) as u8;
            let b = (i % 6) as u8;
            let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            return rgb(conv(r), conv(g), conv(b));
        }
        let v = 8 + (i - 232) as u8 * 10;
        rgb(v, v, v)
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

fn rgb_to_iced(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

/// Return `c` with its alpha channel replaced. Used for the selection wash so
/// the highlight blends over whatever sits beneath it (works on light and dark
/// themes alike) rather than a flat opaque fill.
fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// Linear blend of two colours: `t=0` → `a`, `t=1` → `b`. Alpha follows the
/// same interpolation. Kept channel-space (not gamma-correct) — good enough for
/// deriving a tasteful in-between grey for ANSI "bright black".
fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    Color {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
}

/// Lighten a colour toward white by ~45%, giving each "bright" ANSI variant a
/// visibly brighter version of its normal counterpart while preserving hue.
fn lighten(c: Color) -> Color {
    mix(c, Color::WHITE, 0.45)
}

/// Dim a colour for the `DIM` flag (alacritty's faint attribute) — scale
/// toward black at ~2/3 intensity.
fn dim(c: Color) -> Color {
    Color {
        r: c.r * 0.66,
        g: c.g * 0.66,
        b: c.b * 0.66,
        a: c.a,
    }
}

/// The live terminal canvas program.
///
/// Built fresh in `App::view` each frame: the term handle and metrics are
/// cheap to copy, but the [`canvas::Cache`] and [`Palette`] are borrowed from
/// `App` so cached geometry persists across frames and the palette has one
/// owner (Task 4.4's edit point).
pub struct TermView<'a, Message> {
    pub term: Arc<FairMutex<Term<Listener>>>,
    pub cache: &'a canvas::Cache,
    pub palette: &'a Palette,
    pub metrics: CellMetrics,
    /// Blink phase for the block cursor: `true` draws it, `false` hides it.
    /// `App` toggles this on a timer and clears the cache so the cursor
    /// appears/disappears between frames.
    pub cursor_on: bool,
    /// Message emitted whenever a mouse interaction mutates `term.selection`
    /// (start / extend / clear). `App` handles it by clearing the geometry
    /// cache so the highlight re-renders. Cloned, so cheap variants only.
    pub on_select: Message,
}

impl<'a, Message> TermView<'a, Message> {
    /// Top-left px of the cell at visible grid `point` (line ≥ 0).
    fn cell_xy(&self, line: i32, col: usize) -> (f32, f32) {
        (
            PAD + col as f32 * self.metrics.cell_w,
            PAD + line as f32 * self.metrics.cell_h,
        )
    }
}

/// In-progress drag state for the selection canvas. Lives in the
/// [`canvas::Program::State`] so it survives between events without `App`
/// owning it.
#[derive(Default)]
pub struct SelState {
    dragging: bool,
    /// Whether the current drag has actually extended past its origin cell.
    /// A press+release with no movement is a plain click → clear selection.
    moved: bool,
}

impl<Message: Clone> canvas::Program<Message> for TermView<'_, Message> {
    type State = SelState;

    /// Mouse-driven selection.
    ///
    /// - Left press inside bounds: start a new `Simple` selection anchored at
    ///   the cell under the cursor (in BUFFER coords) and arm the drag.
    /// - Cursor move while dragging: extend the selection to the cell under the
    ///   cursor; mark the drag as "moved" so release knows it wasn't a click.
    /// - Left release: disarm. If the drag never moved, treat it as a plain
    ///   click and clear any selection (so a stray click deselects).
    ///
    /// All term mutation happens under a brief lock that is released before
    /// returning — `draw` only locks inside its own `cache.draw` closure and
    /// iced never runs `draw` and `update` concurrently, so there's no
    /// re-entrancy with the render lock.
    fn update(
        &self,
        state: &mut SelState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let Event::Mouse(mouse_event) = event else {
            return None;
        };

        // Resolve a canvas-local pixel position to a BUFFER point under the
        // current scroll offset, plus the cell side the x falls in.
        let point_at = |pos: Point| -> (GridPoint, Side) {
            let term = self.term.lock();
            let cols = term.columns() as u16;
            let rows = term.screen_lines() as u16;
            let display_offset = term.grid().display_offset();
            drop(term);
            let (col, row) = pixel_to_cell(pos.x, pos.y, self.metrics, cols, rows);
            let side = cell_side(pos.x, col, self.metrics);
            (viewport_cell_to_point(col, row, display_offset), side)
        };

        match mouse_event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let Some(pos) = cursor.position_in(bounds) else {
                    return None;
                };
                let (point, side) = point_at(pos);
                let mut term = self.term.lock();
                term.selection = Some(Selection::new(SelectionType::Simple, point, side));
                drop(term);
                state.dragging = true;
                state.moved = false;
                Some(canvas::Action::publish(self.on_select.clone()).and_capture())
            }
            mouse::Event::CursorMoved { .. } if state.dragging => {
                // Use the clamped in-bounds position so a drag past the edge
                // still extends to an edge cell.
                let pos = cursor.position_in(bounds).or_else(|| {
                    cursor
                        .position()
                        .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y))
                })?;
                let (point, side) = point_at(pos);
                let mut term = self.term.lock();
                if let Some(sel) = term.selection.as_mut() {
                    sel.update(point, side);
                }
                drop(term);
                state.moved = true;
                Some(canvas::Action::publish(self.on_select.clone()).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) if state.dragging => {
                state.dragging = false;
                let was_drag = state.moved;
                state.moved = false;
                if was_drag {
                    // Real drag → keep the installed selection for copy.
                    Some(canvas::Action::capture())
                } else {
                    // Plain click, no movement → clear any selection.
                    let mut term = self.term.lock();
                    term.selection = None;
                    drop(term);
                    Some(canvas::Action::publish(self.on_select.clone()).and_capture())
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &SelState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let metrics = self.metrics;
        let palette = self.palette;

        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            // Backdrop — one fill behind the whole pane.
            frame.fill_rectangle(Point::ORIGIN, frame.size(), palette.bg);

            let term = self.term.lock();

            // Fetch renderable content ONCE per frame and collect the
            // display iterator into a Vec so both passes can iterate it
            // without re-deriving the display window.
            let content = term.renderable_content();
            let RenderableContent {
                display_iter,
                colors,
                cursor,
                selection,
                display_offset,
                ..
            } = content;
            let cells: Vec<_> = display_iter.collect();

            // ── Pass 1: backgrounds, batched into contiguous same-bg runs ──
            //
            // We walk cells (row-major) and coalesce neighbouring cells
            // on the same row that share a bg colour into a single fill rect.
            // This avoids a fill per blank cell AND the hairline seams that
            // per-cell rects leave between fills.
            let mut run: Option<(i32, usize, usize, Color)> = None; // (line, start_col, end_col, bg)
            let flush = |frame: &mut Frame<Renderer>, run: &Option<(i32, usize, usize, Color)>| {
                if let Some((line, start, end, bg)) = *run {
                    if bg != palette.bg {
                        let x = PAD + start as f32 * metrics.cell_w;
                        let y = PAD + line as f32 * metrics.cell_h;
                        let w = (end - start + 1) as f32 * metrics.cell_w;
                        frame.fill_rectangle(
                            Point::new(x, y),
                            Size::new(w, metrics.cell_h),
                            bg,
                        );
                    }
                }
            };

            for indexed in &cells {
                let cell = indexed.cell;
                let point = indexed.point;
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let (mut fg, mut bg) = (
                    palette.resolve(cell.fg, colors),
                    palette.resolve(cell.bg, colors),
                );
                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let line = point.line.0;
                let col = point.column.0;

                match run.as_mut() {
                    Some((rl, _rs, re, rbg))
                        if *rl == line && *re + 1 == col && *rbg == bg =>
                    {
                        *re = col; // extend the run
                    }
                    _ => {
                        flush(frame, &run);
                        run = Some((line, col, col, bg));
                    }
                }
            }
            flush(frame, &run);

            // ── Selection highlight (render-only; Task 4.1 owns interaction) ──
            //
            // Task 4.1: mouse selection + copy. Here we only paint an existing
            // `selection` if the emulator already has one.
            if let Some(range) = selection {
                draw_selection(frame, &range, metrics, palette.selection, display_offset, term.screen_lines());
            }

            // ── Pass 2: glyphs ──
            for indexed in &cells {
                let cell = indexed.cell;
                let point = indexed.point;
                let flags = cell.flags;

                // Skip non-printing cells.
                if cell.c == ' ' || cell.c == '\0' {
                    continue;
                }
                if flags.contains(Flags::WIDE_CHAR_SPACER) || flags.contains(Flags::HIDDEN) {
                    continue;
                }

                let mut fg = palette.resolve(cell.fg, colors);
                let mut bg = palette.resolve(cell.bg, colors);
                if flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if flags.contains(Flags::DIM) {
                    fg = dim(fg);
                }

                // Font role per weight/style flags. Variant fonts aren't packaged
                // yet, so bold/italic fall back to mono — only the synthetic
                // weight/style on the Font struct distinguishes them. (NOTE:
                // a dedicated bold/italic mono face is a follow-up.)
                // `glyph_font()` reads `fonts::mono()`, which the bus theme
                // hot-swaps via `apply_theme_update` — so the font FAMILY
                // updates live on `Topic::Theme`. The cell geometry comes from
                // `CellMetrics::for_font`, derived from the active mono font's
                // real metrics (`sola_kit::fonts::mono_metrics()`); the terminal
                // recomputes it on `Topic::Theme` so a font change reshapes the
                // cell box to match.
                let font = glyph_font(flags);

                let (x, y) = self.cell_xy(point.line.0, point.column.0);
                frame.fill_text(Text {
                    content: cell.c.to_string(),
                    // Snap to integer pixels: fractional glyph origins make the
                    // rasterizer hint each cell slightly differently, which
                    // reads as uneven kerning across the monospace grid.
                    position: Point::new(x.round(), y.round()),
                    color: fg,
                    size: metrics.font_size.into(),
                    font,
                    line_height: LineHeight::Absolute(metrics.cell_h.into()),
                    // Basic shaping is correct for a single-glyph monospace
                    // cell — no ligatures/BiDi/fallback runs, so we avoid
                    // Advanced's per-run positioning variance. (A non-BMP or
                    // complex char that Basic can't render is an accepted edge
                    // case for now.)
                    shaping: Shaping::Basic,
                    ..Text::default()
                });

                // Underline / strikeout as stroked lines across the cell.
                if flags.contains(Flags::UNDERLINE) {
                    let uy = y + metrics.cell_h - 2.0;
                    stroke_h(frame, x, uy, metrics.cell_w, fg);
                }
                if flags.contains(Flags::STRIKEOUT) {
                    let sy = y + metrics.cell_h * 0.5;
                    stroke_h(frame, x, sy, metrics.cell_w, fg);
                }
            }

            // ── Cursor: block at the cursor cell ──
            //
            // We draw a filled block in the cursor colour. Non-block shapes
            // (Beam / Underline / Hollow) are a follow-up — block is the
            // standard fallback and is always legible. Skipped on the "off"
            // blink phase so the cursor blinks (App toggles `cursor_on`).
            // NOTE: cursor.shape (Beam/Underline/HollowBlock) not yet honoured.
            if self.cursor_on {
                let (cx, cy) = self.cell_xy(cursor.point.line.0, cursor.point.column.0);
                let block = Path::rectangle(
                    Point::new(cx, cy),
                    Size::new(metrics.cell_w, metrics.cell_h),
                );
                // Fairly opaque: a low alpha over a dark background muddies the
                // warm gold into brown. 0.85 keeps it reading as gold while the
                // glyph beneath stays faintly visible.
                frame.fill(&block, Color { a: 0.85, ..palette.cursor });
            }
        });

        vec![geometry]
    }
}

/// Font for a glyph given its cell flags. Bold/italic carry synthetic
/// weight/style on the mono family until dedicated faces ship.
fn glyph_font(flags: Flags) -> Font {
    // BOLD_ITALIC is the BOLD|ITALIC bit pair, so the individual checks cover
    // it. A real bold/italic mono face is a follow-up; for now only the
    // synthetic weight/style on the Font struct distinguishes the variants.
    let mut font = fonts::mono();
    if flags.contains(Flags::BOLD) {
        font.weight = iced::font::Weight::Bold;
    }
    if flags.contains(Flags::ITALIC) {
        font.style = iced::font::Style::Italic;
    }
    font
}

/// Stroke a 1px horizontal line of `width` at (`x`, `y`).
fn stroke_h(frame: &mut Frame<Renderer>, x: f32, y: f32, width: f32, color: Color) {
    let path = Path::line(Point::new(x, y), Point::new(x + width, y));
    frame.stroke(
        &path,
        Stroke::default().with_color(color).with_width(1.0),
    );
}

/// Paint the selection highlight over the visible portion of `range`.
///
/// `range.start`/`range.end` are buffer points (line can be negative for
/// scrollback). We convert to visible lines via `display_offset` and clamp to
/// the visible window, filling whole rows between start and end (line-mode) or
/// the start/end columns (block-mode flagged by `is_block`).
fn draw_selection(
    frame: &mut Frame<Renderer>,
    range: &SelectionRange,
    metrics: CellMetrics,
    color: Color,
    display_offset: usize,
    screen_lines: usize,
) {
    // Buffer-line → visible-line (0 = top of viewport). display_offset shifts
    // the viewport up into scrollback; a buffer line is visible when its
    // offset-adjusted index falls in 0..screen_lines.
    let to_visible = |p: GridPoint<Line, Column>| -> Option<(i32, usize)> {
        let vis = p.line.0 + display_offset as i32;
        if vis < 0 || vis as usize >= screen_lines {
            return None;
        }
        Some((vis, p.column.0))
    };

    let start = range.start;
    let end = range.end;
    let is_block = range.is_block;

    // Iterate buffer lines from start to end; for each visible row, fill the
    // selected column span.
    for buf_line in start.line.0..=end.line.0 {
        let line_pt = GridPoint::new(Line(buf_line), Column(0));
        let Some((vis_line, _)) = to_visible(line_pt) else {
            continue;
        };

        let (col_start, col_end) = if is_block {
            (start.column.0, end.column.0)
        } else if start.line.0 == end.line.0 {
            (start.column.0, end.column.0)
        } else if buf_line == start.line.0 {
            // First row: from start column to a generous right edge.
            (start.column.0, usize::MAX)
        } else if buf_line == end.line.0 {
            (0, end.column.0)
        } else {
            (0, usize::MAX)
        };

        let x = PAD + col_start as f32 * metrics.cell_w;
        let y = PAD + vis_line as f32 * metrics.cell_h;
        // usize::MAX means "to the right edge"; cap at a wide span. The exact
        // row width belongs to Task 4.1's selection model.
        let span_cols = col_end.saturating_sub(col_start).saturating_add(1);
        let span_cols = span_cols.min(512); // sane cap for the MAX sentinel
        let w = span_cols as f32 * metrics.cell_w;
        frame.fill_rectangle(Point::new(x, y), Size::new(w, metrics.cell_h), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_font_matches_jetbrains_mono_fallback_ratios() {
        // The JetBrains Mono fallback ratios (FontMetrics::default) at 15px
        // still yield the 9×20 cell the layout assumes — so when the active
        // mono IS JetBrains Mono, behaviour is unchanged from the old
        // hardcoded path.
        let fm = sola_kit::fonts::FontMetrics::default();
        let m = CellMetrics::for_font(15.0, fm);
        assert_eq!(m.font_size, 15.0);
        assert_eq!(m.cell_w, 9.0, "0.6·15 = 9.0 exactly");
        assert_eq!(m.cell_h, 20.0, "1.32·15 = 19.8 → ceil 20");
        // cell_w is always the advance ratio rounded to an integer pixel.
        assert_eq!(m.cell_w, (fm.advance_per_em * m.font_size).round());
        // default() is the 15px fallback derivation.
        let d = CellMetrics::default();
        assert_eq!((d.cell_w, d.cell_h, d.font_size), (9.0, 20.0, 15.0));
    }

    #[test]
    fn cols_rows_for_known_size() {
        // Default metrics: 9×20 cells, 6px padding each side.
        let m = CellMetrics::default();
        // usable: (800 - 12) / 9 = 87.55 → 87 cols; (480 - 12) / 20 = 23.4 → 23 rows.
        let (cols, rows) = cols_rows_for(iced::Size::new(800.0, 480.0), m);
        assert_eq!(cols, 87);
        assert_eq!(rows, 23);
    }

    #[test]
    fn cols_rows_for_clamps_to_at_least_one() {
        let m = CellMetrics::default();
        // A pane smaller than the padding still yields a 1×1 grid, never 0.
        let (cols, rows) = cols_rows_for(iced::Size::new(1.0, 1.0), m);
        assert_eq!(cols, 1);
        assert_eq!(rows, 1);
    }

    #[test]
    fn cols_rows_for_exact_fit() {
        // 10 cols exactly: 6 + 10·9 + 6 = 102 wide. cell_w = 9 at the default
        // 15px size with the JetBrains Mono fallback ratios, so derive the
        // metrics rather than hardcoding them.
        let m = CellMetrics::for_font(15.0, sola_kit::fonts::FontMetrics::default());
        let (cols, _rows) = cols_rows_for(iced::Size::new(6.0 + 90.0 + 6.0, 200.0), m);
        assert_eq!(cols, 10);
    }

    #[test]
    fn cols_rows_for_clamps_rows_independently() {
        // Default metrics: cell_w=9, cell_h=20, PAD=6.
        // Width is wide enough for many columns: usable_w = 200 - 12 = 188 → 20 cols.
        // Height is smaller than one cell + padding (< 12 + 20 = 32): rows clamps to 1.
        // This catches a regression where only the cols axis was clamped.
        let m = CellMetrics::default();
        let (cols, rows) = cols_rows_for(iced::Size::new(200.0, 1.0), m);
        assert!(cols >= 2, "expected multiple cols, got {cols}");
        assert_eq!(rows, 1);
    }

    #[test]
    fn pixel_to_cell_maps_interior_point() {
        // Default metrics: cell_w=9, cell_h=20, PAD=6.
        let m = CellMetrics::default();
        // A pixel one past the top-left of cell (col 3, row 2):
        // x = PAD + 3*9 + 1 = 34, y = PAD + 2*20 + 1 = 47.
        let (col, row) = pixel_to_cell(34.0, 47.0, m, 80, 24);
        assert_eq!((col, row), (3, 2));
    }

    #[test]
    fn pixel_to_cell_clamps_below_and_above_grid() {
        let m = CellMetrics::default();
        // Above/left of the grid clamps to (0, 0).
        assert_eq!(pixel_to_cell(-50.0, -50.0, m, 80, 24), (0, 0));
        // Far past the bottom-right clamps to the last cell (cols-1, rows-1).
        assert_eq!(pixel_to_cell(100_000.0, 100_000.0, m, 80, 24), (79, 23));
    }

    #[test]
    fn cell_side_splits_on_the_half_cell() {
        let m = CellMetrics::default(); // cell_w = 9.
        // Col 3 starts at PAD + 3*9 = 33; half is at +4.5.
        assert_eq!(cell_side(34.0, 3, m), Side::Left); // 1px in → left half.
        assert_eq!(cell_side(40.0, 3, m), Side::Right); // 7px in → right half.
    }

    #[test]
    fn viewport_cell_to_point_no_scroll_is_identity_in_line() {
        // With display_offset 0, visible row N == buffer line N.
        let p = viewport_cell_to_point(5, 2, 0);
        assert_eq!(p.line, Line(2));
        assert_eq!(p.column, Column(5));
    }

    #[test]
    fn viewport_cell_to_point_offset_shifts_into_scrollback() {
        // Scrolled up by 10: the top visible row (row 0) is buffer line -10,
        // and row 3 is buffer line -7. This is the offset that, if dropped,
        // makes a scrolled selection drift.
        assert_eq!(viewport_cell_to_point(0, 0, 10).line, Line(-10));
        assert_eq!(viewport_cell_to_point(4, 3, 10).line, Line(-7));
        assert_eq!(viewport_cell_to_point(4, 3, 10).column, Column(4));
    }

    // Task 4.4 — the kit-theme mapping. Build the palette from a hand-built
    // Atoms with distinct, recognisable channel values and assert each slot
    // lands where the mapping promises.
    fn sample_atoms() -> sola_kit::theme::Atoms {
        use sola_kit::theme::parse;
        sola_kit::theme::Atoms {
            bg: parse("#0d1117"),
            bg_raised: parse("#161b22"),
            bg_hover: parse("#1a2030"),
            border: parse("#2d333b"),
            fg: parse("#e6edf3"),
            fg_muted: parse("#6e7681"),
            accent: parse("#00d4ff"),
            success: parse("#3fb950"),
            warning: parse("#d29922"),
            danger: parse("#f85149"),
        }
    }

    #[test]
    fn from_kit_theme_maps_defaults_and_semantic_ansi() {
        let a = sample_atoms();
        let p = Palette::from_kit_theme(&a);

        // Defaults.
        assert_eq!(p.bg, a.bg, "default bg ← bg-primary");
        assert_eq!(p.fg, a.fg, "default fg ← text-primary");
        // Cursor is a fixed deep gold, not theme-derived.
        assert_eq!(p.cursor, Color::from_rgb8(0xff, 0xb8, 0x00), "cursor ← fixed gold");
        // Selection is the accent wash at 35% alpha.
        assert_eq!(p.selection.r, a.accent.r);
        assert_eq!(p.selection.g, a.accent.g);
        assert_eq!(p.selection.b, a.accent.b);
        assert!((p.selection.a - 0.35).abs() < 1e-6, "selection ← accent @ 0.35");

        // Semantic ANSI slots map to the matching kit atoms.
        assert_eq!(p.ansi[1], a.danger, "ansi[1] red ← danger");
        assert_eq!(p.ansi[2], a.success, "ansi[2] green ← success");
        assert_eq!(p.ansi[3], a.warning, "ansi[3] yellow ← warning");
        assert_eq!(p.ansi[4], a.accent, "ansi[4] blue ← accent");
        assert_eq!(p.ansi[7], a.fg_muted, "ansi[7] white ← fg_muted");
        assert_eq!(p.ansi[15], a.fg, "ansi[15] bright white ← fg");
    }

    #[test]
    fn from_kit_theme_brights_are_brighter() {
        let a = sample_atoms();
        let p = Palette::from_kit_theme(&a);
        // Each bright variant (8..=14, paired with 0..=6) must be at least as
        // light as its normal counterpart in every channel, and strictly
        // brighter overall — `lighten`/`mix` move toward white.
        for (normal, bright) in [(1usize, 9usize), (2, 10), (3, 11), (4, 12), (5, 13), (6, 14)] {
            let n = p.ansi[normal];
            let b = p.ansi[bright];
            let lum = |c: Color| c.r + c.g + c.b;
            assert!(
                lum(b) > lum(n),
                "bright slot {bright} not brighter than normal slot {normal}"
            );
        }
    }

    // The fallback dark theme must still read sensibly: red is red-dominant,
    // green is green-dominant, blue is blue-dominant. Guards against a future
    // edit that breaks the pre-theme palette.
    #[test]
    fn default_palette_primaries_have_correct_hue() {
        let p = Palette::default();
        let red = p.ansi[1];
        assert!(red.r > red.g && red.r > red.b, "ansi[1] should be red-dominant");
        let green = p.ansi[2];
        assert!(green.g > green.r && green.g > green.b, "ansi[2] should be green-dominant");
        let blue = p.ansi[4];
        assert!(blue.b > blue.r && blue.b > blue.g, "ansi[4] should be blue-dominant");
    }
}
