//! Applications known to the shell.
//!
//! `ApplicationsConfig` is the in-memory form of `~/.config/sola/shell/applications.json`.
//! It is consumed by the shell (for launcher search, switcher icon lookup, session
//! reconciliation) and written by the settings app. Types live in `sola-core` so
//! neither side owns the schema.
//!
//! Persists via the [`crate::config::JsonConfigIn`] impl at the bottom of
//! this file — `ApplicationsConfig::load()` / `.save()` work directly.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::JsonConfigIn;

/// A launchable application known to the shell.
///
/// Used by the launcher for search+spawn, by the switcher for icon lookup,
/// and intended as the single source of truth for "applications this
/// desktop knows about."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Application {
    /// Stable identifier. Matches the `app_id` the program reports on the
    /// bus when it connects; used for icon lookups by the switcher.
    pub app_id: String,
    /// Human-readable name shown in UI; used as the search target.
    pub label: String,
    /// Command to spawn. Whitespace-split into argv; no shell interpretation.
    pub command: String,
    /// Icon reference in `"<pack>/<name>"` form (e.g. `"lucide/terminal"`).
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplicationsConfig {
    #[serde(default)]
    pub apps: Vec<Application>,
}

impl ApplicationsConfig {
    pub fn get(&self, app_id: &str) -> Option<&Application> {
        self.apps.iter().find(|a| a.app_id == app_id)
    }

    /// Append a new entry. Errors if `app_id` already exists.
    pub fn add(&mut self, app: Application) -> Result<(), DuplicateAppId> {
        if self.get(&app.app_id).is_some() {
            return Err(DuplicateAppId(app.app_id));
        }
        self.apps.push(app);
        Ok(())
    }

    /// Replace the entry currently under `old_app_id` with `new`.
    ///
    /// If `new.app_id != old_app_id`, the entry is renamed — fails with
    /// `Duplicate` if another entry already uses `new.app_id`.
    pub fn update(&mut self, old_app_id: &str, new: Application) -> Result<(), UpdateError> {
        let idx = self
            .apps
            .iter()
            .position(|a| a.app_id == old_app_id)
            .ok_or_else(|| UpdateError::NotFound(old_app_id.to_string()))?;
        if new.app_id != old_app_id && self.apps.iter().any(|a| a.app_id == new.app_id) {
            return Err(UpdateError::Duplicate(DuplicateAppId(new.app_id)));
        }
        self.apps[idx] = new;
        Ok(())
    }

    /// Remove the entry with `app_id` if present. No-op if absent.
    pub fn remove(&mut self, app_id: &str) {
        self.apps.retain(|a| a.app_id != app_id);
    }

    /// For each entry whose command's first word is a relative name that
    /// resolves on `PATH`, replace it with the absolute path. Returns
    /// `true` if any entry changed; callers typically save when so.
    ///
    /// Entries whose first word is already absolute, or can't be resolved,
    /// are left untouched.
    pub fn normalize(&mut self) -> bool {
        let mut changed = false;
        for app in &mut self.apps {
            if let Some(new_cmd) = normalize_command(&app.command) {
                app.command = new_cmd;
                changed = true;
            }
        }
        changed
    }
}

/// Returns the rewritten command if the first word is a relative name that
/// resolves on `PATH`; `None` if unchanged or unresolvable.
fn normalize_command(cmd: &str) -> Option<String> {
    let mut parts = cmd.split_whitespace();
    let first = parts.next()?;
    if Path::new(first).is_absolute() {
        return None;
    }
    let abs = resolve_in_path(first)?;
    let rest: Vec<&str> = parts.collect();
    let abs_str = abs.to_string_lossy();
    Some(if rest.is_empty() {
        abs_str.into_owned()
    } else {
        format!("{} {}", abs_str, rest.join(" "))
    })
}

/// True if the command's first word points at an existing executable file,
/// either directly (absolute path) or via `PATH` lookup.
pub fn command_exists(cmd: &str) -> bool {
    let Some(first) = cmd.split_whitespace().next() else {
        return false;
    };
    if Path::new(first).is_absolute() {
        is_executable(Path::new(first))
    } else {
        resolve_in_path(first).is_some()
    }
}

fn resolve_in_path(name: &str) -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateAppId(pub String);

impl std::fmt::Display for DuplicateAppId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "app_id already exists: {}", self.0)
    }
}

impl std::error::Error for DuplicateAppId {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    NotFound(String),
    Duplicate(DuplicateAppId),
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "no entry with app_id: {id}"),
            Self::Duplicate(d) => write!(f, "{d}"),
        }
    }
}

impl std::error::Error for UpdateError {}

