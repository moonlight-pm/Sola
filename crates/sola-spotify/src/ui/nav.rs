//! In-app back/forward — page history, not track skip.

use crate::worker::Page;

const MAX_ENTRIES: usize = 64;

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
        if self.entries.len() > MAX_ENTRIES {
            let extra = self.entries.len() - MAX_ENTRIES;
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

    fn current(&self) -> &NavEntry {
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
}
