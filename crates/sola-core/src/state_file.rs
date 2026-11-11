//! A file of state that only its own app reads: a shell's saved tab places,
//! a terminal's sessions. It is the app's to keep, not the bus's: the bus
//! carries what other processes read (`docs/specs/2026-11-10-bus-state-design.md`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::warn;

/// One app's state file, as JSON, written whole and atomically.
pub struct StateFile {
    path: PathBuf,
}

impl StateFile {
    /// `$XDG_STATE_HOME/sola/<app>/<name>` (else `~/.local/state/...`):
    /// kept across restarts and reboots.
    pub fn new(app: &str, name: &str) -> Self {
        Self {
            path: crate::paths::state_dir().join(app).join(name),
        }
    }

    /// `$XDG_RUNTIME_DIR/sola/<app>/<name>`: kept while the user is logged
    /// in, across the app's restarts, gone at reboot.
    pub fn runtime(app: &str, name: &str) -> Self {
        Self {
            path: crate::env::runtime_dir().join("sola").join(app).join(name),
        }
    }

    /// At `path`, for tests.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What was saved, or `None` when nothing was (or it cannot be read,
    /// which is logged).
    pub fn load<T: DeserializeOwned>(&self) -> Option<T> {
        match crate::config::load_json(&self.path) {
            Ok(value) => Some(value),
            Err(crate::config::ConfigError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                None
            }
            Err(e) => {
                warn!(path = %self.path.display(), %e, "state file unreadable; starting without it");
                None
            }
        }
    }

    /// Save `value` in place of what was there (a failure is logged).
    pub fn save<T: Serialize>(&self, value: &T) {
        if let Err(e) = crate::config::save_json_pretty(&self.path, value) {
            warn!(path = %self.path.display(), %e, "state file not saved");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads_and_says_none_before() {
        let dir = tempfile::tempdir().unwrap();
        let file = StateFile::at(dir.path().join("app").join("thing.json"));
        assert_eq!(file.load::<Vec<String>>(), None);
        file.save(&vec!["a".to_string()]);
        assert_eq!(file.load::<Vec<String>>(), Some(vec!["a".to_string()]));
    }

    #[test]
    fn an_unreadable_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thing.json");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(StateFile::at(path).load::<Vec<String>>(), None);
    }
}
