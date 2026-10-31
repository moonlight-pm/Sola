//! Geometry: the menubar, the shell's overlays, the tab bar, the tiled
//! zones (see [`crate::stacks`]), and floating windows' rectangles.
//!
//! Functions return Frames; the caller (`Shell`) emits them.
use std::collections::{HashMap, HashSet};

use sola_bus::topics::{FloatGeometry, FrameUpdate, OutputGeometry, WindowGeometry, Zone, ZoneLayout};
use tracing::info;


/// An island of the top row (board 03): the flower, the workspaces, the
/// instruments, the clock — 28 px pills on the ground.
pub const ISLAND_H: i32 = 28;

/// The top row's window: a gap above its islands. Tiles and the tab bar
/// start a gap below it (at 48), so the row is gap + island + gap.
pub const MENUBAR_HEIGHT: i32 = GAP + ISLAND_H;

/// Iced open size/position for menu and launcher: usable area
/// below the menubar. Matches [`Self::default_app_frame`].
pub fn overlay_open_rect(output_w: i32, output_h: i32) -> (f32, f32, f32, f32) {
    let w = output_w.max(1) as f32;
    let h = (output_h - MENUBAR_HEIGHT).max(1) as f32;
    (0.0, MENUBAR_HEIGHT as f32, w, h)
}

/// Swapchain size while a shell overlay is mapped but dismissed.
///
/// Iced presents every daemon window after any `Message`. A 2×2 buffer is
/// cheap; a full-output one is not. River still `hide`s these until
/// Composition includes them.
///
/// Must be ≥ winit's Wayland `MIN_WINDOW_SIZE` (2×1). A 1×1 window with
/// `resizable = false` sets min 2×1 and max 1×1 → `xdg_toplevel` error 2
/// (`invalid_size`) and iced panics on create.
pub const OVERLAY_PARK: i32 = 2;

/// Off-output origin for a parked overlay. If hide() loses a frame, a
/// 2×2 at (0,0) still flashes; this keeps that leak off the screen.
pub const OVERLAY_PARK_X: i32 = -10000;
pub const OVERLAY_PARK_Y: i32 = -10000;

/// Apps whose windows land in Floating rather than in a tiled zone: the
/// loupe, Quick Look-style previews, and gamescope's game nest.
pub fn floats_by_default(app_id: &str) -> bool {
    matches!(app_id, "sola-scope" | "sola-preview") || app_id.eq_ignore_ascii_case("gamescope")
}

/// Parked overlay `Topic::Frame` (2×2, off-output). River ignores
/// non-positive frames; 2×2 also satisfies winit's Wayland min size.
pub fn overlay_park_frame(window_id: u32) -> FrameUpdate {
    FrameUpdate {
        window_id,
        x: OVERLAY_PARK_X,
        y: OVERLAY_PARK_Y,
        width: OVERLAY_PARK,
        height: OVERLAY_PARK,
        fullscreen: false,
    }
}

/// Live overlay rectangle. `cover_menubar` is the selection marquee
/// (full output at 0,0); other overlays sit in the usable area below
/// the menubar.
pub fn overlay_live_frame(
    window_id: u32,
    output_w: i32,
    output_h: i32,
    cover_menubar: bool,
) -> FrameUpdate {
    if cover_menubar {
        FrameUpdate {
            window_id,
            x: 0,
            y: 0,
            width: output_w.max(1),
            height: output_h.max(1),
            fullscreen: false,
        }
    } else {
        FrameUpdate {
            window_id,
            x: 0,
            y: MENUBAR_HEIGHT,
            width: output_w.max(1),
            height: (output_h - MENUBAR_HEIGHT).max(1),
            fullscreen: false,
        }
    }
}

/// Frame an overlay: parked size while dismissed, live size while shown.
/// Visible overlays need output geometry; without it this returns `None`
/// so we do not invent a 1920 placeholder.
pub fn overlay_frame(
    window_id: u32,
    visible: bool,
    output: Option<(i32, i32)>,
    cover_menubar: bool,
) -> Option<FrameUpdate> {
    if !visible {
        return Some(overlay_park_frame(window_id));
    }
    let (ow, oh) = output?;
    Some(overlay_live_frame(window_id, ow, oh, cover_menubar))
}

/// True when reported content size is the live overlay, not the park.
/// Floor is well above [`OVERLAY_PARK`] so a 2×2 buffer cannot join
/// Composition (that is the first-show flash).
pub fn overlay_geometry_is_live(width: i32, height: i32) -> bool {
    width >= 64 && height >= 64
}

/// Notification overlay: tight card stack at the top-right, not the full
/// usable area (empty space must click through to apps).
pub const NOTIFY_CARD_W: i32 = 340;
pub const NOTIFY_MARGIN_RIGHT: i32 = 12;
pub const NOTIFY_MARGIN_TOP: i32 = 6;
/// How far the stack starts above its rest y (drop-from-bar).
pub const NOTIFY_DROP: i32 = 14;

