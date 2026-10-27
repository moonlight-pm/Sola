//! The Browser field: the launcher's second stage when Browser is picked
//! (Joshua, 2026-10-26: "If i use the app launcher to launch browser, it
//! currently opens an about:blank -- thats not useful, instead present a
//! omni search bar, then open the browser with that").
//!
//! Type an address or a search; Return opens it in a new page window as
//! the browser's location bar would (`sola_core::omnibox`: the address,
//! else Kagi's first result). The rows: what Return does, Kagi's results
//! page for a search, then the visited pages that match — the browser's
//! own history, ranked as its location bar ranks it. With nothing typed,
//! the latest visits.

use sola_core::omnibox::{self, history::Visit};

/// The browser's app id.
pub const APP_ID: &str = "sola-browser";
/// Visited pages shown for a query.
const HITS: usize = 6;
/// Latest visits shown before anything is typed.
const RECENT: usize = 6;

/// A row, in the order shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// Return: the address, or the first result for the search.
    Go { url: String, address: bool },
    /// Kagi's results page for the search.
    Results(String),
    /// `hits[i]`.
    Visit(usize),
}

#[derive(Debug, Default)]
pub struct WebChooser {
    /// The browser's history, newest first.
    visits: Vec<Visit>,
    /// The visits shown under the query.
    pub hits: Vec<Visit>,
    query: String,
}

impl WebChooser {
    pub fn new(visits: Vec<Visit>) -> Self {
        let mut c = Self {
            visits,
            ..Self::default()
        };
        c.apply_query("");
        c
    }

    pub fn apply_query(&mut self, query: &str) {
        self.query = query.trim().to_string();
        self.hits = if self.query.is_empty() {
            self.visits.iter().take(RECENT).cloned().collect()
        } else {
            omnibox::history::search(&self.visits, &self.query, HITS)
        };
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// What Return does first, Kagi's results for a search, then the
    /// visits.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if !self.query.is_empty() {
            let address = omnibox::looks_like_url(&self.query);
            rows.push(Row::Go {
                url: omnibox::enter_url(&self.query),
                address,
            });
            if !address {
                rows.push(Row::Results(omnibox::kagi_search_url(&self.query)));
            }
        }
        rows.extend((0..self.hits.len()).map(Row::Visit));
        rows
    }

    /// The page row `row` opens.
    pub fn url(&self, row: &Row) -> Option<String> {
        match row {
            Row::Go { url, .. } | Row::Results(url) => Some(url.clone()),
            Row::Visit(i) => self.hits.get(*i).map(|v| v.url.clone()),
        }
    }
}

/// A visit's line: its title, else its host.
pub fn visit_label(v: &Visit) -> String {
    let title = v.title.trim();
    if title.is_empty() {
        omnibox::history::host_of(&v.url)
    } else {
        title.to_string()
    }
}

/// A visit's address, short: host and path, no scheme or query.
pub fn visit_note(v: &Visit) -> String {
    let rest = v.url.split_once("://").map(|(_, r)| r).unwrap_or(&v.url);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let rest = rest.trim_end_matches('/');
    rest.strip_prefix("www.").unwrap_or(rest).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(url: &str, title: &str) -> Visit {
        Visit {
            url: url.into(),
            title: title.into(),
            last_unix: 0,
        }
    }

    fn chooser() -> WebChooser {
        WebChooser::new(vec![
            visit("https://github.com/moonlight-pm/Sola", "Sola"),
            visit("https://news.ycombinator.com/", "Hacker News"),
            visit("https://docs.rs/iced/latest/iced/", "iced - Rust"),
        ])
    }

    #[test]
    fn nothing_typed_lists_the_latest_visits() {
        let c = chooser();
        assert_eq!(c.rows(), [Row::Visit(0), Row::Visit(1), Row::Visit(2)]);
    }

    #[test]
    fn an_address_opens_and_matching_visits_follow() {
        let mut c = chooser();
        c.apply_query("github.com");
        let rows = c.rows();
        assert_eq!(
            rows[0],
            Row::Go {
                url: "https://github.com".into(),
                address: true
            }
        );
        assert_eq!(rows[1], Row::Visit(0));
        assert_eq!(c.url(&rows[1]).as_deref(), Some("https://github.com/moonlight-pm/Sola"));
    }

    #[test]
    fn a_search_offers_the_first_result_then_the_results_page() {
        let mut c = chooser();
        c.apply_query("rust iced");
        let rows = c.rows();
        assert!(matches!(&rows[0], Row::Go { address: false, url } if url.contains("%5Crust")));
        assert!(matches!(&rows[1], Row::Results(url) if url.ends_with("q=rust%20iced")));
    }

    #[test]
    fn notes_are_short_addresses() {
        assert_eq!(
            visit_note(&visit("https://www.example.com/a/b/?q=1", "")),
            "example.com/a/b"
        );
        assert_eq!(visit_label(&visit("https://example.com/x", "")), "example.com");
    }
}
