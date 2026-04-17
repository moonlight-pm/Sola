use serde::{Deserialize, Serialize};
use sola_app::config::JsonConfig;

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

impl JsonConfig for TabStore {
    const FILE_NAME: &'static str = "browser-tabs.json";
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

impl JsonConfig for BrowsingHistory {
    const FILE_NAME: &'static str = "browser-history.json";
}

const MAX_HISTORY_ENTRIES: usize = 1000;

impl BrowsingHistory {
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
}
