//! Browser-side operations on the bus-defined persistent topics.
//!
//! Tabs, browser config, and visit history all live on the bus as
//! persistent topics (see `sola_bus::topics::{BrowserTab, BrowserConfig,
//! BrowserHistory}`). This module attaches local-only operations
//! (record-visit, search) to those topics via an extension trait so
//! the browser can mutate the aggregate and re-emit.

use sola_bus::topics::{BrowserHistory, HistoryEntry};

pub const MAX_HISTORY_ENTRIES: usize = 1000;

pub trait HistoryOps {
    /// Increment the visit counter for `url` (or insert a new entry).
    /// The visited entry is moved to the front of `entries`. The list
    /// is capped at `MAX_HISTORY_ENTRIES`.
    fn record_visit(&mut self, url: &str, title: &str);

    /// Substring match against url+title (case-insensitive). Results
    /// are ordered by visit count, descending.
    fn search(&self, query: &str, limit: usize) -> Vec<&HistoryEntry>;
}

impl HistoryOps for BrowserHistory {
    fn record_visit(&mut self, url: &str, title: &str) {
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
        if let Some(pos) = self.entries.iter().position(|e| e.url == url) {
            let entry = self.entries.remove(pos);
            self.entries.insert(0, entry);
        }
        self.entries.truncate(MAX_HISTORY_ENTRIES);
    }

    fn search(&self, query: &str, limit: usize) -> Vec<&HistoryEntry> {
        let q = query.to_lowercase();
        let mut hits: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .filter(|e| e.url.to_lowercase().contains(&q) || e.title.to_lowercase().contains(&q))
            .collect();
        hits.sort_by(|a, b| b.visits.cmp(&a.visits));
        hits.truncate(limit);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_record_and_search() {
        let mut history = BrowserHistory::default();
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
        let mut history = BrowserHistory::default();
        for i in 0..1100 {
            history.record_visit(&format!("https://example.com/{i}"), "Test");
        }
        assert_eq!(history.entries.len(), MAX_HISTORY_ENTRIES);
    }
}
