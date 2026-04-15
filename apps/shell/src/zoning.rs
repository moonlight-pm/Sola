use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sola_bus::topics::{FrameUpdate, OutputGeometry, Zone};
use tracing::{info, warn};

pub const MENUBAR_HEIGHT: i32 = 28;

pub struct ZoningState {
    pub output_size: Option<(i32, i32)>,
    pub focused_app_id: Option<String>,
    pub zone_assignments: HashMap<String, Zone>,
    session_path: PathBuf,
}

impl ZoningState {
    pub fn new() -> Self {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
                PathBuf::from(home).join(".config")
            })
            .join("sola");

        let session_path = config_dir.join("session.json");
        let zone_assignments = load_session(&session_path);

        Self {
            output_size: None,
            focused_app_id: None,
            zone_assignments,
            session_path,
        }
    }

    pub fn set_output_size(&mut self, geo: &OutputGeometry) {
        info!(width = geo.width, height = geo.height, "cached output geometry");
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

    /// Compute FrameUpdates for all zone-assigned apps (used on output resize).
    pub fn restore(&self) -> Vec<FrameUpdate> {
        let Some((w, h)) = self.output_size else {
            return vec![];
        };

        self.zone_assignments
            .iter()
            .map(|(app_id, zone)| compute_frame(*zone, app_id, w, h))
            .collect()
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
        let session = Session {
            zones: self
                .zone_assignments
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        };

        if let Some(parent) = self.session_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match serde_json::to_string_pretty(&session) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.session_path, json) {
                    warn!(path = %self.session_path.display(), "failed to write session: {e}");
                }
            }
            Err(e) => warn!("failed to serialize session: {e}"),
        }
    }
}

fn zone_for_keycode(code: u32) -> Option<Zone> {
    match code {
        80 => Some(Zone::TopMiddle),
        83 => Some(Zone::Left),
        84 => Some(Zone::FullMiddle),
        85 => Some(Zone::Right),
        88 => Some(Zone::BottomMiddle),
        90 => Some(Zone::Fullscreen),
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

#[derive(Serialize, Deserialize)]
struct Session {
    zones: HashMap<String, Zone>,
}

fn load_session(path: &PathBuf) -> HashMap<String, Zone> {
    match std::fs::read_to_string(path) {
        Ok(json) => match serde_json::from_str::<Session>(&json) {
            Ok(session) => {
                info!(count = session.zones.len(), "restored zone session");
                session.zones
            }
            Err(e) => {
                warn!(path = %path.display(), "failed to parse session: {e}");
                HashMap::new()
            }
        },
        Err(_) => HashMap::new(),
    }
}
