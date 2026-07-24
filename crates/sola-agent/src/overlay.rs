//! Sola-only session metadata (pins, last-opened, title overrides).
//! Never stores transcripts.

use std::collections::{HashMap, HashSet};
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
    /// Recent project directories for the new-session picker (newest first).
    #[serde(default)]
    pub recent_cwds: Vec<String>,
    /// Display title overrides keyed by session id (Sola-only; Grok TUI
    /// still shows its own title until a native rename exists).
    #[serde(default)]
    pub title_overrides: HashMap<String, String>,
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
    note_cwd_inner(&mut o, cwd);
    o.last_opened.retain(|x| x != id);
    o.last_opened.insert(0, id.to_string());
    o.last_opened.truncate(50);
    save(&o);
}

pub fn note_cwd(cwd: &str) {
    let mut o = load();
    o.last_cwd = Some(cwd.to_string());
    note_cwd_inner(&mut o, cwd);
    save(&o);
}

fn note_cwd_inner(o: &mut Overlay, cwd: &str) {
    o.recent_cwds.retain(|c| c != cwd);
    o.recent_cwds.insert(0, cwd.to_string());
    o.recent_cwds.truncate(20);
}

pub fn set_title_override(id: &str, title: &str) {
    let mut o = load();
    let t = title.trim();
    if t.is_empty() {
        o.title_overrides.remove(id);
    } else {
        o.title_overrides.insert(id.to_string(), t.to_string());
    }
    save(&o);
}

pub fn title_override(id: &str) -> Option<String> {
    load().title_overrides.get(id).cloned()
}
