//! On-disk JSON helpers and the skipped-track list.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::paths::AppDirs;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Skipped {
    #[serde(default)]
    pub uris: HashSet<String>,
}

impl Skipped {
    pub fn load(dirs: &AppDirs) -> Self {
        read_json(&dirs.skipped_file()).unwrap_or_default()
    }

    pub fn save(&self, dirs: &AppDirs) {
        write_json(&dirs.skipped_file(), self);
    }

    pub fn contains(&self, uri: &str) -> bool {
        self.uris.contains(uri)
    }

    pub fn toggle(&mut self, uri: String) -> bool {
        if !self.uris.remove(&uri) {
            self.uris.insert(uri);
            true
        } else {
            false
        }
    }
}

/// Liked Songs URIs, so likes survive a 429 on the shared Web API app.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Liked {
    #[serde(default)]
    pub uris: HashSet<String>,
}

impl Liked {
    pub fn load(dirs: &AppDirs) -> Self {
        read_json(&dirs.liked_file()).unwrap_or_default()
    }

    pub fn save(&self, dirs: &AppDirs) {
        write_json(&dirs.liked_file(), self);
    }

    pub fn set(&mut self, uri: String, saved: bool) {
        if saved {
            self.uris.insert(uri);
        } else {
            self.uris.remove(&uri);
        }
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(text) = serde_json::to_string(value) else {
        return;
    };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}
