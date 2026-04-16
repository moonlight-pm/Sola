use serde::{Deserialize, Serialize};
use sola_app::config::JsonConfigIn;

/// A launchable application known to the shell.
///
/// Used by the launcher for search+spawn, by the switcher for icon lookup,
/// and intended as the single source of truth for "applications this
/// desktop knows about."
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl JsonConfigIn for ApplicationsConfig {
    const APP_DIR: &'static str = "shell";
    const FILE_NAME: &'static str = "applications.json";
}

impl ApplicationsConfig {
    pub fn get(&self, app_id: &str) -> Option<&Application> {
        self.apps.iter().find(|a| a.app_id == app_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_example_json() {
        let cfg = ApplicationsConfig {
            apps: vec![
                Application {
                    app_id: "firefox".into(),
                    label: "Firefox".into(),
                    command: "firefox".into(),
                    icon: "simpleicons/firefox".into(),
                },
                Application {
                    app_id: "sola-terminal".into(),
                    label: "Terminal".into(),
                    command: "/opt/sola/bin/sola-terminal".into(),
                    icon: "lucide/terminal".into(),
                },
            ],
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ApplicationsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.apps.len(), 2);
        assert_eq!(back.apps[0].app_id, "firefox");
        assert_eq!(back.get("sola-terminal").unwrap().icon, "lucide/terminal");
    }

    #[test]
    fn missing_apps_field_defaults_to_empty() {
        let cfg: ApplicationsConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.apps.is_empty());
    }
}
