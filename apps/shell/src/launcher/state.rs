use sola_bus::topics::FocusTarget;

use crate::applications::{Application, ApplicationsConfig};

#[derive(Default)]
pub struct LauncherState {
    pub active: bool,
    /// Focus target at the moment the launcher opened. Restored on close so
    /// the previously-focused app gets keyboard routing back.
    pub prior_focus: Option<FocusTarget>,
    pub query: String,
    /// `app_id`s that pass the current filter, in config order.
    pub filtered_ids: Vec<String>,
    pub selected: usize,
}

impl LauncherState {
    /// Rebuild `filtered_ids` from the given applications and query, keeping
    /// config order and resetting `selected` to 0.
    pub fn apply_query(&mut self, apps: &ApplicationsConfig, query: &str) {
        self.query = query.to_string();
        self.filtered_ids = filter(apps, query);
        self.selected = 0;
    }

}

/// Case-insensitive substring match on `label`, preserving config order.
pub fn filter(apps: &ApplicationsConfig, query: &str) -> Vec<String> {
    let q = query.trim().to_lowercase();
    apps.apps
        .iter()
        .filter(|a| q.is_empty() || a.label.to_lowercase().contains(&q))
        .map(|a| a.app_id.clone())
        .collect()
}

/// Serialize the filtered app list for JS rendering.
pub fn render_json(apps: &ApplicationsConfig, ids: &[String]) -> String {
    let entries: Vec<&Application> = ids.iter().filter_map(|id| apps.get(id)).collect();
    let json: Vec<_> = entries
        .iter()
        .map(|a| {
            serde_json::json!({
                "app_id": a.app_id,
                "label": a.label,
                "icon": a.icon,
            })
        })
        .collect();
    serde_json::to_string(&json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ApplicationsConfig {
        ApplicationsConfig {
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
                Application {
                    app_id: "files".into(),
                    label: "Files".into(),
                    command: "nautilus".into(),
                    icon: "lucide/folder".into(),
                },
            ],
        }
    }

    #[test]
    fn empty_query_returns_all_in_order() {
        let ids = filter(&fixture(), "");
        assert_eq!(ids, vec!["firefox", "sola-terminal", "files"]);
    }

    #[test]
    fn substring_match_is_case_insensitive() {
        let ids = filter(&fixture(), "fi");
        assert_eq!(ids, vec!["firefox", "files"]);
    }

    #[test]
    fn no_matches_returns_empty() {
        let ids = filter(&fixture(), "zzz");
        assert!(ids.is_empty());
    }

    #[test]
    fn whitespace_is_trimmed() {
        let ids = filter(&fixture(), "  term  ");
        assert_eq!(ids, vec!["sola-terminal"]);
    }

    #[test]
    fn apply_query_resets_selection() {
        let apps = fixture();
        let mut state = LauncherState::default();
        state.selected = 2;
        state.apply_query(&apps, "fi");
        assert_eq!(state.selected, 0);
        assert_eq!(state.filtered_ids.first().map(String::as_str), Some("firefox"));
    }
}
