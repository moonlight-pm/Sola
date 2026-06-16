//! Zone-snapping state: output geometry, window-zone assignments,
//! config persistence, and frame computation.
//!
//! All methods that previously called `ctx.emit(Topic::Frame(...))` or
//! `ctx.emit(Topic::Zones(...))` now return the data instead; the caller
//! (Shell::update) emits on their behalf.
use std::collections::{HashMap, HashSet};

use sola_bus::topics::{FrameUpdate, OutputGeometry, Zone};
use sola_core::KeyCode;
use tracing::{info, warn};

pub const MENUBAR_HEIGHT: i32 = 28;

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
        }
    }

    /// Apply the config zone to a window if its app has a saved zone
    /// and this specific window hasn't been auto-zoned yet this session.
    /// Only sola-* apps persist zones — external apps are zoned manually.
    ///
    /// Keyed per window_id (not per app_id) so every window of a
    /// multi-window app (e.g. Steam main + popups) receives its config
    /// zone once.
    ///
    /// Caller emits `Topic::Frame` for the returned value in Task 10.
    pub fn apply_config_zone(&mut self, app_id: &str, window_id: u32) -> Option<FrameUpdate> {
        if !app_id.starts_with("sola-") || self.config_applied.contains(&window_id) {
            return None;
        }
        let zone = self.app_zone_config.get(app_id).copied()?;
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

        let (w, h) = match self.output_size {
            Some(s) => s,
            None => {
                warn!("zone key pressed but no output geometry cached");
                return None;
            }
        };

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

        // Only persist zone config for sola-* apps. External apps
        // are zoned manually each session.
        if app_id.starts_with("sola-") {
            self.app_zone_config.insert(app_id, zone);
            self.config_applied.insert(window_id);
            self.zones_dirty = true;
        }

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

    

    /// Compute the Frame for an explicitly-zoned window.
    /// Returns None if the window has no zone assignment.
    pub fn window_frame(&self, window_id: u32) -> Option<FrameUpdate> {
        let zone = self.window_zones.get(&window_id)?;
        let (w, h) = self.output_size?;
        Some(compute_frame(*zone, window_id, w, h))
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
    fn handle_key_does_not_mark_external_app_dirty() {
        let mut s = state_with_output(1920, 1080);
        s.set_focused("firefox".to_string());
        let frame = s.handle_key(KeyCode::KP_6.raw(), Some(7));
        assert!(frame.is_some());
        assert!(s.take_zones_update().is_none(), "external app must not dirty zones");
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