/// Live Frame for the notification overlay. `enter_t` is 0 at the start of
/// the drop (up, overlapping the bar's bottom edge) and 1 at rest.
pub fn notify_live_frame(
    window_id: u32,
    output_w: i32,
    output_h: i32,
    stack_h: i32,
    enter_t: f32,
) -> FrameUpdate {
    let max_h = (output_h - MENUBAR_HEIGHT).max(64);
    let w = NOTIFY_CARD_W.min(output_w.max(64));
    let h = stack_h.max(64).min(max_h);
    let rest_y = MENUBAR_HEIGHT + NOTIFY_MARGIN_TOP;
    let start_y = rest_y - NOTIFY_DROP;
    let t = enter_t.clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - t).powi(3);
    let y = start_y + ((rest_y - start_y) as f32 * e).round() as i32;
    let x = (output_w - w - NOTIFY_MARGIN_RIGHT).max(0);
    FrameUpdate {
        window_id,
        x,
        y,
        width: w,
        height: h,
        fullscreen: false,
    }
}

pub fn notify_overlay_frame(
    window_id: u32,
    visible: bool,
    output: Option<(i32, i32)>,
    stack_h: i32,
    enter_t: f32,
) -> Option<FrameUpdate> {
    if !visible {
        return Some(overlay_park_frame(window_id));
    }
    let (ow, oh) = output?;
    Some(notify_live_frame(window_id, ow, oh, stack_h, enter_t))
}

/// Live size for the menu overlay (dropdown / calendar / stat / chips).
///
/// A full-output swapchain is cheap on a GPU and disastrous on software
/// GL (llvmpipe): first show compiles pipelines and fills ~1M pixels,
/// pegging a core. Size the buffer to the card, like notify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuOverlaySpec {
    pub x: i32,
    /// Its top: the menubar's bottom edge for the menubar's menus.
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Extra height so the popover drop-shadow is not clipped.
pub const MENU_SHADOW_PAD: i32 = 16;

pub fn menu_live_frame(
    window_id: u32,
    output_w: i32,
    output_h: i32,
    spec: MenuOverlaySpec,
) -> FrameUpdate {
    let usable_h = (output_h - spec.y).max(64);
    let w = spec.width.max(64).min(output_w.max(64));
    let h = (spec.height + MENU_SHADOW_PAD).max(64).min(usable_h);
    let x = spec.x.clamp(0, (output_w - w).max(0));
    FrameUpdate {
        window_id,
        x,
        y: spec.y,
        width: w,
        height: h,
        fullscreen: false,
    }
}

pub fn menu_overlay_frame(
    window_id: u32,
    visible: bool,
    output: Option<(i32, i32)>,
    spec: MenuOverlaySpec,
) -> Option<FrameUpdate> {
    if !visible {
        return Some(overlay_park_frame(window_id));
    }
    let (ow, oh) = output?;
    Some(menu_live_frame(window_id, ow, oh, spec))
}

/// Live size for a centered overlay card (Super+K; the launcher can
/// reuse this later). Same park/live split as [`menu_overlay_frame`]:
/// dismissed is 2×2 off-output; shown is the card plus shadow pad, never
/// the full usable area. A 1080p wgpu fill on software GL pegs a core
/// on every `CursorMoved`.
///
/// `x`/`y` are the **card** origin (not including pad). The live Frame
/// grows by [`CARD_SHADOW_PAD`] on each edge so the modal drop-shadow
/// is not clipped. Visible + no output → `None` (no 1920 placeholder).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardOverlaySpec {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Extra pixels on each edge so the modal drop-shadow is not clipped.
/// Modal blur is 28 + 8px offset; menus only pad height ([`MENU_SHADOW_PAD`])
/// because they hang from the bar.
pub const CARD_SHADOW_PAD: i32 = 32;

/// Center a content card in the usable area below the menubar.
pub fn centered_card_spec(
    output_w: i32,
    output_h: i32,
    width: i32,
    height: i32,
) -> CardOverlaySpec {
    let usable_h = (output_h - MENUBAR_HEIGHT).max(1);
    let w = width.max(1);
    let h = height.max(1);
    CardOverlaySpec {
        x: ((output_w - w) / 2).max(0),
        y: MENUBAR_HEIGHT + ((usable_h - h) / 2).max(0),
        width: w,
        height: h,
    }
}

