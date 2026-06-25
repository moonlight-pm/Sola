//! Zone-snapping state: output geometry, window-zone assignments,
//! config persistence, and frame computation.
//!
//! All methods that previously called `ctx.emit(Topic::Frame(...))` or
//! `ctx.emit(Topic::Zones(...))` now return the data instead; the caller
//! (Shell::update) emits on their behalf.
use std::collections::{HashMap, HashSet};

use sola_bus::topics::{FloatGeometry, FrameUpdate, OutputGeometry, WindowGeometry, Zone};
use sola_core::KeyCode;
use tracing::{info, warn};

pub const MENUBAR_HEIGHT: i32 = 28;

/// Inset, in pixels per edge, applied to a freshly-floated window. A float
/// with no remembered geometry centers in the usable area shrunk by this
/// much on every side — a clear "this window is floating" cue (and, until a
/// titlebar exists, the only visible proof the float key fired). 50px/side =
/// the window's dimensions drop by 100×100 off the full output.
pub const FLOAT_MARGIN: i32 = 50;

/// Floor for a floated window's dimensions after insetting, so a float from
/// a very small source rect can't collapse to a sliver (or go negative).
const MIN_FLOAT_DIM: i32 = 100;

pub struct ZoningState {
    pub output_size: Option<(i32, i32)>,
    pub focused_app_id: Option<String>,
    /// Zone assignments by app_id, owned by the Zones persistent
    /// topic. Populated on subscription from the bus sticky replay
    /// and mutated locally when the user snaps a sola-* window.
    app_zone_config: HashMap<String, Zone>,
    /// Runtime zone assignments by window_id. Only windows that have
    /// been explicitly zoned (by the user or from config) are here.
    /// Windows NOT in this map keep their own geometry.
    pub window_zones: HashMap<u32, Zone>,
    /// Window IDs that have already had their config zone applied.
    /// Keyed per-window (not per-app) so every window of a multi-window
    /// app gets its config zone applied once — fixes the Steam case where
    /// only the first window of an app was being auto-zoned.
    config_applied: HashSet<u32>,
    /// Set by `handle_key` when the user snaps a sola-* window.
    /// Consumed by `take_zones_update` so the caller knows to emit
    /// a fresh `Topic::Zones` for persistence.
    zones_dirty: bool,
    /// Last known rectangle of each floating app, keyed by app_id. Fed by
    /// `Topic::WindowGeometry` for floating windows and by `Topic::FloatGeometry`
    /// replay at startup; consumed by `apply_config_zone` to restore on relaunch.
    pub float_geometry: HashMap<String, FloatGeometry>,
    /// Latest on-screen rectangle of every known window, keyed by window_id.
    /// Fed by `Topic::WindowGeometry` (sola-river) for all windows; read by
    /// `current_rect` so an explicit float can shrink the window in place.
    pub live_geometry: HashMap<u32, WindowGeometry>,
}

impl ZoningState {
    pub fn new() -> Self {
        Self {
            output_size: None,
            focused_app_id: None,
            app_zone_config: HashMap::new(),
            window_zones: HashMap::new(),
            config_applied: HashSet::new(),
            zones_dirty: false,
            float_geometry: HashMap::new(),
            live_geometry: HashMap::new(),
        }
    }

    /// Replace the app→zone map with a snapshot from the bus.
    /// Clears `config_applied` so `apply_config_zone` re-evaluates
    /// against the new mapping for each known window.
    pub fn set_zones(&mut self, zones: HashMap<String, Zone>) {
        self.app_zone_config = zones;
        self.config_applied.clear();
    }

    /// Take the current zone map if the local state has mutated
    /// since the last call. Returns `None` when nothing changed, so
    /// the caller can skip the emit/persist cost.
    ///
    /// Caller emits `Topic::Zones` in Task 10.
    pub fn take_zones_update(&mut self) -> Option<HashMap<String, Zone>> {
        if !self.zones_dirty {
            return None;
        }
        self.zones_dirty = false;
        Some(self.app_zone_config.clone())
    }

    pub fn set_output_size(&mut self, geo: &OutputGeometry) {
        info!(
            width = geo.width,
            height = geo.height,
            "cached output geometry"
        );
        self.output_size = Some((geo.width, geo.height));
    }

