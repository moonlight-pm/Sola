//! Sola-only session metadata (pins, last-opened). Never stores transcripts.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Overlay {
    #[serde(default)]
    pub pinned: HashSet<String>,
    #[serde(default)]
    pub last_opened: Vec<String>,
    #[serde(default)]
    pub last_cwd: Option<String>,
}

fn overlay_path() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("agent")
        .join("overlay.json")
}

pub fn load() -> Overlay {
    let path = overlay_path();
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Overlay::default(),
    }
}

pub fn save(overlay: &Overlay) {
    let path = overlay_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(overlay) {
        let _ = fs::write(path, json);
    }
}

pub fn toggle_pin(id: &str) {
    let mut o = load();
    if !o.pinned.remove(id) {
        o.pinned.insert(id.to_string());
    }
    save(&o);
}

pub fn note_opened(id: &str, cwd: &str) {
    let mut o = load();
    o.last_cwd = Some(cwd.to_string());
    o.last_opened.retain(|x| x != id);
    o.last_opened.insert(0, id.to_string());
    o.last_opened.truncate(50);
    save(&o);
}
