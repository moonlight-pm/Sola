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
    /// App IDs that have already had their config zone applied to a
    /// window. Prevents auto-zoning every new window of the same app.
    config_applied: HashSet<String>,
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

    /// Forget that this app has been auto-zoned this session, so the next
    /// window for the same app_id picks up its config zone again. Called
    /// when an app's last window disappears.
    pub fn forget_app(&mut self, app_id: &str) {
        self.config_applied.remove(app_id);
    }

    /// Apply the config zone to a window if its app has a saved zone
    /// and no window for that app has been zoned yet.
    /// Only sola-* apps persist zones — external apps are zoned manually.
    pub fn apply_config_zone(&mut self, app_id: &str, window_id: u32) -> Option<FrameUpdate> {
        if !app_id.starts_with("sola-") || self.config_applied.contains(app_id) {
            return None;
        }
        let zone = self.app_zone_config.get(app_id).copied()?;
        // If geometry hasn't arrived yet we can't compute the frame. Bail
        // without mutating state so a later Apps event retries once
        // OutputGeometry has been cached.
        let (w, h) = self.output_size?;
        self.config_applied.insert(app_id.to_string());
        self.window_zones.insert(window_id, zone);
        Some(compute_frame(zone, window_id, w, h))
    }

    /// Handle a zone snap keycode for the focused window.
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
            self.config_applied
                .insert(self.focused_app_id.clone().unwrap_or_default());
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
