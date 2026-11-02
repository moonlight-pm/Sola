//! What comes back after a restart: the files that were open, where you were
//! in them, and any text not yet saved (the recovery files).

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub open: Vec<Entry>,
    /// Which of them was in front.
    pub front: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    /// `None`: never saved. Only comes back with text to recover.
    pub path: Option<PathBuf>,
    pub line: usize,
    pub col: usize,
    pub top: usize,
    /// Its recovery file, when it has unsaved text.
    pub recover: Option<String>,
}

fn root() -> PathBuf {
    sola_core::config::sola_config_dir().join("editor")
}

fn session_path() -> PathBuf {
    root().join("session.json")
}

fn recover_dir() -> PathBuf {
    root().join("recover")
}

pub fn load() -> Session {
    sola_core::config::load_json(&session_path()).unwrap_or_default()
}

pub fn save(session: &Session) {
    if let Err(e) = fs::create_dir_all(root()) {
        tracing::warn!(error = %e, "can't make the session's folder");
        return;
    }
    if let Err(e) = sola_core::config::save_json_pretty(&session_path(), session) {
        tracing::warn!(error = %e, "can't save the session");
    }
}

/// The name of a window's recovery file, from its tab key.
pub fn recover_name(key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn write_recover(name: &str, text: &str) {
    let dir = recover_dir();
    let written = fs::create_dir_all(&dir).and_then(|()| fs::write(dir.join(name), text));
    if let Err(e) = written {
        tracing::warn!(%name, error = %e, "can't write a recovery file");
    }
}

pub fn read_recover(name: &str) -> Option<String> {
    fs::read_to_string(recover_dir().join(name)).ok()
}

pub fn remove_recover(name: &str) {
    let _ = fs::remove_file(recover_dir().join(name));
}