    pub fn set_focused(&mut self, app_id: String) {
        self.focused_app_id = Some(app_id);
    }

    /// Forget config-zone tracking for all windows of a departed app.
    /// Pass the set of window IDs that are being removed from the registry.
    /// Called from `on_windows` when an app's windows disappear so that
    /// re-launching the app gets a fresh auto-zone pass.
    pub fn forget_windows(&mut self, removed_wids: &[u32]) {
        for wid in removed_wids {
            self.config_applied.remove(wid);
            self.live_geometry.remove(wid);
        }
    }

    /// Record a floating window's geometry against its app_id. Returns true if a
    /// new/changed `FloatGeometry` should be persisted. Ignores windows that are
    /// not currently floating — only floats remember their rectangle.
    pub fn note_window_geometry(
        &mut self,
        app_id: &str,
        window_id: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> bool {
        if self.window_zones.get(&window_id) != Some(&Zone::Float) {
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

    /// Apply the config zone to a window if its app has a saved zone
    /// and this specific window hasn't been auto-zoned yet this session.
    /// Applies to every app, sola or external, so a restored window
    /// (e.g. a relaunched Helium) lands back in its last zone.
    ///
    /// Keyed per window_id (not per app_id) so every window of a
    /// multi-window app (e.g. Steam main + popups) receives its config
    /// zone once.
    ///
    /// Caller emits `Topic::Frame` for the returned value in Task 10.
    pub fn apply_config_zone(&mut self, app_id: &str, window_id: u32) -> Option<FrameUpdate> {
        if self.config_applied.contains(&window_id) {
            return None;
        }
        let zone = self.app_zone_config.get(app_id).copied()?;
        // Floating windows aren't snapped to a zone rect. Record the
        // assignment so it isn't retried each Windows event, then restore the
        // float's remembered rectangle if we have one, else float to the
        // default centered inset so a first launch is visibly floating.
        if matches!(zone, Zone::Float) {
            self.config_applied.insert(window_id);
            self.window_zones.insert(window_id, zone);
            // The restored size rides sola-river's first-`dimensions` gate
            // (deferred until the surface initializes); position applies
            // immediately. So restore can't reproduce the resize-before-init crash.
            if let Some(g) = self.float_geometry.get(app_id) {
                return Some(FrameUpdate {
                    window_id,
                    x: g.x,
                    y: g.y,
                    width: g.width,
                    height: g.height,
                    fullscreen: false,
                });
            }
            // No remembered geometry: emit the default inset frame so the
            // window is visibly floating. Needs output geometry; if it hasn't
            // arrived a later Apps event retries once OutputGeometry is cached.
            return self.output_size.map(|(w, h)| float_frame(window_id, w, h));
        }
        // If geometry hasn't arrived yet we can't compute the frame. Bail
        // without mutating state so a later Apps event retries once
        // OutputGeometry has been cached.
        let (w, h) = self.output_size?;
        self.config_applied.insert(window_id);
        self.window_zones.insert(window_id, zone);
        Some(compute_frame(zone, window_id, w, h))
    }

    /// Handle a zone snap keycode for the focused window.
    ///
    /// Returns the `FrameUpdate` to emit. Caller emits `Topic::Frame` and
    /// calls `take_zones_update()` → `Topic::Zones` in Task 10.
    pub fn handle_key(&mut self, code: u32, focused_window_id: Option<u32>) -> Option<FrameUpdate> {
        let zone = zone_for_keycode(code)?;

        let app_id = match self.focused_app_id.clone() {
            Some(id) => id,
            None => {
                warn!("zone key pressed but no focused app");
                return None;
            }
        };

        let window_id = match focused_window_id {
            Some(wid) => wid,
            None => {
                warn!("zone key pressed but no focused window_id");
                return None;
            }
        };

        // Floating: record + persist the assignment, then re-center the
        // window to the default inset frame. Explicit float always resets to
        // the inset (predictable); relaunch restores saved geometry via
        // apply_config_zone. Unfloat is just pressing any other Meta+Numpad
        // zone key, which overwrites the zone here and snaps as usual.
        if matches!(zone, Zone::Float) {
            // Inset from the window's current rect (read BEFORE we overwrite
            // its zone), so an explicit float shrinks it in place. Fall back
            // to the centered output inset when the current rect is unknown,
            // or to no frame at all when we lack output geometry.
            let frame = self
                .current_rect(window_id, &app_id)
                .map(|(x, y, w, h)| inset_rect(window_id, x, y, w, h))
                .or_else(|| self.output_size.map(|(w, h)| float_frame(window_id, w, h)));
            self.window_zones.insert(window_id, zone);
            self.app_zone_config.insert(app_id.clone(), zone);
            self.config_applied.insert(window_id);
            self.zones_dirty = true;
            info!(app_id = %app_id, window_id, ?frame, "floating window");
            return frame;
        }

        let (w, h) = match self.output_size {
            Some(s) => s,
            None => {
                warn!("zone key pressed but no output geometry cached");
                return None;
            }
        };

        let frame = compute_frame(zone, window_id, w, h);
        info!(
            app_id = %app_id,
            window_id,
            ?zone,
            x = frame.x,
            y = frame.y,
            width = frame.width,
            height = frame.height,
            "snapping to zone"
        );

        self.window_zones.insert(window_id, zone);

        // Persist the zone for every app — sola or external — so layouts
        // survive a restart and re-apply when the app's window reappears.
        self.app_zone_config.insert(app_id, zone);
        self.config_applied.insert(window_id);
        self.zones_dirty = true;

        Some(frame)
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

    /// Default frame: full output area below the menubar.
    pub fn default_app_frame(&self, window_id: u32) -> Option<FrameUpdate> {
        let (w, h) = self.output_size?;
        Some(FrameUpdate {
            window_id,
            x: 0,
            y: MENUBAR_HEIGHT,
            width: w,
            height: h - MENUBAR_HEIGHT,
            fullscreen: false,
        })
    }

    /// The focused window's current on-screen rectangle, as best the shell
    /// knows it. A non-Float **zone** is authoritative: it's where the shell
    /// put the window, and the app's configured zone is replayed from
    /// persisted `Zones` immediately at startup — so this works even in the
    /// window right after a restart, before per-window state or live geometry
    /// has caught up. The runtime per-window zone wins, then the app's
    /// configured zone, then the live rect reported by sola-river (for
    /// genuinely unzoned, self-sized windows). `None` if nothing is known.
    fn current_rect(&self, window_id: u32, app_id: &str) -> Option<(i32, i32, i32, i32)> {
        let zone = self
            .window_zones
            .get(&window_id)
            .or_else(|| self.app_zone_config.get(app_id))
            .copied();
        if let Some(zone) = zone {
            if !matches!(zone, Zone::Float) {
                if let Some((w, h)) = self.output_size {
                    let f = compute_frame(zone, window_id, w, h);
                    return Some((f.x, f.y, f.width, f.height));
                }
            }
        }
        self.live_geometry
            .get(&window_id)
            .map(|g| (g.x, g.y, g.width, g.height))
    }

    pub fn window_frame(&self, window_id: u32) -> Option<FrameUpdate> {
        let zone = self.window_zones.get(&window_id)?;
        // A floating window has no zone-computed frame — Float's rect is 0×0,
        // which would size the window to nothing (clients then self-size to
        // full screen). Its size is owned by handle_key/apply_config_zone, so
        // callers must not re-impose a frame here.
        if matches!(zone, Zone::Float) {
            return None;
        }
        let (w, h) = self.output_size?;
        Some(compute_frame(*zone, window_id, w, h))
    }

    /// True while the window is floating. Callers that re-broadcast frames
    /// (e.g. `emit_all_frames`) skip these so a float keeps the size it was
    /// given instead of being clobbered back to a default/zero frame.
    pub fn is_floating(&self, window_id: u32) -> bool {
        matches!(self.window_zones.get(&window_id), Some(Zone::Float))
    }
}

pub const ZONING_KEYCODES: &[u32] = &[
    KeyCode::KP_8.raw(),
    KeyCode::KP_4.raw(),
    KeyCode::KP_5.raw(),
    KeyCode::KP_6.raw(),
    KeyCode::KP_2.raw(),
    KeyCode::KP_0.raw(),
    KeyCode::KP_EQUAL.raw(),
    KeyCode::KP_DECIMAL.raw(),
    KeyCode::KP_ENTER.raw(),
    KeyCode::KP_MULTIPLY.raw(),
];

fn zone_for_keycode(code: u32) -> Option<Zone> {
    match code {
        c if c == KeyCode::KP_8.raw() => Some(Zone::TopMiddle),
        c if c == KeyCode::KP_4.raw() => Some(Zone::Left),
        c if c == KeyCode::KP_5.raw() => Some(Zone::FullMiddle),
        c if c == KeyCode::KP_6.raw() => Some(Zone::Right),
        c if c == KeyCode::KP_2.raw() => Some(Zone::BottomMiddle),
        c if c == KeyCode::KP_0.raw() => Some(Zone::Fullscreen),
        c if c == KeyCode::KP_EQUAL.raw() => Some(Zone::Top),
        c if c == KeyCode::KP_DECIMAL.raw() => Some(Zone::Bottom),
        c if c == KeyCode::KP_ENTER.raw() => Some(Zone::Cinema),
        c if c == KeyCode::KP_MULTIPLY.raw() => Some(Zone::Float),
        _ => None,
    }
}

fn compute_frame(zone: Zone, window_id: u32, output_w: i32, output_h: i32) -> FrameUpdate {
    let (xp, yp, wp, hp) = zone.rect();
    // Cinema covers the whole output including the menubar; every other
    // zone sits under the menubar and is sized against the usable area.
    let (offset_y, usable_h) = if matches!(zone, Zone::Cinema) {
        (0, output_h)
    } else {
        (MENUBAR_HEIGHT, output_h - MENUBAR_HEIGHT)
    };

    let x = (xp * output_w as f64).round() as i32;
    let y = offset_y + (yp * usable_h as f64).round() as i32;
    let w = (wp * output_w as f64).round() as i32;
    let h = (hp * usable_h as f64).round() as i32;

    FrameUpdate {
        window_id,
        x,
        y,
        width: w,
        height: h,
        // Cinema = "true fullscreen treatment": the compositor sends
        // xdg-shell fullscreen state, which forces clients past their
        // own max_size / work-area assumptions. Every other zone is a
        // normal toplevel.
        fullscreen: matches!(zone, Zone::Cinema),
    }
}


/// Default frame for a freshly-floated window: the usable area (below the
/// menubar) inset by `FLOAT_MARGIN` on every edge, so the window sits
/// centered with a uniform margin. Used when a float has no remembered
/// geometry; a float *with* saved geometry restores that instead.
fn float_frame(window_id: u32, output_w: i32, output_h: i32) -> FrameUpdate {
    FrameUpdate {
        window_id,
        x: FLOAT_MARGIN,
        y: MENUBAR_HEIGHT + FLOAT_MARGIN,
        width: output_w - 2 * FLOAT_MARGIN,
        height: (output_h - MENUBAR_HEIGHT) - 2 * FLOAT_MARGIN,
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
        s.set_output_size(&OutputGeometry { width: w, height: h });
        s
    }

    // Helper: compute a frame for a zone on a 1920×1080 output.
    fn frame_for(zone: Zone) -> FrameUpdate {
        compute_frame(zone, 1, 1920, 1080)
    }

    // Usable height for most zones (below menubar).
    const UH: i32 = 1080 - MENUBAR_HEIGHT; // 1052

    // --- Zone geometry parity with legacy shell ---

    #[test]
    fn zone_left() {
        // Zone::Left rect = (0.0, 0.0, 0.28, 1.0)
        let f = frame_for(Zone::Left);
        assert_eq!(f.x, 0);
        assert_eq!(f.y, MENUBAR_HEIGHT);
        assert_eq!(f.width, (0.28 * 1920.0f64).round() as i32);
        assert_eq!(f.height, UH);
        assert!(!f.fullscreen);
    }

    #[test]
    fn zone_right() {
        // Zone::Right rect = (0.72, 0.0, 0.28, 1.0)
        let f = frame_for(Zone::Right);
        assert_eq!(f.x, (0.72 * 1920.0f64).round() as i32);
        assert_eq!(f.y, MENUBAR_HEIGHT);
        assert_eq!(f.width, (0.28 * 1920.0f64).round() as i32);
        assert_eq!(f.height, UH);
        assert!(!f.fullscreen);
    }

    #[test]
    fn zone_top_middle() {
        // Zone::TopMiddle rect = (0.28, 0.0, 0.44, 0.7)
        let f = frame_for(Zone::TopMiddle);
        assert_eq!(f.x, (0.28 * 1920.0f64).round() as i32);
        assert_eq!(f.y, MENUBAR_HEIGHT);
        assert_eq!(f.width, (0.44 * 1920.0f64).round() as i32);
        assert_eq!(f.height, (0.7 * UH as f64).round() as i32);
        assert!(!f.fullscreen);
    }

    #[test]
    fn zone_bottom_middle() {
        // Zone::BottomMiddle rect = (0.28, 0.7, 0.44, 0.3)
        let f = frame_for(Zone::BottomMiddle);
        assert_eq!(f.x, (0.28 * 1920.0f64).round() as i32);
        assert_eq!(f.y, MENUBAR_HEIGHT + (0.7 * UH as f64).round() as i32);
        assert_eq!(f.width, (0.44 * 1920.0f64).round() as i32);
        assert_eq!(f.height, (0.3 * UH as f64).round() as i32);
        assert!(!f.fullscreen);
    }

    #[test]
    fn zone_full_middle() {
        // Zone::FullMiddle rect = (0.28, 0.0, 0.44, 1.0)
        let f = frame_for(Zone::FullMiddle);
        assert_eq!(f.x, (0.28 * 1920.0f64).round() as i32);
        assert_eq!(f.y, MENUBAR_HEIGHT);
        assert_eq!(f.width, (0.44 * 1920.0f64).round() as i32);
        assert_eq!(f.height, UH);
        assert!(!f.fullscreen);
    }

    #[test]
    fn zone_fullscreen() {
        // Zone::Fullscreen rect = (0.0, 0.0, 1.0, 1.0) — still below menubar
        let f = frame_for(Zone::Fullscreen);
        assert_eq!(f.x, 0);
        assert_eq!(f.y, MENUBAR_HEIGHT);
        assert_eq!(f.width, 1920);
        assert_eq!(f.height, UH);
        assert!(!f.fullscreen);
    }

    #[test]
    fn zone_top() {
        // Zone::Top rect = (0.0, 0.0, 1.0, 0.7)
        let f = frame_for(Zone::Top);
        assert_eq!(f.x, 0);
        assert_eq!(f.y, MENUBAR_HEIGHT);
        assert_eq!(f.width, 1920);
        assert_eq!(f.height, (0.7 * UH as f64).round() as i32);
        assert!(!f.fullscreen);
    }

    #[test]
    fn zone_bottom() {
        // Zone::Bottom rect = (0.0, 0.7, 1.0, 0.3)
        let f = frame_for(Zone::Bottom);
        assert_eq!(f.x, 0);
        assert_eq!(f.y, MENUBAR_HEIGHT + (0.7 * UH as f64).round() as i32);
        assert_eq!(f.width, 1920);
        assert_eq!(f.height, (0.3 * UH as f64).round() as i32);
        assert!(!f.fullscreen);
    }

    #[test]
    fn zone_cinema_covers_full_output_with_fullscreen_flag() {
        // Zone::Cinema rect = (0.0, 0.0, 1.0, 1.0) — covers full output, no menubar offset
        let f = frame_for(Zone::Cinema);
        assert_eq!(f.x, 0);
        assert_eq!(f.y, 0);
        assert_eq!(f.width, 1920);
        assert_eq!(f.height, 1080);
        assert!(f.fullscreen, "Cinema must set fullscreen flag");
    }

    // --- ZoningState integration ---

    #[test]
    fn handle_key_returns_none_without_geometry() {
        let mut s = ZoningState::new();
        s.set_focused("sola-browser".to_string());
        let result = s.handle_key(KeyCode::KP_4.raw(), Some(42));
        assert!(result.is_none(), "no geometry cached → no frame");
    }

    #[test]
    fn handle_key_returns_none_without_focused_app() {
        let mut s = state_with_output(1920, 1080);
        let result = s.handle_key(KeyCode::KP_4.raw(), Some(42));
        assert!(result.is_none(), "no focused app → no frame");
    }

    #[test]
    fn handle_key_snaps_window_and_marks_sola_zone_dirty() {
        let mut s = state_with_output(1920, 1080);
        s.set_focused("sola-browser".to_string());
        let frame = s.handle_key(KeyCode::KP_4.raw(), Some(99));
        assert!(frame.is_some());
        assert_eq!(frame.unwrap().window_id, 99);
        // sola-* app: zones_dirty should be set
        let update = s.take_zones_update();
        assert!(update.is_some());
        // second take returns None
        assert!(s.take_zones_update().is_none());
    }

    #[test]
    fn handle_key_persists_external_app_zone() {
        let mut s = state_with_output(1920, 1080);
        s.set_focused("helium".to_string());
        let frame = s.handle_key(KeyCode::KP_6.raw(), Some(7));
        assert!(frame.is_some());
        // External apps now persist their zone (Topic::Zones), same as sola-*.
        assert!(s.take_zones_update().is_some(), "external app must dirty zones");

        // …and the saved zone re-applies to a fresh window of that app, so a
        // relaunched external app lands back where it was.
        let reapplied = s.apply_config_zone("helium", 8);
        assert!(reapplied.is_some(), "saved external zone must re-apply on new window");
    }

    #[test]
    fn kp_multiply_maps_to_float() {
        assert_eq!(zone_for_keycode(KeyCode::KP_MULTIPLY.raw()), Some(Zone::Float));
    }

    #[test]
    fn zoning_keycodes_include_float_key() {
        assert!(ZONING_KEYCODES.contains(&KeyCode::KP_MULTIPLY.raw()));
    }

    #[test]
    fn handle_key_float_records_zone_emits_inset_frame() {
        let mut s = state_with_output(1920, 1080);
        s.set_focused("UnrealEditor".to_string());
        let frame = s
            .handle_key(KeyCode::KP_MULTIPLY.raw(), Some(42))
            .expect("Float must emit the default inset frame");
        // Centered inset: 50px margin per edge, below the 28px menubar.
        assert_eq!((frame.x, frame.y, frame.width, frame.height), (50, 78, 1820, 952));
        assert!(!frame.fullscreen);
        // And the assignment is recorded + persisted.
        assert_eq!(s.window_zones.get(&42).copied(), Some(Zone::Float));
        let update = s.take_zones_update().expect("Float must dirty zones");
        assert_eq!(update.get("UnrealEditor").copied(), Some(Zone::Float));
    }

    #[test]
    fn apply_config_zone_float_emits_inset_frame_without_saved_geometry() {
        let mut s = state_with_output(1920, 1080);
        let mut zones = std::collections::HashMap::new();
        zones.insert("UnrealEditor".to_string(), Zone::Float);
        s.set_zones(zones);

        let frame = s
            .apply_config_zone("UnrealEditor", 7)
            .expect("Float config with no saved geometry emits the inset frame");
        assert_eq!((frame.x, frame.y, frame.width, frame.height), (50, 78, 1820, 952));
        assert_eq!(s.window_zones.get(&7).copied(), Some(Zone::Float));
        // Marked applied so it isn't retried every Windows event.
        assert!(s.apply_config_zone("UnrealEditor", 7).is_none());
    }

    #[test]
    fn floating_window_geometry_is_recorded_by_app() {
        let mut z = ZoningState::new();
        z.window_zones.insert(7, Zone::Float);
        let changed = z.note_window_geometry("UnrealEditor", 7, 10, 20, 1280, 800);
        assert!(changed);
        let g = z.float_geometry.get("UnrealEditor").expect("recorded");
        assert_eq!((g.x, g.y, g.width, g.height), (10, 20, 1280, 800));
        // Re-recording the same rectangle is a no-op (nothing to persist).
        assert!(!z.note_window_geometry("UnrealEditor", 7, 10, 20, 1280, 800));
        // A non-floating window's geometry is ignored.
        z.window_zones.insert(8, Zone::Left);
        assert!(!z.note_window_geometry("Helium", 8, 0, 0, 100, 100));
        assert!(z.float_geometry.get("Helium").is_none());
    }

    #[test]
    fn float_with_saved_geometry_restores_a_frame() {
        let mut z = ZoningState::new();
        z.set_output_size(&OutputGeometry { width: 5120, height: 2160 });
        z.set_focused("UnrealEditor".to_string());
        z.handle_key(KeyCode::KP_MULTIPLY.raw(), Some(3)); // float it (records Zone::Float)
        z.float_geometry.insert(
            "UnrealEditor".into(),
            FloatGeometry { app_id: "UnrealEditor".into(), x: 100, y: 50, width: 1280, height: 800 },
        );
        // config_applied was set by handle_key for window 3; restore targets a
        // *fresh* window (relaunch → new window_id), so use a different id.
        let frame = z
            .apply_config_zone("UnrealEditor", 9)
            .expect("saved geometry → restore frame");
        assert_eq!((frame.x, frame.y, frame.width, frame.height), (100, 50, 1280, 800));
        assert!(!frame.fullscreen);

        // A float without saved geometry now floats to the default inset
        // (5120×2160 → 50px margins, below the 28px menubar).
        z.set_focused("Blender".to_string());
        z.handle_key(KeyCode::KP_MULTIPLY.raw(), Some(4));
        let frame = z
            .apply_config_zone("Blender", 10)
            .expect("no saved geometry → default inset frame");
        assert_eq!((frame.x, frame.y, frame.width, frame.height), (50, 78, 5020, 2032));
    }

    #[test]
    fn float_insets_from_current_zone_rect() {
        // A window snapped to the right zone, then floated, shrinks in place —
        // inset from the right-zone rect, not recentered on the full output.
        let mut s = state_with_output(1920, 1080);
        s.set_focused("helium".to_string());
        s.handle_key(KeyCode::KP_6.raw(), Some(7)); // snap to Right first
        let frame = s
            .handle_key(KeyCode::KP_MULTIPLY.raw(), Some(7))
            .expect("float must emit a frame");
        // Right = (0.72,0,0.28,1.0) on 1920×1080 below the 28px menubar,
        // inset 50px/edge: x 1382→1432, y 28→78, w 538→438, h 1052→952.
        assert_eq!((frame.x, frame.y, frame.width, frame.height), (1432, 78, 438, 952));
    }

    #[test]
    fn float_insets_from_live_geometry() {
        // When sola-river has reported a live rect, float insets from that
        // exact geometry regardless of zone.
        let mut s = state_with_output(1920, 1080);
        s.set_focused("helium".to_string());
        s.live_geometry.insert(
            5,
            WindowGeometry { window_id: 5, x: 300, y: 200, width: 800, height: 600 },
        );
        let frame = s
            .handle_key(KeyCode::KP_MULTIPLY.raw(), Some(5))
            .expect("float must emit a frame");
        assert_eq!((frame.x, frame.y, frame.width, frame.height), (350, 250, 700, 500));
    }

    #[test]
    fn window_frame_is_none_for_floating_window() {
        // emit_all_frames must not re-frame a float: Float's zone rect is 0×0,
        // which would size the window to nothing and the client would self-size
        // to full screen. Regression for "float keeps flouting to full screen".
        let mut s = state_with_output(1920, 1080);
        s.window_zones.insert(7, Zone::Float);
        assert!(s.window_frame(7).is_none(), "float must not be zone-framed");
        assert!(s.is_floating(7));
        // A normally-zoned window still produces a frame.
        s.window_zones.insert(8, Zone::Right);
        assert!(s.window_frame(8).is_some());
        assert!(!s.is_floating(8));
    }

    #[test]
    fn float_insets_from_app_zone_config_when_window_zone_unset() {
        // Post-restart race: the per-window zone hasn't been applied yet, but
        // the app's configured zone (replayed from persisted Zones) is known.
        // Float must still inset from that zone, NOT blow up to the centered
        // output fallback. Regression for "float flouts out to full screen".
        let mut s = state_with_output(1920, 1080);
        s.set_focused("sola-settings".to_string());
        let mut zones = std::collections::HashMap::new();
        zones.insert("sola-settings".to_string(), Zone::Right);
        s.set_zones(zones); // populates app_zone_config, NOT window_zones
        assert!(s.window_zones.get(&7).is_none(), "precondition: window zone unset");
        let frame = s
            .handle_key(KeyCode::KP_MULTIPLY.raw(), Some(7))
            .expect("float must emit a frame");
        // Right (0.72,0,0.28,1.0) on 1920×1080 inset 50/edge → (1432,78,438,952).
        assert_eq!((frame.x, frame.y, frame.width, frame.height), (1432, 78, 438, 952));
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