pub fn card_live_frame(
    window_id: u32,
    output_w: i32,
    output_h: i32,
    spec: CardOverlaySpec,
) -> FrameUpdate {
    let usable_y = MENUBAR_HEIGHT;
    let usable_h = (output_h - MENUBAR_HEIGHT).max(64);
    let pad = CARD_SHADOW_PAD;
    let w = (spec.width + 2 * pad).max(64).min(output_w.max(64));
    let h = (spec.height + 2 * pad).max(64).min(usable_h);
    let x = (spec.x - pad).clamp(0, (output_w - w).max(0));
    let y = (spec.y - pad).clamp(usable_y, usable_y + (usable_h - h).max(0));
    FrameUpdate {
        window_id,
        x,
        y,
        width: w,
        height: h,
        fullscreen: false,
    }
}

pub fn card_overlay_frame(
    window_id: u32,
    visible: bool,
    output: Option<(i32, i32)>,
    spec: CardOverlaySpec,
) -> Option<FrameUpdate> {
    if !visible {
        return Some(overlay_park_frame(window_id));
    }
    let (ow, oh) = output?;
    Some(card_live_frame(window_id, ow, oh, spec))
}

/// Inset, in pixels per edge, applied to a freshly-floated window. A float
/// with no remembered geometry centers in the usable area shrunk by this
/// much on every side — a clear "this window is floating" cue (and, until a
/// titlebar exists, the only visible proof the float key fired). 50px/side =
/// the window's dimensions drop by 100×100 off the full output.
pub const FLOAT_MARGIN: i32 = 50;

/// Floor for a floated window's dimensions after insetting, so a float from
/// a very small source rect can't collapse to a sliver (or go negative).
const MIN_FLOAT_DIM: i32 = 100;

/// gamescope's host nest at its first map (gamescope's default `-W/-H`).
/// After paint, zoning may resize the **host** surface; the nested game's
/// size is its own `-w/-h`, scaled to fit (`-S fit`, letterboxed).
const GAMESCOPE_NEST_W: i32 = 1920;
const GAMESCOPE_NEST_H: i32 = 1080;

/// The tab bar, docked left under the menubar (Next).
pub const BAR_WIDTH: i32 = 240;

/// The one gap of the grid (board 02): around the output's edges under the
/// menubar, between the bar and the tiles, and between tiles. What shows
/// through it is the ground (the background picture, or the black).
pub const GAP: i32 = 10;

/// Where tiles go: right of the bar, under the menubar, one gap in from
/// every edge.
pub fn content_rect(output_w: i32, output_h: i32) -> (i32, i32, i32, i32) {
    let x = GAP + BAR_WIDTH + GAP;
    let y = MENUBAR_HEIGHT + GAP;
    (x, y, (output_w - x - GAP).max(1), (output_h - y - GAP).max(1))
}

/// The tab bar's Frame: the grid's left column.
pub fn bar_frame(window_id: u32, output_w: i32, output_h: i32) -> FrameUpdate {
    let y = MENUBAR_HEIGHT + GAP;
    FrameUpdate {
        window_id,
        x: GAP,
        y,
        width: BAR_WIDTH.min(output_w.max(1)),
        height: (output_h - y - GAP).max(1),
        fullscreen: false,
    }
}

/// A tiled zone's rectangle: Full is the content area; a grid zone its
/// cells of it, which it keeps whether or not its neighbours have tabs.
/// Cells sit one [`GAP`] apart; a zone spanning several takes the gaps
/// between them.
pub fn tile_frame(
    window_id: u32,
    layout: &ZoneLayout,
    zone: Zone,
    output_w: i32,
    output_h: i32,
) -> FrameUpdate {
    let (cx, cy, cw, ch) = content_rect(output_w, output_h);
    let mut frame = FrameUpdate {
        window_id,
        x: cx,
        y: cy,
        width: cw,
        height: ch,
        fullscreen: false,
    };
    let Some(z) = (match zone {
        Zone::Grid(id) => layout.grid(id),
        _ => None,
    }) else {
        return frame;
    };
    let (x0, x1) = span(cx, cw, layout.columns, z.column, z.columns);
    let (y0, y1) = span(cy, ch, layout.rows, z.row, z.rows);
    frame.x = x0;
    frame.y = y0;
    frame.width = (x1 - x0).max(1);
    frame.height = (y1 - y0).max(1);
    frame
}

/// Cells `first .. first + n` of `count` along `len` from `from`: their
/// start and end, one [`GAP`] between neighbouring cells.
fn span(from: i32, len: i32, count: u8, first: u8, n: u8) -> (i32, i32) {
    let count = i32::from(count.max(1));
    let avail = (len - GAP * (count - 1)).max(1) as f32;
    let edge = |i: i32| from + (avail * i as f32 / count as f32).round() as i32 + i * GAP;
    let (first, n) = (i32::from(first), i32::from(n.max(1)));
    (edge(first), edge(first + n) - GAP)
}

