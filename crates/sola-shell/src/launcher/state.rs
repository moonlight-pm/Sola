//! Launcher filter state.
//!
//! `LauncherState` tracks whether the launcher overlay is active, which apps
//! pass the current query, and which entry is selected.
//!
//! Window management (opening, closing, rendering to iced) lands in Task 5.
//! `render_value` from the legacy shell is omitted here: the iced launcher
//! will use typed Rust data rather than JSON envelopes, so the serialization
//! helper has no equivalent in the new stack.
use sola_core::applications::ApplicationsConfig;

#[derive(Default)]
pub struct LauncherState {
    pub active: bool,
    /// Window ID of the focused window when the launcher opened. Restored on
    /// close so the previously-focused app gets keyboard routing back.
    pub prior_focus: Option<u32>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use sola_core::applications::Application;

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
        assert_eq!(
            state.filtered_ids.first().map(String::as_str),
            Some("firefox")
        );
    }

    #[test]
    fn apply_query_stores_query_string() {
        let apps = fixture();
        let mut state = LauncherState::default();
        state.apply_query(&apps, "term");
        assert_eq!(state.query, "term");
    }

    #[test]
    fn uppercase_query_matches_lowercase_label() {
        let ids = filter(&fixture(), "FIRE");
        assert_eq!(ids, vec!["firefox"]);
    }

    #[test]
    fn config_order_preserved() {
        // "i" is contained in Firefox, Terminal, Files — all three, in config order.
        let ids = filter(&fixture(), "i");
        assert_eq!(ids, vec!["firefox", "sola-terminal", "files"]);
    }
}
