use std::collections::HashMap;

use sola_bus::topics::{FrameUpdate, OutputGeometry, Zone};
use sola_core::KeyCode;
use tracing::{info, warn};

use crate::config::ShellConfig;
use sola_app::config::JsonConfig;

pub const MENUBAR_HEIGHT: i32 = 28;

pub struct ZoningState {
    pub output_size: Option<(i32, i32)>,
    pub focused_app_id: Option<String>,
    pub zone_assignments: HashMap<String, Zone>,
}

impl ZoningState {
    pub fn new() -> Self {
        let config = ShellConfig::load();

        Self {
            output_size: None,
            focused_app_id: None,
            zone_assignments: config.zones,
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

    pub fn set_focused(&mut self, app_id: String) {
        self.focused_app_id = Some(app_id);
    }

    /// Handle a zone snap keycode. Returns a FrameUpdate for the focused app.
    pub fn handle_key(&mut self, code: u32) -> Option<FrameUpdate> {
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

        info!(app_id = %app_id, ?zone, "snapping to zone");
        let frame = compute_frame(zone, &app_id, w, h);

        self.zone_assignments.insert(app_id, zone);
        self.save_session();

        Some(frame)
    }

    /// Compute the menubar's Frame for a given output.
    pub fn menubar_frame(&self) -> Option<FrameUpdate> {
        let (w, _h) = self.output_size?;
        Some(FrameUpdate {
            app_id: "sola-shell".into(),
            title: Some("menubar".into()),
            x: 0,
            y: 0,
            width: w,
            height: MENUBAR_HEIGHT,
        })
    }

    /// Compute the default Frame for an app without a zone assignment.
    /// Gives it the full output area below the menubar.
    pub fn default_app_frame(&self, app_id: &str) -> Option<FrameUpdate> {
        let (w, h) = self.output_size?;
        Some(FrameUpdate {
            app_id: app_id.to_string(),
            title: None,
            x: 0,
            y: MENUBAR_HEIGHT,
            width: w,
            height: h - MENUBAR_HEIGHT,
        })
    }

    /// Compute the Frame for a zoned app, or default if no zone assigned.
    pub fn app_frame(&self, app_id: &str) -> Option<FrameUpdate> {
        if let Some(zone) = self.zone_assignments.get(app_id) {
            let (w, h) = self.output_size?;
            Some(compute_frame(*zone, app_id, w, h))
        } else {
            self.default_app_frame(app_id)
        }
    }

    fn save_session(&self) {
        let config = ShellConfig {
            zones: self
                .zone_assignments
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        };
        config.save();
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
        _ => None,
    }
}

fn compute_frame(zone: Zone, app_id: &str, output_w: i32, output_h: i32) -> FrameUpdate {
    let (xp, yp, wp, hp) = zone.rect();
    let usable_h = output_h - MENUBAR_HEIGHT;

    let x = (xp * output_w as f64).round() as i32;
    let y = MENUBAR_HEIGHT + (yp * usable_h as f64).round() as i32;
    let w = (wp * output_w as f64).round() as i32;
    let h = (hp * usable_h as f64).round() as i32;

    FrameUpdate {
        app_id: app_id.to_string(),
        title: None,
        x,
        y,
        width: w,
        height: h,
    }
}