/// A tab in Cinema: the whole output, menubar and bar included. The
/// compositor's fullscreen state forces clients past their own max size /
/// work-area assumptions.
pub fn cinema_frame(window_id: u32, output_w: i32, output_h: i32) -> FrameUpdate {
    FrameUpdate {
        window_id,
        x: 0,
        y: 0,
        width: output_w.max(1),
        height: output_h.max(1),
        fullscreen: true,
    }
}

/// Floating windows: which ones the shell leaves unsized, their
/// remembered rectangles, and the live geometry it insets from.
pub struct ZoningState {
    pub output_size: Option<(i32, i32)>,
    /// Floating windows → whether their app remembers the rectangle (a
    /// float in the bar's Floating zone does; a dialog does not).
    floats: HashMap<u32, bool>,
    /// Last known rectangle of each floating app, keyed by app_id. Fed by
    /// `Topic::WindowGeometry` for floating windows and by `Topic::FloatGeometry`
    /// replay at startup; restored when the app's window floats again.
    pub float_geometry: HashMap<String, FloatGeometry>,
    /// Latest on-screen rectangle of every known window, keyed by window_id.
    /// Fed by `Topic::WindowGeometry` (sola-river) for all windows; a float
    /// made from a tile shrinks from here.
    pub live_geometry: HashMap<u32, WindowGeometry>,
    /// Window IDs we last published as floating via `Topic::WindowFloating`.
    /// Lets `take_floating_changes` emit only on an actual transition
    /// (float ↔ unfloat), so a non-floating window never emits at all.
    floating_emitted: HashSet<u32>,
}

impl ZoningState {
    pub fn new() -> Self {
        Self {
            output_size: None,
            floats: HashMap::new(),
            float_geometry: HashMap::new(),
            live_geometry: HashMap::new(),
            floating_emitted: HashSet::new(),
        }
    }

    pub fn set_output_size(&mut self, geo: &OutputGeometry) {
        info!(
            width = geo.width,
            height = geo.height,
            "cached output geometry"
        );
        self.output_size = Some((geo.width, geo.height));
    }

    /// Diff each known window's float state against what was last published,
    /// returning `(window_id, floating)` for every change so the caller can
    /// emit `Topic::WindowFloating`. A window that has vanished from `known`
    /// emits `floating = false` once and is then forgotten. Idempotent: a
    /// second call with no transitions returns an empty vec.
    pub fn take_floating_changes(&mut self, known: &[u32]) -> Vec<(u32, bool)> {
        let mut out = Vec::new();
        let known_set: HashSet<u32> = known.iter().copied().collect();
        // Windows we'd published as floating that vanished: announce unfloat
        // once, then drop tracking.
        let gone: Vec<u32> = self
            .floating_emitted
            .iter()
            .copied()
            .filter(|w| !known_set.contains(w))
            .collect();
        for w in gone {
            self.floating_emitted.remove(&w);
            out.push((w, false));
        }
        for &w in known {
            let now = self.is_floating(w);
            let was = self.floating_emitted.contains(&w);
            if now && !was {
                self.floating_emitted.insert(w);
                out.push((w, true));
            } else if !now && was {
                self.floating_emitted.remove(&w);
                out.push((w, false));
            }
        }
        out
    }

    /// Forget per-window state for windows that closed.
    pub fn forget_windows(&mut self, removed_wids: &[u32]) {
        for wid in removed_wids {
            self.floats.remove(wid);
            self.live_geometry.remove(wid);
        }
    }

    /// `window_id` floats. A float that `remembers` gets its app's saved
    /// rectangle back; gamescope gets the nest size; anything else
    /// keeps the size it asks for (sola-river centers it).
    pub fn float(&mut self, app_id: &str, window_id: u32, remembers: bool) -> Option<FrameUpdate> {
        self.floats.insert(window_id, remembers);
        if remembers {
            if let Some(g) = self.float_geometry.get(app_id) {
                // Skip non-positive saved rects (poisoned FloatGeometry).
                if g.width > 0 && g.height > 0 {
                    return Some(FrameUpdate {
                        window_id,
                        x: g.x,
                        y: g.y,
                        width: g.width,
                        height: g.height,
                        fullscreen: false,
                    });
                }
            }
        }
        // gamescope: first map at the nest size, centered. Must match
        // initial `gamescope -W/-H` so the host can commit before a resize.
        if app_id.eq_ignore_ascii_case("gamescope") {
            let (out_w, out_h) = self.output_size?;
            return Some(gamescope_float_frame(window_id, out_w, out_h));
        }
        None
    }