impl JsonConfigIn for ApplicationsConfig {
    const APP_DIR: &'static str = "shell";
    const FILE_NAME: &'static str = "applications.json";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Application {
        Application {
            app_id: "firefox".into(),
            label: "Firefox".into(),
            command: "firefox".into(),
            icon: "simpleicons/firefox".into(),
        }
    }

    #[test]
    fn round_trips_example_json() {
        let cfg = ApplicationsConfig {
            apps: vec![sample()],
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ApplicationsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.apps.len(), 1);
        assert_eq!(back.apps[0].app_id, "firefox");
    }

    #[test]
    fn missing_apps_field_defaults_to_empty() {
        let cfg: ApplicationsConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.apps.is_empty());
    }

    #[test]
    fn get_finds_by_app_id() {
        let cfg = ApplicationsConfig {
            apps: vec![sample()],
        };
        assert_eq!(cfg.get("firefox").unwrap().label, "Firefox");
        assert!(cfg.get("nope").is_none());
    }

    #[test]
    fn add_appends_new_entry() {
        let mut cfg = ApplicationsConfig::default();
        cfg.add(sample()).unwrap();
        assert_eq!(cfg.apps.len(), 1);
    }

    #[test]
    fn add_rejects_duplicate_app_id() {
        let mut cfg = ApplicationsConfig {
            apps: vec![sample()],
        };
        let err = cfg.add(sample()).unwrap_err();
        assert_eq!(err, DuplicateAppId("firefox".into()));
        assert_eq!(cfg.apps.len(), 1);
    }

    #[test]
    fn update_replaces_entry_in_place() {
        let mut cfg = ApplicationsConfig {
            apps: vec![sample()],
        };
        let new = Application {
            app_id: "firefox".into(),
            label: "Firefox ESR".into(),
            command: "firefox-esr".into(),
            icon: "simpleicons/firefox".into(),
        };
        cfg.update("firefox", new).unwrap();
        assert_eq!(cfg.apps[0].label, "Firefox ESR");
        assert_eq!(cfg.apps[0].command, "firefox-esr");
    }

    #[test]
    fn update_can_rename_app_id() {
        let mut cfg = ApplicationsConfig {
            apps: vec![sample()],
        };
        let new = Application {
            app_id: "firefox-nightly".into(),
            label: "Firefox Nightly".into(),
            command: "firefox-nightly".into(),
            icon: "simpleicons/firefox".into(),
        };
        cfg.update("firefox", new).unwrap();
        assert!(cfg.get("firefox").is_none());
        assert_eq!(cfg.get("firefox-nightly").unwrap().label, "Firefox Nightly");
    }

    #[test]
    fn update_rejects_rename_that_collides() {
        let other = Application {
            app_id: "brave".into(),
            label: "Brave".into(),
            command: "brave".into(),
            icon: "simpleicons/brave".into(),
        };
        let mut cfg = ApplicationsConfig {
            apps: vec![sample(), other],
        };
        let renamed = Application {
            app_id: "brave".into(),
            label: "Firefox".into(),
            command: "firefox".into(),
            icon: "simpleicons/firefox".into(),
        };
        let err = cfg.update("firefox", renamed).unwrap_err();
        assert_eq!(err, UpdateError::Duplicate(DuplicateAppId("brave".into())));
    }

    #[test]
    fn update_missing_returns_not_found() {
        let mut cfg = ApplicationsConfig::default();
        assert!(matches!(
            cfg.update("nope", sample()),
            Err(UpdateError::NotFound(_))
        ));
    }

    #[test]
    fn remove_deletes_entry() {
        let mut cfg = ApplicationsConfig {
            apps: vec![sample()],
        };
        cfg.remove("firefox");
        assert!(cfg.apps.is_empty());
    }

    #[test]
    fn remove_missing_is_noop() {
        let mut cfg = ApplicationsConfig {
            apps: vec![sample()],
        };
        cfg.remove("nope");
        assert_eq!(cfg.apps.len(), 1);
    }

    #[test]
    fn normalize_leaves_absolute_path_unchanged() {
        let mut cfg = ApplicationsConfig {
            apps: vec![Application {
                app_id: "foo".into(),
                label: "Foo".into(),
                command: "/opt/sola/bin/sola-settings".into(),
                icon: "".into(),
            }],
        };
        assert!(!cfg.normalize());
        assert_eq!(cfg.apps[0].command, "/opt/sola/bin/sola-settings");
    }

    #[test]
    fn normalize_leaves_unresolvable_unchanged() {
        let mut cfg = ApplicationsConfig {
            apps: vec![Application {
                app_id: "foo".into(),
                label: "Foo".into(),
                command: "definitely-not-a-real-binary-xyz-123".into(),
                icon: "".into(),
            }],
        };
        assert!(!cfg.normalize());
        assert_eq!(cfg.apps[0].command, "definitely-not-a-real-binary-xyz-123");
    }

    #[test]
    fn normalize_resolves_name_and_preserves_args() {
        // Pick a binary that's ~universally present on Linux hosts.
        let name = "sh";
        let Some(abs) = resolve_in_path(name) else {
            // CI without /bin on PATH — skip rather than fail.
            return;
        };
        let mut cfg = ApplicationsConfig {
            apps: vec![Application {
                app_id: "shell".into(),
                label: "Shell".into(),
                command: format!("{name} -c echo"),
                icon: "".into(),
            }],
        };
        assert!(cfg.normalize());
        assert_eq!(cfg.apps[0].command, format!("{} -c echo", abs.display()));
    }

    #[test]
    fn command_exists_flags_missing_absolute() {
        assert!(!command_exists("/this/path/does/not/exist/xyz"));
    }

    #[test]
    fn command_exists_flags_missing_in_path() {
        assert!(!command_exists("definitely-not-a-real-binary-xyz-123"));
    }
}
