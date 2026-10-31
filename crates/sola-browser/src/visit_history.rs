//! Typed / visited URLs for the omnibox (not tab back-forward).
//!
//! Stored browser-wide at `~/.local/share/sola/browser/shared/history.json`.
//! The location bar shows the top matches as a list — it does not
//! autocomplete the field. The record, the file and the ranking are
//! `sola_core::omnibox::history`: the launcher's Browser field reads the
//! same file.

pub use sola_core::omnibox::history::Visit;
use sola_core::omnibox::history::{Store, keep_url};

const STORE_VERSION: u32 = 1;
const CAP: usize = 2000;
/// Matches shown under the location bar.
pub const OMNIBOX_HITS: usize = 5;

#[derive(Debug, Clone)]
pub struct VisitHistory {
    items: Vec<Visit>,
}

impl Default for VisitHistory {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl VisitHistory {
    pub fn load() -> Self {
        Self {
            items: load_store().items,
        }
    }

    pub fn items(&self) -> &[Visit] {
        &self.items
    }

    /// Record a committed page. No-op for blank / search-engine result URLs.
    pub fn record(&mut self, url: &str, title: &str) {
        if !self.ingest(url, title) {
            return;
        }
        self.save();
    }

    /// Ingest without writing disk (session seed). Returns whether the list changed.
    pub fn ingest(&mut self, url: &str, title: &str) -> bool {
        let url = url.trim();
        if !keep_url(url) {
            return false;
        }
        let title = title.trim();
        let now = unix_now();
        if let Some(i) = self.items.iter().position(|v| v.url == url) {
            let title_same = title.is_empty() || title == self.items[i].title;
            if i == 0 && title_same {
                return false;
            }
            let mut v = self.items.remove(i);
            if !title.is_empty() && title != v.title {
                v.title = title.to_string();
            }
            v.last_unix = now;
            self.items.insert(0, v);
            return true;
        }
        self.items.insert(
            0,
            Visit {
                url: url.to_string(),
                title: title.to_string(),
                last_unix: now,
            },
        );
        if self.items.len() > CAP {
            self.items.truncate(CAP);
        }
        true
    }

    pub fn save(&self) {
        save_store(&Store {
            version: STORE_VERSION,
            items: self.items.clone(),
        });
    }

    /// Top [`OMNIBOX_HITS`] matches for a typed query. Empty query → none
    /// (the bar is not a full history browser).
    pub fn search(&self, query: &str) -> Vec<Visit> {
        sola_core::omnibox::history::search(&self.items, query, OMNIBOX_HITS)
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn index_path() -> std::path::PathBuf {
    crate::profiles::shared_dir().join("history.json")
}

fn load_store() -> Store {
    let path = index_path();
    match sola_core::config::load_json_or_default::<Store>(&path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to load visit history");
            Store::default()
        }
    }
}

fn save_store(store: &Store) {
    let path = index_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = sola_core::config::save_json_pretty(&path, store) {
        tracing::warn!(path = %path.display(), error = %e, "failed to write visit history");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(url: &str, title: &str) -> Visit {
        Visit {
            url: url.into(),
            title: title.into(),
            last_unix: 0,
        }
    }

    #[test]
    fn search_ranks_host_over_path() {
        let h = VisitHistory {
            items: vec![
                v("https://example.com/weather", "Weather blog"),
                v("https://github.com/foo", "foo"),
                v("https://weather.gov/", "NWS"),
            ],
        };
        let hits = h.search("weather");
        assert_eq!(hits[0].url, "https://weather.gov/");
        assert!(hits.iter().any(|x| x.url.contains("example.com")));
        assert!(!hits.iter().any(|x| x.url.contains("github")));
    }

    #[test]
    fn ingest_moves_existing_to_front() {
        let mut h = VisitHistory::default();
        assert!(h.ingest("https://a.example/", "A"));
        assert!(h.ingest("https://b.example/", "B"));
        assert!(h.ingest("https://a.example/", "A2"));
        assert_eq!(h.items[0].url, "https://a.example/");
        assert_eq!(h.items[0].title, "A2");
        assert_eq!(h.items.len(), 2);
    }

    #[test]
    fn skips_blank_and_kagi_search() {
        let mut h = VisitHistory::default();
        assert!(!h.ingest("about:blank", ""));
        assert!(!h.ingest("https://kagi.com/search?q=rust", "Kagi"));
        assert!(h.ingest("https://doc.rust-lang.org/", "Rust"));
    }

    #[test]
    fn empty_query_has_no_hits() {
        let h = VisitHistory {
            items: vec![v("https://a.example/", "A")],
        };
        assert!(h.search("").is_empty());
        assert!(h.search("   ").is_empty());
    }

    #[test]
    fn search_dedups_query_variants() {
        let h = VisitHistory {
            items: vec![
                v("https://ideogram.ai/login?utm_source=a", "Ideogram"),
                v("https://ideogram.ai/login?utm_source=b", "Ideogram"),
                v("https://ideogram.ai/login", "Ideogram"),
            ],
        };
        let hits = h.search("ideo");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].url.contains("ideogram.ai/login"));
    }

    #[test]
    fn caps_hits_at_five() {
        let items: Vec<Visit> = (0..12)
            .map(|i| v(&format!("https://n{i}.example/page"), "n"))
            .collect();
        let h = VisitHistory { items };
        assert_eq!(h.search("example").len(), OMNIBOX_HITS);
    }
}