    /// The float key on a tile: float it, shrunk in place from `from` (its
    /// tile), and remember that rectangle for its app.
    pub fn float_in_place(
        &mut self,
        app_id: &str,
        window_id: u32,
        from: Option<(i32, i32, i32, i32)>,
    ) -> Option<FrameUpdate> {
        self.floats.insert(window_id, true);
        // gamescope: always the nest aspect, never an inset tall column.
        let frame = if app_id.eq_ignore_ascii_case("gamescope") {
            self.output_size
                .map(|(w, h)| gamescope_float_frame(window_id, w, h))
        } else {
            from.or_else(|| {
                self.live_geometry
                    .get(&window_id)
                    .map(|g| (g.x, g.y, g.width, g.height))
            })
            .map(|(x, y, w, h)| inset_rect(window_id, x, y, w, h))
            .or_else(|| self.output_size.map(|(w, h)| float_frame(window_id, w, h)))
        }?;
        self.float_geometry.insert(
            app_id.to_string(),
            FloatGeometry {
                app_id: app_id.to_string(),
                x: frame.x,
                y: frame.y,
                width: frame.width,
                height: frame.height,
            },
        );
        info!(app_id = %app_id, window_id, ?frame, "floating window");
        Some(frame)
    }

    /// `window_id` is a tile again.
    pub fn unfloat(&mut self, window_id: u32) {
        self.floats.remove(&window_id);
    }

    /// The app's windows float and keep their rectangle: the loupe, and
    /// any app floated with the float key until it is tiled again.
    pub fn remembers(&self, app_id: &str) -> bool {
        app_id == "sola-scope" || self.float_geometry.contains_key(app_id)
    }

    /// Forget an app's float rectangle (it was tiled). True when there was one.
    pub fn forget_float(&mut self, app_id: &str) -> bool {
        self.float_geometry.remove(app_id).is_some()
    }

