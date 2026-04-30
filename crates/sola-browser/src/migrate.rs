//! One-shot migration from the legacy JsonConfig-backed JSON files
//! (`browser-tabs.json`, `browser-history.json`) into the new bus
//! topics (`BrowserTab`, `BrowserConfig`, `BrowserHistory`).
//!
//! Runs once on startup. After successful migration the legacy files
//! are renamed to `.migrated` so subsequent launches skip this path.
//! The whole module can be deleted in a future cleanup PR — its
//! responsibilities are bounded and self-contained.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sola_bus::topics::{BrowserConfig, BrowserHistory, BrowserTab, HistoryEntry};

/// Snapshot of legacy on-disk state, ready to emit via the bus.
#[derive(Debug)]
pub struct MigrationPlan {
    pub tabs: Vec<BrowserTab>,
    pub config: BrowserConfig,
    pub history: BrowserHistory,
}

#[derive(Deserialize)]
struct LegacyTab {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    session_state: Option<String>,
}

#[derive(Deserialize)]
struct LegacyTabStore {
    #[serde(default)]
    tabs: Vec<LegacyTab>,
}

#[derive(Deserialize, Serialize)]
struct LegacyHistory {
    #[serde(default)]
    entries: Vec<HistoryEntry>,
}

/// Compute a migration plan from `dir` (typically `~/.config/sola/`).
/// Returns `None` if there's nothing to migrate (either the new
/// namespace already exists, or no legacy files are present).
pub fn compute_migration(dir: &Path) -> Option<MigrationPlan> {
    // If the new namespace root exists, assume migration already happened.
    if dir.join("browser").exists() {
        return None;
    }
    let tabs_path = dir.join("browser-tabs.json");
    let history_path = dir.join("browser-history.json");
    if !tabs_path.exists() && !history_path.exists() {
        return None;
    }

    let tabs: Vec<BrowserTab> = std::fs::read_to_string(&tabs_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<LegacyTabStore>(&raw).ok())
        .map(|store| {
            store
                .tabs
                .into_iter()
                .enumerate()
                .map(|(i, t)| BrowserTab {
                    id: uuid::Uuid::new_v4().to_string(),
                    url: t.url,
                    title: t.title,
                    ordinal: i as u32,
                    session_state: t.session_state,
                })
                .collect()
        })
        .unwrap_or_default();

    let history = std::fs::read_to_string(&history_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<LegacyHistory>(&raw).ok())
        .map(|h| BrowserHistory { entries: h.entries })
        .unwrap_or_default();

    // Legacy schema didn't persist a usable tab identity (current
    // restore generates throwaway `restored-{i}` ids and unconditionally
    // activates the first tab). `realize_active`'s lowest-ordinal
    // fallback reproduces "select first tab" exactly when active_tab_id
    // is None.
    Some(MigrationPlan {
        tabs,
        config: BrowserConfig {
            active_tab_id: None,
        },
        history,
    })
}

/// Rename the legacy files to `.migrated` so future launches skip the
/// migrator. Missing files are silently ignored. Errors are logged but
/// not propagated — a failure here is non-fatal (the new namespace
/// already exists at this point).
pub fn mark_migrated(dir: &Path) {
    for name in &["browser-tabs.json", "browser-history.json"] {
        let path = dir.join(name);
        if !path.exists() {
            continue;
        }
        let dest = path.with_extension("json.migrated");
        if let Err(e) = std::fs::rename(&path, &dest) {
            tracing::warn!(path = %path.display(), %e, "failed to rename legacy file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn populates_topics_from_legacy_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        fs::write(
            dir.join("browser-tabs.json"),
            r#"{
              "tabs": [
                {"url": "https://example.com/", "title": "Example"},
                {"url": "https://github.com/", "title": "GitHub", "session_state": "abc"}
              ],
              "active_tab_id": "ignored-by-design"
            }"#,
        )
        .unwrap();
        fs::write(
            dir.join("browser-history.json"),
            r#"{
              "entries": [
                {"url": "https://example.com/", "title": "Example", "visits": 3}
              ]
            }"#,
        )
        .unwrap();

        let plan = compute_migration(dir).unwrap();
        assert_eq!(plan.tabs.len(), 2);
        assert_eq!(plan.tabs[0].url, "https://example.com/");
        assert_eq!(plan.tabs[0].ordinal, 0);
        assert_eq!(plan.tabs[1].session_state.as_deref(), Some("abc"));
        // Legacy active id had no usable identity; left None for
        // realize_active's lowest-ordinal fallback.
        assert!(plan.config.active_tab_id.is_none());
        assert_eq!(plan.history.entries.len(), 1);
    }

    #[test]
    fn skips_when_new_namespace_exists() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("browser-tabs.json"), "{}").unwrap();
        fs::create_dir_all(dir.join("browser")).unwrap();
        assert!(compute_migration(dir).is_none());
    }

    #[test]
    fn skips_when_no_legacy_files() {
        let tmp = TempDir::new().unwrap();
        assert!(compute_migration(tmp.path()).is_none());
    }

    #[test]
    fn mark_migrated_renames_existing_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("browser-tabs.json"), "{}").unwrap();
        fs::write(dir.join("browser-history.json"), "{}").unwrap();
        mark_migrated(dir);
        assert!(!dir.join("browser-tabs.json").exists());
        assert!(!dir.join("browser-history.json").exists());
        assert!(dir.join("browser-tabs.json.migrated").exists());
        assert!(dir.join("browser-history.json.migrated").exists());
    }

    #[test]
    fn mark_migrated_ignores_missing_files() {
        let tmp = TempDir::new().unwrap();
        mark_migrated(tmp.path());
    }
}
