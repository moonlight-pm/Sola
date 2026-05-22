pub mod assets;
pub mod state;

pub use assets::SWITCHER_ASSETS;
pub use state::SwitcherState;

use crate::app::{ShellApp, SwitcherApp};

impl ShellApp {
    /// App entries for the switcher render envelope, with icon resolved
    /// against the `applications` registry. Returns a JSON array of
    /// `{ app_id, name, icon, window_count }` objects.
    pub fn switcher_apps_value(&self) -> serde_json::Value {
        let entries: Vec<serde_json::Value> = self
            .switcher
            .apps
            .iter()
            .map(|app| {
                let icon = self
                    .icon_for(&app.app_id)
                    .map(String::from)
                    .unwrap_or_else(|| "app".to_string());
                let window_count = self
                    .known_windows
                    .iter()
                    .filter(|w| w.app_id == app.app_id)
                    .count() as u32;
                serde_json::json!({
                    "app_id": app.app_id,
                    "name": self.display_label(&app.app_id),
                    "icon": icon,
                    "window_count": window_count,
                })
            })
            .collect();
        serde_json::Value::Array(entries)
    }

    /// Build a deduplicated list of app_ids for the switcher, ordered by MRU.
    pub fn rebuild_switcher_apps(&self) -> Vec<SwitcherApp> {
        use std::collections::HashSet;

        let unique_app_ids: Vec<String> = self
            .known_windows
            .iter()
            .filter(|w| {
                use sola_kit::SolaApp;
                w.app_id != Self::APP_ID
            })
            .map(|w| w.app_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let mut apps: Vec<SwitcherApp> = self
            .mru_apps
            .iter()
            .filter(|id| unique_app_ids.contains(id))
            .map(|id| SwitcherApp { app_id: id.clone() })
            .collect();
        // Append any known apps not yet in MRU.
        for id in &unique_app_ids {
            if !self.mru_apps.contains(id) {
                apps.push(SwitcherApp { app_id: id.clone() });
            }
        }
        apps
    }
}