    /// Record a floating window's geometry against its app_id. Returns true if a
    /// new/changed `FloatGeometry` should be persisted. Only floats that
    /// remember (not dialogs) record.
    pub fn note_window_geometry(
        &mut self,
        app_id: &str,
        window_id: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> bool {
        if self.floats.get(&window_id) != Some(&true) {
            return false;
        }
        // Never persist a 0×0 (or otherwise non-positive) rect — a pre-init
        // dimensions blip; restoring it on relaunch emits Frame(0,0,0,0)
        // which breaks screenshots and forces a self-size loop.
        if width <= 0 || height <= 0 {
            return false;
        }
        let next = FloatGeometry {
            app_id: app_id.to_string(),
            x,
            y,
            width,
            height,
        };
        if self.float_geometry.get(app_id) == Some(&next) {
            return false;
        }
        self.float_geometry.insert(app_id.to_string(), next);
        true
    }

    /// Compute the menubar's Frame for a given output.
    pub fn menubar_frame(&self, window_id: u32) -> Option<FrameUpdate> {
        let (w, _h) = self.output_size?;
        Some(FrameUpdate {
            window_id,
            x: 0,
            y: 0,
            width: w,
            height: MENUBAR_HEIGHT,
            fullscreen: false,
        })
    }

    /// True while the window floats (the shell does not size it).
    pub fn is_floating(&self, window_id: u32) -> bool {
        self.floats.contains_key(&window_id)
    }
}

/// Default frame for a freshly-floated window with nothing to shrink
/// from: the content area inset by `FLOAT_MARGIN` on every edge.
fn float_frame(window_id: u32, output_w: i32, output_h: i32) -> FrameUpdate {
    let (x, y, w, h) = content_rect(output_w, output_h);
    inset_rect(window_id, x, y, w, h)
}

/// Float gamescope at the nest aspect (16:9), centered in the usable
/// area. Prefer the default nest size when it fits; otherwise scale 16:9 down
/// to the margin box. Avoids inheriting a tall zone host rect (heavy letterbox).
fn gamescope_float_frame(window_id: u32, output_w: i32, output_h: i32) -> FrameUpdate {
    let usable_h = (output_h - MENUBAR_HEIGHT).max(MIN_FLOAT_DIM);
    let max_w = (output_w - 2 * FLOAT_MARGIN).max(MIN_FLOAT_DIM);
    let max_h = (usable_h - 2 * FLOAT_MARGIN).max(MIN_FLOAT_DIM);

    let (w, h) = if GAMESCOPE_NEST_W <= max_w && GAMESCOPE_NEST_H <= max_h {
        (GAMESCOPE_NEST_W, GAMESCOPE_NEST_H)
    } else if max_w * GAMESCOPE_NEST_H <= max_h * GAMESCOPE_NEST_W {
        // Width-limited 16:9 (or nest aspect).
        let w = max_w;
        let h = ((w as i64 * GAMESCOPE_NEST_H as i64) / GAMESCOPE_NEST_W as i64) as i32;
        (w.max(MIN_FLOAT_DIM), h.max(MIN_FLOAT_DIM))
    } else {
        let h = max_h;
        let w = ((h as i64 * GAMESCOPE_NEST_W as i64) / GAMESCOPE_NEST_H as i64) as i32;
        (w.max(MIN_FLOAT_DIM), h.max(MIN_FLOAT_DIM))
    };

    let x = ((output_w - w) / 2).max(0);
    let y = MENUBAR_HEIGHT + ((usable_h - h) / 2).max(0);
    FrameUpdate {
        window_id,
        x,
        y,
        width: w,
        height: h,
        fullscreen: false,
    }
}

/// Inset a source rectangle by `FLOAT_MARGIN` on every edge (each dimension
/// clamped to at least `MIN_FLOAT_DIM`). This is how an explicit float
/// shrinks a window in place rather than recentering it on the output.
fn inset_rect(window_id: u32, x: i32, y: i32, w: i32, h: i32) -> FrameUpdate {
    FrameUpdate {
        window_id,
        x: x + FLOAT_MARGIN,
        y: y + FLOAT_MARGIN,
        width: (w - 2 * FLOAT_MARGIN).max(MIN_FLOAT_DIM),
        height: (h - 2 * FLOAT_MARGIN).max(MIN_FLOAT_DIM),
        fullscreen: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_output(w: i32, h: i32) -> ZoningState {
        let mut s = ZoningState::new();
        s.set_output_size(&OutputGeometry {
            width: w,
            height: h,
        });
        s
    }

    fn rect(f: &FrameUpdate) -> (i32, i32, i32, i32) {
        (f.x, f.y, f.width, f.height)
    }

    #[test]
    fn bar_and_two_halves_on_canto() {
        // Under the top row: gap + island + gap = 48 (board 03).
        assert_eq!(MENUBAR_HEIGHT + GAP, 48);
        assert_eq!(rect(&bar_frame(1, 1920, 1080)), (10, 48, 240, 1022));
        let l = ZoneLayout::default();
        assert_eq!(rect(&tile_frame(1, &l, Zone::Grid(1), 1920, 1080)), (260, 48, 820, 1022));
        assert_eq!(rect(&tile_frame(1, &l, Zone::Grid(2), 1920, 1080)), (1090, 48, 820, 1022));
    }

    #[test]
    fn three_columns_and_a_wide_zone_on_novus() {
        use sola_bus::topics::GridZone;
        let l = ZoneLayout {
            rows: 2,
            columns: 3,
            zones: vec![
                GridZone { id: 1, row: 0, column: 0, rows: 2, columns: 1 },
                GridZone { id: 2, row: 0, column: 1, rows: 1, columns: 2 },
                GridZone { id: 3, row: 1, column: 2, rows: 1, columns: 1 },
            ],
        };
        // Content: x 260.., w 5120 − 270 = 4850; y 48.., h 2160 − 58 = 2102.
        let one = rect(&tile_frame(1, &l, Zone::Grid(1), 5120, 2160));
        let two = rect(&tile_frame(1, &l, Zone::Grid(2), 5120, 2160));
        let three = rect(&tile_frame(1, &l, Zone::Grid(3), 5120, 2160));
        assert_eq!(one, (260, 48, 1610, 2102));
        assert_eq!(two.0, one.0 + one.2 + GAP);
        assert_eq!(two.0 + two.2, 260 + 4850);
        assert_eq!(three.1, two.1 + two.3 + GAP);
        assert_eq!(three.1 + three.3, 48 + 2102);
    }

    #[test]
    fn full_is_the_content_area() {
        let f = tile_frame(1, &ZoneLayout::default(), Zone::Fullscreen, 1920, 1080);
        assert_eq!((rect(&f), f.fullscreen), ((260, 48, 1650, 1022), false));
    }

    #[test]
    fn cinema_is_the_whole_output() {
        let f = cinema_frame(1, 1920, 1080);
        assert_eq!((rect(&f), f.fullscreen), ((0, 0, 1920, 1080), true));
    }

    #[test]
    fn floating_changes_emit_once_and_track_departures() {
        let mut s = ZoningState::new();
        s.float("a", 7, true);
        assert_eq!(s.take_floating_changes(&[7]), vec![(7, true)]);
        assert!(s.take_floating_changes(&[7]).is_empty());
        s.unfloat(7);
        assert_eq!(s.take_floating_changes(&[7]), vec![(7, false)]);
        s.float("a", 7, true);
        assert_eq!(s.take_floating_changes(&[7]), vec![(7, true)]);
        assert_eq!(s.take_floating_changes(&[]), vec![(7, false)]);
    }

    #[test]
    fn a_float_restores_its_apps_rectangle() {
        let mut s = state_with_output(1920, 1080);
        assert!(s.float("sola-scope", 3, true).is_none());
        assert!(s.note_window_geometry("sola-scope", 3, 100, 120, 300, 200));
        assert!(!s.note_window_geometry("sola-scope", 3, 100, 120, 300, 200));
        assert!(!s.note_window_geometry("sola-scope", 3, 0, 0, 0, 0));
        let f = s.float("sola-scope", 4, true).expect("saved rect");
        assert_eq!(rect(&f), (100, 120, 300, 200));
    }

    #[test]
    fn dialogs_do_not_record_geometry() {
        let mut s = state_with_output(1920, 1080);
        s.float("app", 5, false);
        assert!(s.is_floating(5));
        assert!(!s.note_window_geometry("app", 5, 10, 10, 300, 200));
    }

    #[test]
    fn float_key_shrinks_the_tile_in_place() {
        let mut s = state_with_output(1920, 1080);
        let f = s
            .float_in_place("sola-terminal", 7, Some((1090, 48, 820, 1022)))
            .expect("frame");
        assert_eq!(rect(&f), (1140, 98, 720, 922));
        assert!(s.is_floating(7));
        assert_eq!(s.float_geometry["sola-terminal"].x, 1140);
        // Nothing to shrink from: the content area, inset.
        let f = s.float_in_place("other", 8, None).expect("frame");
        assert_eq!(rect(&f), (310, 98, 1550, 922));
    }

    #[test]
    fn gamescope_floats_at_the_nest_aspect() {
        let mut s = state_with_output(1920, 1080);
        let f = s.float_in_place("gamescope", 9, Some((260, 48, 820, 1022))).expect("frame");
        let aspect = f.width as f32 / f.height as f32;
        assert!((aspect - 16.0 / 9.0).abs() < 0.01, "{aspect}");
        assert!(s.float("gamescope", 10, false).is_some());
    }

    #[test]
    fn overlay_open_rect_matches_usable_area() {
        let (x, y, w, h) = overlay_open_rect(5120, 2160);
        assert_eq!(x, 0.0);
        assert_eq!(y, MENUBAR_HEIGHT as f32);
        assert_eq!(w, 5120.0);
        assert_eq!(h, (2160 - MENUBAR_HEIGHT) as f32);
    }

    #[test]
    fn overlay_park_frame_meets_winit_wayland_min() {
        let f = overlay_park_frame(42);
        assert_eq!(
            (f.window_id, f.x, f.y, f.width, f.height),
            (
                42,
                OVERLAY_PARK_X,
                OVERLAY_PARK_Y,
                OVERLAY_PARK,
                OVERLAY_PARK
            )
        );
        assert!(
            f.width >= 2 && f.height >= 1,
            "winit MIN_WINDOW_SIZE is 2×1"
        );
        assert!(!f.fullscreen);
    }

    #[test]
    fn overlay_frame_parks_when_hidden_even_without_output() {
        let f = overlay_frame(7, false, None, false).unwrap();
        assert_eq!((f.width, f.height), (OVERLAY_PARK, OVERLAY_PARK));
    }

    #[test]
    fn overlay_frame_live_usable_area_or_full_output() {
        let launcher = overlay_frame(1, true, Some((5120, 2160)), false).unwrap();
        assert_eq!(
            (launcher.x, launcher.y, launcher.width, launcher.height),
            (0, MENUBAR_HEIGHT, 5120, 2160 - MENUBAR_HEIGHT)
        );
        let selection = overlay_frame(2, true, Some((5120, 2160)), true).unwrap();
        assert_eq!(
            (selection.x, selection.y, selection.width, selection.height),
            (0, 0, 5120, 2160)
        );
        assert!(overlay_frame(3, true, None, false).is_none());
    }

    #[test]
    fn menu_live_frame_is_the_card_not_the_output() {
        let spec = MenuOverlaySpec {
            x: 40,
            y: MENUBAR_HEIGHT,
            width: 220,
            height: 360,
        };
        let f = menu_live_frame(3, 1280, 800, spec);
        assert_eq!(f.x, 40);
        assert_eq!(f.y, MENUBAR_HEIGHT);
        assert_eq!(f.width, 220);
        assert_eq!(f.height, 360 + MENU_SHADOW_PAD);
        assert!(f.width * f.height < 1280 * (800 - MENUBAR_HEIGHT));
        assert!(overlay_geometry_is_live(f.width, f.height));

        let clamped = menu_live_frame(
            3,
            1280,
            800,
            MenuOverlaySpec {
                x: 1200,
                y: MENUBAR_HEIGHT,
                width: 220,
                height: 900,
            },
        );
        assert_eq!(clamped.x, 1280 - 220);
        assert_eq!(clamped.height, 800 - MENUBAR_HEIGHT);

        // The workspace menu hangs under the tab bar's head.
        let under = menu_live_frame(
            3,
            1280,
            800,
            MenuOverlaySpec {
                x: 20,
                y: 100,
                width: 240,
                height: 900,
            },
        );
        assert_eq!(under.y, 100);
        assert_eq!(under.height, 800 - 100);
        assert!(menu_overlay_frame(3, false, None, spec).is_some());
        assert!(menu_overlay_frame(3, true, None, spec).is_none());
    }

    #[test]
    fn card_live_frame_is_the_card_not_the_output() {
        let spec = CardOverlaySpec {
            x: 700,
            y: 200,
            width: 520,
            height: 500,
        };
        let f = card_live_frame(5, 1920, 1080, spec);
        assert_eq!(f.width, 520 + 2 * CARD_SHADOW_PAD);
        assert_eq!(f.height, 500 + 2 * CARD_SHADOW_PAD);
        assert_eq!(f.x, 700 - CARD_SHADOW_PAD);
        assert_eq!(f.y, 200 - CARD_SHADOW_PAD);
        let usable = 1920 * (1080 - MENUBAR_HEIGHT);
        assert!(
            f.width * f.height < usable,
            "card swapchain {}×{} must be well under usable {}×{}",
            f.width,
            f.height,
            1920,
            1080 - MENUBAR_HEIGHT
        );
        assert!(overlay_geometry_is_live(f.width, f.height));

        let parked = card_overlay_frame(5, false, None, spec).unwrap();
        assert_eq!(
            (parked.x, parked.y, parked.width, parked.height),
            overlay_park_tuple()
        );
        assert!(card_overlay_frame(5, true, None, spec).is_none());
    }

    #[test]
    fn shortcuts_live_frame_is_card_sized_not_1080p() {
        let spec = centered_card_spec(
            1920,
            1080,
            crate::shortcuts::view::CARD_WIDTH.round() as i32,
            crate::shortcuts::view::CARD_HEIGHT.round() as i32,
        );
        let f = card_live_frame(8, 1920, 1080, spec);
        let usable_w = 1920;
        let usable_h = 1080 - MENUBAR_HEIGHT;
        assert!(f.width < usable_w, "live width {} is full output", f.width);
        assert!(
            f.height < usable_h,
            "live height {} is full usable {}",
            f.height,
            usable_h
        );
        let live_px = f.width * f.height;
        let usable_px = usable_w * usable_h;
        assert!(
            live_px * 4 < usable_px,
            "Super+K live {}×{} ({} px) must stay well under usable {}×{} ({} px)",
            f.width,
            f.height,
            live_px,
            usable_w,
            usable_h,
            usable_px
        );
        assert!(f.y >= MENUBAR_HEIGHT);
        assert!(overlay_geometry_is_live(f.width, f.height));
        assert_eq!(
            card_overlay_frame(8, false, Some((1920, 1080)), spec)
                .unwrap()
                .width,
            OVERLAY_PARK
        );
        assert!(card_overlay_frame(8, true, None, spec).is_none());
    }

    fn overlay_park_tuple() -> (i32, i32, i32, i32) {
        (OVERLAY_PARK_X, OVERLAY_PARK_Y, OVERLAY_PARK, OVERLAY_PARK)
    }

    #[test]
    fn overlay_geometry_is_live_rejects_park() {
        assert!(!overlay_geometry_is_live(OVERLAY_PARK, OVERLAY_PARK));
        assert!(!overlay_geometry_is_live(2, 1));
        assert!(!overlay_geometry_is_live(32, 32));
        assert!(overlay_geometry_is_live(5120, 2132));
        assert!(overlay_geometry_is_live(64, 64));
        assert!(overlay_geometry_is_live(NOTIFY_CARD_W, 76));
    }

    #[test]
    fn notify_live_frame_top_right_drops_into_rest() {
        let start = notify_live_frame(9, 1920, 1080, 76, 0.0);
        assert_eq!(start.window_id, 9);
        assert_eq!(start.width, NOTIFY_CARD_W);
        assert_eq!(start.height, 76.max(64));
        assert_eq!(start.x, 1920 - NOTIFY_CARD_W - NOTIFY_MARGIN_RIGHT);
        assert_eq!(start.y, MENUBAR_HEIGHT + NOTIFY_MARGIN_TOP - NOTIFY_DROP);

        let rest = notify_live_frame(9, 1920, 1080, 76, 1.0);
        assert_eq!(rest.y, MENUBAR_HEIGHT + NOTIFY_MARGIN_TOP);
        assert_eq!(rest.x, start.x);

        assert!(notify_overlay_frame(9, false, None, 76, 1.0).is_some());
        assert!(notify_overlay_frame(9, true, None, 76, 1.0).is_none());
    }

    #[test]
    fn menubar_frame_spans_full_width_at_top() {
        let s = state_with_output(1920, 1080);
        let f = s.menubar_frame(1).unwrap();
        assert_eq!(f.x, 0);
        assert_eq!(f.y, 0);
        assert_eq!(f.width, 1920);
        assert_eq!(f.height, MENUBAR_HEIGHT);
    }
}
