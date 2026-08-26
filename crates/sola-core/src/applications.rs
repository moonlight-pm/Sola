//! Applications known to the shell.
//!
//! `ApplicationsConfig` is the in-memory shape of the user-edited
//! launcher list. It is consumed by `sola-shell` (launcher search,
//! switcher icon lookup, session reconciliation) and produced by
//! `sola-settings`. Types live in `sola-core` so neither side owns
//! the schema. Persistence is via the bus: `Topic::Applications` is a
//! `#[persistent]` topic that the bus host stores in `state.yaml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Installed wrapper binary. Settings synthesizes
/// `"{WRAPPER_BIN} {app_id}"` for `kind = wrapper` entries so the
/// launcher/session path stays “spawn this command”.
pub const WRAPPER_BIN: &str = "/opt/sola/bin/sola-wrapper";

/// How the shell launches this catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    /// `command` is a real argv (today's default).
    #[default]
    Command,
    /// Website wrapped by `sola-wrapper <app_id>`. `url` is required;
    /// `command` is synthesized as [`wrapper_command`].
    Wrapper,
}

/// A launchable application known to the shell.
///
/// Used by the launcher for search+spawn, by the switcher for icon lookup,
/// and intended as the single source of truth for "applications this
/// desktop knows about."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Application {
    /// Stable identifier. Matches the `app_id` the program reports on the
    /// bus when it connects; used for icon lookups by the switcher.
    pub app_id: String,
    /// Human-readable name shown in UI; used as the search target.
    pub label: String,
    /// Command to spawn. Whitespace-split into argv; no shell interpretation.
    /// For [`AppKind::Wrapper`], Settings writes [`wrapper_command`].
    pub command: String,
    /// Icon reference. Either a pack name (`"lucide/terminal"`,
    /// `"simpleicons/firefox"`) resolved under `/opt/sola/share/icons/`,
    /// or a filesystem path to a full-color raster
    /// (`"/home/…/orca-ide.png"`, `"~/.local/share/sola/icons/…"`).
    /// Pack SVGs are theme-tinted; path / pack PNGs render full-color.
    pub icon: String,
    /// Launch kind. Missing in old `state.yaml` records → [`AppKind::Command`].
    /// Live bus postcard from a pre-wrapper host is four strings; sola-bus
    /// `decode_payload` falls back so Settings still lists those apps.
    #[serde(default)]
    pub kind: AppKind,
    /// Start URL when [`Self::kind`] is [`AppKind::Wrapper`].
    #[serde(default)]
    pub url: Option<String>,
}

/// Synthesized launcher argv for a wrapper id.
pub fn wrapper_command(app_id: &str) -> String {
    format!("{WRAPPER_BIN} {app_id}")
}

/// True when `url` is a non-empty `http://` or `https://` start URL.
pub fn is_wrapper_url(url: &str) -> bool {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplicationsConfig {
    #[serde(default)]
    pub apps: Vec<Application>,
}

impl ApplicationsConfig {
    /// Exact `app_id` match (launcher, settings, emit/update keys).
    pub fn get(&self, app_id: &str) -> Option<&Application> {
        self.apps.iter().find(|a| a.app_id == app_id)
    }

    /// Catalog look-up for a **running** window's Wayland / xdg `app_id`.
    ///
    /// Tries exact match first, then ASCII case-insensitive. External apps
    /// often disagree with the launcher catalog on casing
    /// (`StartupWMClass=orca` vs catalog `Orca`, `signal` vs `Signal`).
    /// Switcher icon/label resolution uses this so official faces still
    /// show when the catalog key was entered with different case.
    pub fn get_for_window(&self, app_id: &str) -> Option<&Application> {
        self.get(app_id).or_else(|| {
            self.apps
                .iter()
                .find(|a| a.app_id.eq_ignore_ascii_case(app_id))
        })
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
            if app.normalize() {
                changed = true;
            }
        }
        changed
    }
}

impl Application {
    /// Per-entry counterpart to [`ApplicationsConfig::normalize`]. If
    /// `command`'s first word is a relative name that resolves on
    /// `PATH`, rewrite it to the absolute path. Returns `true` if the
    /// command changed.
    pub fn normalize(&mut self) -> bool {
        if let Some(new_cmd) = normalize_command(&self.command) {
            self.command = new_cmd;
            true
        } else {
            false
        }
    }

    pub fn is_wrapper(&self) -> bool {
        self.kind == AppKind::Wrapper
    }

