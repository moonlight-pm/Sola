//! Typed / visited URLs for the omnibox (not tab back-forward).
//!
//! Stored browser-wide at `~/.local/share/sola/browser/shared/history.json`.
//! The location bar shows the top matches as a list — it does not
//! autocomplete the field.

use serde::{Deserialize, Serialize};

const STORE_VERSION: u32 = 1;
const CAP: usize = 2000;
/// Matches shown under the location bar.
pub const OMNIBOX_HITS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visit {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub last_unix: u64,
}

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
        let store = Store::load();
        Self { items: store.items }
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
        Store {
            version: STORE_VERSION,
            items: self.items.clone(),
        }
        .save();
    }

    /// Top [`OMNIBOX_HITS`] matches for a typed query. Empty query → none
    /// (the bar is not a full history browser).
    pub fn search(&self, query: &str) -> Vec<Visit> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() || q == "https://" || q == "http://" {
            return Vec::new();
        }
        let mut ranked: Vec<(u32, usize, &Visit)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, v)| score(&q, v).map(|s| (s, i, v)))
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (_, _, v) in ranked {
            if !seen.insert(path_key(&v.url)) {
                continue;
            }
            out.push(v.clone());
            if out.len() == OMNIBOX_HITS {
                break;
            }
        }
        out
    }
}

fn keep_url(url: &str) -> bool {
    if url.is_empty() || url == "about:blank" {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("devtools:")
        || lower.starts_with("sola:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
    {
        return false;
    }
    // Don't let Kagi SERP / lucky URLs crowd out real sites.
    if crate::util::is_kagi_search_url(&lower) {
        return false;
    }
    true
}

fn score(q: &str, v: &Visit) -> Option<u32> {
    let url = v.url.to_ascii_lowercase();
    let title = v.title.to_ascii_lowercase();
    let host = host_of(&url);
    let mut s = 0u32;
    if host.starts_with(q) || url.contains(&format!("://{q}")) || url.contains(&format!(".{q}")) {
        s += 300;
    } else if host.contains(q) {
        s += 200;
    } else if title.contains(q) {
        s += 120;
    } else if url.contains(q) {
        s += 80;
    } else {
        return None;
    }
    Some(s)
}

fn path_key(url: &str) -> String {
    let mut s = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    while s.ends_with('/') && s.matches('/').count() > 2 {
        s.pop();
    }
    s
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split('/').next().unwrap_or("").to_ascii_lowercase()
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Store {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: Vec<Visit>,
}

impl Store {
    fn load() -> Self {
        let path = index_path();
        match sola_core::config::load_json_or_default::<Self>(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to load visit history");
                Self::default()
            }
        }
    }

    fn save(&self) {
        let path = index_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = sola_core::config::save_json_pretty(&path, self) {
            tracing::warn!(path = %path.display(), error = %e, "failed to write visit history");
        }
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
