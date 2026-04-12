use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTab {
    pub url: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_state: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TabStore {
    pub tabs: Vec<PersistedTab>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visits: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BrowsingHistory {
    pub entries: Vec<HistoryEntry>,
}

const MAX_HISTORY_ENTRIES: usize = 1000;

impl TabStore {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        let dir = path.parent().expect("tab store path must have parent");
        std::fs::create_dir_all(dir).ok();
        let tmp = path.with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, &json).is_ok() {
                std::fs::rename(&tmp, path).ok();
            }
        }
    }
}

impl BrowsingHistory {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        let dir = path.parent().expect("history path must have parent");
        std::fs::create_dir_all(dir).ok();
        let tmp = path.with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, &json).is_ok() {
                std::fs::rename(&tmp, path).ok();
            }
        }
    }

    pub fn record_visit(&mut self, url: &str, title: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.url == url) {
            entry.title = title.to_string();
            entry.visits += 1;
        } else {
            self.entries.push(HistoryEntry {
                url: url.to_string(),
                title: title.to_string(),
                visits: 1,
            });
        }
        // Move visited entry to front
        if let Some(pos) = self.entries.iter().position(|e| e.url == url) {
            let entry = self.entries.remove(pos);
            self.entries.insert(0, entry);
        }
        self.entries.truncate(MAX_HISTORY_ENTRIES);
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<&HistoryEntry> {
        let query_lower = query.to_lowercase();
        let mut matches: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                e.url.to_lowercase().contains(&query_lower)
                    || e.title.to_lowercase().contains(&query_lower)
            })
            .collect();
        matches.sort_by(|a, b| b.visits.cmp(&a.visits));
        matches.truncate(limit);
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("sola-browser-test");
        fs::create_dir_all(&dir).ok();
        dir.join(name)
    }

    #[test]
    fn tab_store_round_trip() {
        let path = tmp_path("tabs-rt.json");
        let store = TabStore {
            tabs: vec![PersistedTab {
                url: "https://example.com".into(),
                title: "Example".into(),
                session_state: Some("abc123".into()),
            }],
            active_tab_id: Some("tab-1".into()),
        };
        store.save(&path);
        let loaded = TabStore::load(&path);
        assert_eq!(loaded.tabs.len(), 1);
        assert_eq!(loaded.tabs[0].url, "https://example.com");
        assert_eq!(loaded.tabs[0].session_state.as_deref(), Some("abc123"));
        assert_eq!(loaded.active_tab_id.as_deref(), Some("tab-1"));
        fs::remove_file(&path).ok();
    }

    #[test]
    fn tab_store_load_missing_file() {
        let path = tmp_path("nonexistent.json");
        let store = TabStore::load(&path);
        assert!(store.tabs.is_empty());
        assert!(store.active_tab_id.is_none());
    }

    #[test]
    fn history_record_and_search() {
        let mut history = BrowsingHistory::default();
        history.record_visit("https://github.com", "GitHub");
        history.record_visit("https://github.com", "GitHub");
        history.record_visit("https://example.com", "Example");
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.entries[0].url, "https://example.com");
        assert_eq!(history.entries[1].visits, 2);

        let results = history.search("git", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://github.com");
    }

    #[test]
    fn history_caps_at_max() {
        let mut history = BrowsingHistory::default();
        for i in 0..1100 {
            history.record_visit(&format!("https://example.com/{i}"), "Test");
        }
        assert_eq!(history.entries.len(), MAX_HISTORY_ENTRIES);
    }

    #[test]
    fn history_round_trip() {
        let path = tmp_path("history-rt.json");
        let mut history = BrowsingHistory::default();
        history.record_visit("https://github.com", "GitHub");
        history.save(&path);
        let loaded = BrowsingHistory::load(&path);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].url, "https://github.com");
        fs::remove_file(&path).ok();
    }
}
