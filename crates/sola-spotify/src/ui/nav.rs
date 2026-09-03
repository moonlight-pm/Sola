//! In-app back/forward — page history, not track skip.

use crate::settings::{SavedNav, SavedNavEntry};
use crate::worker::Page;

/// How far Back can walk. Current page is not counted.
const MAX_BACK: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavEntry {
    pub page: Page,
    pub search: String,
}

#[derive(Clone, Debug)]
pub struct NavHistory {
    entries: Vec<NavEntry>,
    index: usize,
}

impl NavHistory {
    pub fn new(page: Page) -> Self {
        Self {
            entries: vec![NavEntry {
                page,
                search: String::new(),
            }],
            index: 0,
        }
    }

    pub fn from_saved(saved: &SavedNav, fallback: Page) -> Self {
        let mut entries: Vec<NavEntry> = saved
            .entries
            .iter()
            .filter_map(|entry| {
                let page = Page::decode(&entry.page)?;
                let search = if page == Page::Search {
                    entry.search.clone()
                } else {
                    String::new()
                };
                Some(NavEntry { page, search })
            })
            .collect();
        if entries.is_empty() {
            return Self::new(fallback);
        }
        let mut index = saved.index.min(entries.len() - 1);
        if index > MAX_BACK {
            let extra = index - MAX_BACK;
            entries.drain(..extra);
            index -= extra;
        }
        Self { entries, index }
    }

    pub fn to_saved(&self) -> SavedNav {
        SavedNav {
            entries: self
                .entries
                .iter()
                .map(|entry| SavedNavEntry {
                    page: entry.page.encode(),
                    search: entry.search.clone(),
                })
                .collect(),
            index: self.index,
        }
    }

    pub fn can_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    /// Record a user navigation. Search identity includes the query.
    /// Returns false when this is already the current entry.
    pub fn push(&mut self, page: Page, search: String) -> bool {
        let search = if page == Page::Search {
            search
        } else {
            String::new()
        };
        if self.current().page == page && self.current().search == search {
            return false;
        }
        self.entries.truncate(self.index + 1);
        self.entries.push(NavEntry { page, search });
        if self.entries.len() > MAX_BACK + 1 {
            let extra = self.entries.len() - (MAX_BACK + 1);
            self.entries.drain(..extra);
        }
        self.index = self.entries.len() - 1;
        true
    }

    pub fn back(&mut self) -> Option<NavEntry> {
        if !self.can_back() {
            return None;
        }
        self.index -= 1;
        Some(self.current().clone())
    }

    pub fn forward(&mut self) -> Option<NavEntry> {
        if !self.can_forward() {
            return None;
        }
        self.index += 1;
        Some(self.current().clone())
    }

    pub fn current(&self) -> &NavEntry {
        &self.entries[self.index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_nowhere_to_go() {
        let nav = NavHistory::new(Page::Home);
        assert!(!nav.can_back());
        assert!(!nav.can_forward());
    }

    #[test]
    fn back_and_forward_walk_the_stack() {
        let mut nav = NavHistory::new(Page::Home);
        assert!(nav.push(Page::Liked, String::new()));
        assert!(nav.push(Page::Album("a".into()), String::new()));
        assert!(nav.can_back());
        assert!(!nav.can_forward());

        let back = nav.back().unwrap();
        assert_eq!(back.page, Page::Liked);
        assert!(nav.can_forward());

        let fwd = nav.forward().unwrap();
        assert_eq!(fwd.page, Page::Album("a".into()));
    }

    #[test]
    fn new_branch_drops_forward() {
        let mut nav = NavHistory::new(Page::Home);
        nav.push(Page::Liked, String::new());
        nav.push(Page::Queue, String::new());
        nav.back();
        nav.push(Page::Artists, String::new());
        assert!(!nav.can_forward());
        assert_eq!(nav.back().unwrap().page, Page::Liked);
    }

    #[test]
    fn same_page_is_a_no_op() {
        let mut nav = NavHistory::new(Page::Home);
        assert!(!nav.push(Page::Home, String::new()));
        assert!(!nav.can_back());
    }

    #[test]
    fn search_query_is_part_of_identity() {
        let mut nav = NavHistory::new(Page::Home);
        assert!(nav.push(Page::Search, "neon".into()));
        assert!(!nav.push(Page::Search, "neon".into()));
        assert!(nav.push(Page::Search, "graphite".into()));
        assert_eq!(nav.back().unwrap().search, "neon");
    }

    #[test]
    fn non_search_pages_ignore_the_query_box() {
        let mut nav = NavHistory::new(Page::Home);
        assert!(!nav.push(Page::Home, "typed-but-not-submitted".into()));
    }

    #[test]
    fn drops_oldest_past_twenty_back_steps() {
        let mut nav = NavHistory::new(Page::Home);
        for i in 0..20 {
            assert!(nav.push(Page::Album(i.to_string()), String::new()));
        }
        for _ in 0..20 {
            assert!(nav.back().is_some());
        }
        assert_eq!(nav.current().page, Page::Home);
        assert!(nav.back().is_none());

        for i in 0..20 {
            nav.push(Page::Album(i.to_string()), String::new());
        }
        assert!(nav.push(Page::Liked, String::new()));
        for _ in 0..20 {
            assert!(nav.back().is_some());
        }
        assert_eq!(nav.current().page, Page::Album("0".into()));
        assert!(nav.back().is_none());
    }

    #[test]
    fn roundtrip_keeps_back_forward_and_search() {
        let mut nav = NavHistory::new(Page::Home);
        nav.push(Page::Liked, String::new());
        nav.push(Page::Search, "neon".into());
        nav.back();
        let mut restored = NavHistory::from_saved(&nav.to_saved(), Page::Queue);
        assert_eq!(restored.current().page, Page::Liked);
        assert!(restored.can_back());
        assert!(restored.can_forward());
        let fwd = restored.forward().unwrap();
        assert_eq!(fwd.page, Page::Search);
        assert_eq!(fwd.search, "neon");
    }

    #[test]
    fn empty_save_falls_back_to_last_page() {
        let nav = NavHistory::from_saved(&SavedNav::default(), Page::Liked);
        assert_eq!(nav.current().page, Page::Liked);
        assert!(!nav.can_back());
        assert!(!nav.can_forward());
    }

    #[test]
    fn restore_trims_back_stack_to_twenty() {
        let entries = (0..30)
            .map(|i| SavedNavEntry {
                page: format!("album:{i}"),
                search: String::new(),
            })
            .collect();
        let saved = SavedNav { entries, index: 29 };
        let mut nav = NavHistory::from_saved(&saved, Page::Home);
        assert_eq!(nav.current().page, Page::Album("29".into()));
        for _ in 0..20 {
            assert!(nav.back().is_some());
        }
        assert_eq!(nav.current().page, Page::Album("9".into()));
        assert!(nav.back().is_none());
    }
}
