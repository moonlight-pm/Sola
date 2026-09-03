//! Playback settings persisted under `~/.config/sola/spotify/settings.json`.

use serde::{Deserialize, Serialize};

use crate::paths::AppDirs;

/// One Back/Forward history entry (`page` is `Page::encode`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedNavEntry {
    pub page: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub search: String,
}

/// Page history for in-app Back/Forward. Empty means just `last_page`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedNav {
    #[serde(default)]
    pub entries: Vec<SavedNavEntry>,
    #[serde(default)]
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub device_name: String,
    pub bitrate_kbps: u16,
    #[serde(default)]
    pub normalisation: bool,
    #[serde(default = "default_true")]
    pub autoplay: bool,
    #[serde(default = "default_true")]
    pub gapless: bool,
    /// Encoded `Page` (`home`, `playlist:<id>`, …). Empty means Home.
    #[serde(default)]
    pub last_page: String,
    /// Last track URI on the restored page (selected row after restart).
    #[serde(default)]
    pub last_track: String,
    /// Playlist last added to (add-to picker pins it first).
    #[serde(default)]
    pub last_playlist: String,
    /// Back/Forward stack (max 20 back steps). Restored on launch.
    #[serde(default)]
    pub nav: SavedNav,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: "Sola".into(),
            bitrate_kbps: 320,
            normalisation: false,
            autoplay: true,
            gapless: true,
            last_page: String::new(),
            last_track: String::new(),
            last_playlist: String::new(),
            nav: SavedNav::default(),
        }
    }
}

impl Settings {
    pub fn load(dirs: &AppDirs) -> Self {
        let path = dirs.settings_file();
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, dirs: &AppDirs) {
        if let Some(parent) = dirs.settings_file().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dirs.settings_file(), text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_without_nav_deserialize() {
        let settings: Settings =
            serde_json::from_str(r#"{"device_name":"Sola","bitrate_kbps":320}"#).unwrap();
        assert!(settings.nav.entries.is_empty());
        assert_eq!(settings.nav.index, 0);
        assert!(settings.last_page.is_empty());
    }
}
