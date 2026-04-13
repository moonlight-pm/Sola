use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sola_bus::topics::{KeyEvent, OutputGeometry, WindowGeometry, Zone};
use tracing::{info, warn};

const GAP: f64 = 5.0;

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

    /// Try to handle a key event as a zone snap.
    /// Returns a `WindowGeometry` to send on the bus, or None if not a zone key.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<WindowGeometry> {
        if !key.pressed || !key.super_held {
            return None;
        }

        let zone = zone_for_keycode(key.code)?;
        let (w, h) = self.output_size?;
        let app_id = self.focused_app_id.clone()?;

        let geo = compute_geometry(zone, &app_id, w, h);

        self.zone_assignments.insert(app_id, zone);
        self.save_session();

        Some(geo)
    }

    /// Restore all saved zone assignments as WindowGeometry messages.
    pub fn restore(&self) -> Vec<WindowGeometry> {
        let Some((w, h)) = self.output_size else {
            return vec![];
        };

        self.zone_assignments
            .iter()
            .map(|(app_id, zone)| compute_geometry(*zone, app_id, w, h))
            .collect()
    }

    fn save_session(&self) {
        let session = Session {
            zones: self.zone_assignments.iter()
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
        72 => Some(Zone::TopMiddle),    // KP_8
        75 => Some(Zone::Left),         // KP_4
        76 => Some(Zone::FullMiddle),   // KP_5
        77 => Some(Zone::Right),        // KP_6
        80 => Some(Zone::BottomMiddle), // KP_2
        82 => Some(Zone::Fullscreen),   // KP_0
        _ => None,
    }
}

fn compute_geometry(zone: Zone, app_id: &str, output_w: i32, output_h: i32) -> WindowGeometry {
    let (xp, yp, wp, hp) = zone.rect();
    let half = GAP / 2.0;

    let x = (xp * output_w as f64 + half).round() as i32;
    let y = (yp * output_h as f64 + half).round() as i32;
    let w = (wp * output_w as f64 - GAP).round() as i32;
    let h = (hp * output_h as f64 - GAP).round() as i32;

    WindowGeometry {
        app_id: app_id.to_string(),
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
