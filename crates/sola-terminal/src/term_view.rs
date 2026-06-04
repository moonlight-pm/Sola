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

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{LineHeight, Shaping};
use iced::{Color, Font, Point, Rectangle, Renderer, Size, Theme, mouse};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as GridPoint};
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
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
/// point size fed to `fill_text`. For Task 2.5 these are fixed (cribbed from
/// the spike); Task 2.6 will derive them from the real font metrics and use
/// [`cols_rows_for`] to choose the grid size on resize.
#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub cell_w: f32,
    pub cell_h: f32,
    pub font_size: f32,
}

impl Default for CellMetrics {
    fn default() -> Self {
        // Cribbed from spike_render.rs: a 14px monospace glyph fits a 9×18 box.
        Self {
            cell_w: 9.0,
            cell_h: 18.0,
            font_size: 14.0,
        }
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

/// The 16/256-colour table + the named defaults the embedder supplies.
///
/// `renderable_content().colors` is `&Colors([Option<Rgb>; …])` and every
/// entry is `None` by default — alacritty ships NO palette, so this struct is
/// the source of truth for colour resolution. Hardcoded to a dark theme for
/// now; Task 4.4 will populate it from `App::theme` (the bus theme).
//
// Task 4.4: drive Palette from bus theme.
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
            cursor: rgb(0xc8, 0xcd, 0xd6),
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
                    // `NamedColor as usize` index (0..=15).
                    other => self.indexed(other as usize as u8),
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
pub struct TermView<'a> {
    pub term: Arc<FairMutex<Term<Listener>>>,
    pub cache: &'a canvas::Cache,
    pub palette: &'a Palette,
    pub metrics: CellMetrics,
}

impl<'a> TermView<'a> {
    /// Top-left px of the cell at visible grid `point` (line ≥ 0).
    fn cell_xy(&self, line: i32, col: usize) -> (f32, f32) {
        (
            PAD + col as f32 * self.metrics.cell_w,
            PAD + line as f32 * self.metrics.cell_h,
        )
    }
}

impl<Message> canvas::Program<Message> for TermView<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
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
            let content = term.renderable_content();
            let colors = content.colors;
            let cursor = content.cursor;
            let selection = content.selection;

            // ── Pass 1: backgrounds, batched into contiguous same-bg runs ──
            //
            // We walk display_iter (row-major) and coalesce neighbouring cells
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

            for indexed in term.renderable_content().display_iter {
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
            // `content.selection` if the emulator already has one.
            if let Some(range) = selection {
                draw_selection(frame, &range, metrics, palette.selection, content.display_offset, term.screen_lines());
            }

            // ── Pass 2: glyphs ──
            for indexed in term.renderable_content().display_iter {
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
                let font = glyph_font(flags);

                let (x, y) = self.cell_xy(point.line.0, point.column.0);
                frame.fill_text(Text {
                    content: cell.c.to_string(),
                    position: Point::new(x, y),
                    color: fg,
                    size: metrics.font_size.into(),
                    font,
                    line_height: LineHeight::Absolute(metrics.cell_h.into()),
                    shaping: Shaping::Advanced,
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
            // standard fallback and is always legible.
            // NOTE: cursor.shape (Beam/Underline/HollowBlock) not yet honoured.
            let (cx, cy) = self.cell_xy(cursor.point.line.0, cursor.point.column.0);
            let block = Path::rectangle(
                Point::new(cx, cy),
                Size::new(metrics.cell_w, metrics.cell_h),
            );
            // Semi-transparent so the glyph under the cursor stays visible.
            frame.fill(&block, Color { a: 0.5, ..palette.cursor });
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
    fn cols_rows_for_known_size() {
        // Default metrics: 9×18 cells, 6px padding each side.
        let m = CellMetrics::default();
        // usable: (800 - 12) / 9 = 87.55 → 87 cols; (480 - 12) / 18 = 26 rows.
        let (cols, rows) = cols_rows_for(iced::Size::new(800.0, 480.0), m);
        assert_eq!(cols, 87);
        assert_eq!(rows, 26);
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
        // 10 cols × 5 rows exactly: 6+90+6 = 102 wide, 6+90+6 = 102 tall.
        let m = CellMetrics {
            cell_w: 9.0,
            cell_h: 18.0,
            font_size: 14.0,
        };
        let (cols, _rows) = cols_rows_for(iced::Size::new(6.0 + 90.0 + 6.0, 200.0), m);
        assert_eq!(cols, 10);
    }
}