    /// For wrappers, rewrite `command` to [`wrapper_command`]; then PATH-normalize.
    pub fn finalize(&mut self) {
        if self.is_wrapper() {
            self.command = wrapper_command(&self.app_id);
        }
        let _ = self.normalize();
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

pub fn resolve_in_path(name: &str) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Application {
        Application {
            app_id: "firefox".into(),
            label: "Firefox".into(),
            command: "firefox".into(),
            icon: "simpleicons/firefox".into(),
            ..Default::default()
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
    fn get_for_window_falls_back_to_case_insensitive() {
        let cfg = ApplicationsConfig {
            apps: vec![Application {
                app_id: "Orca".into(),
                label: "Orca".into(),
                command: "orca".into(),
                icon: "/tmp/orca.png".into(),
                ..Default::default()
            }],
        };
        assert!(cfg.get("orca").is_none(), "exact get stays case-sensitive");
        let hit = cfg.get_for_window("orca").expect("ci match");
        assert_eq!(hit.app_id, "Orca");
        assert_eq!(hit.icon, "/tmp/orca.png");
        assert_eq!(
            cfg.get_for_window("ORCA").map(|a| a.label.as_str()),
            Some("Orca")
        );
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        };
        let mut cfg = ApplicationsConfig {
            apps: vec![sample(), other],
        };
        let renamed = Application {
            app_id: "brave".into(),
            label: "Firefox".into(),
            command: "firefox".into(),
            icon: "simpleicons/firefox".into(),
            ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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

    #[test]
    fn postcard_roundtrips_command_and_wrapper() {
        let cmd = sample();
        let bytes = postcard::to_allocvec(&cmd).unwrap();
        let back: Application = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, cmd);

        let wrap = Application {
            app_id: "slack".into(),
            label: "Slack".into(),
            command: wrapper_command("slack"),
            icon: "simpleicons/slack".into(),
            kind: AppKind::Wrapper,
            url: Some("https://app.slack.com".into()),
        };
        let bytes = postcard::to_allocvec(&wrap).unwrap();
        let back: Application = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, wrap);
    }

    #[test]
    fn old_json_records_default_kind_and_url() {
        let json = r#"{"app_id":"firefox","label":"Firefox","command":"firefox","icon":"simpleicons/firefox"}"#;
        let a: Application = serde_json::from_str(json).unwrap();
        assert_eq!(a.kind, AppKind::Command);
        assert_eq!(a.url, None);
        assert!(!a.is_wrapper());
    }

    #[test]
    fn old_yaml_records_still_load() {
        let yaml = "app_id: firefox\nlabel: Firefox\ncommand: firefox\nicon: simpleicons/firefox\n";
        let a: Application = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(a.app_id, "firefox");
        assert_eq!(a.kind, AppKind::Command);
        assert_eq!(a.url, None);
    }

    #[test]
    fn wrapper_fields_round_trip() {
        let a = Application {
            app_id: "slack".into(),
            label: "Slack".into(),
            command: wrapper_command("slack"),
            icon: "simpleicons/slack".into(),
            kind: AppKind::Wrapper,
            url: Some("https://app.slack.com".into()),
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: Application = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, AppKind::Wrapper);
        assert_eq!(back.url.as_deref(), Some("https://app.slack.com"));
        assert_eq!(back.command, "/opt/sola/bin/sola-wrapper slack");
        assert!(json.contains("\"kind\":\"wrapper\""));
    }

    #[test]
    fn command_kind_serializes_with_null_url() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains("\"kind\":\"command\""), "{json}");
        let back: Application = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, AppKind::Command);
        assert_eq!(back.url, None);
    }

    #[test]
    fn finalize_synthesizes_wrapper_command() {
        let mut a = Application {
            app_id: "discord".into(),
            label: "Discord".into(),
            command: "whatever".into(),
            icon: String::new(),
            kind: AppKind::Wrapper,
            url: Some("https://discord.com/app".into()),
        };
        a.finalize();
        assert_eq!(a.command, wrapper_command("discord"));
    }

    #[test]
    fn is_wrapper_url_requires_http_scheme() {
        assert!(is_wrapper_url("https://app.slack.com"));
        assert!(is_wrapper_url("http://localhost:3000/"));
        assert!(!is_wrapper_url(""));
        assert!(!is_wrapper_url("app.slack.com"));
        assert!(!is_wrapper_url("https://"));
        assert!(!is_wrapper_url("ftp://example.com"));
    }
}
