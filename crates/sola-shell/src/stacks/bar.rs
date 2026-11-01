//! A workspace's bar as blocks (`docs/specs/2026-10-21-next-tab-bar-design.md`
//! § App groups and keys): a loose row, or an app's group — the apps that
//! open a window per document ([`grouped`]) keep their windows together
//! under the app's header. Groups and loose rows mix in any order. The flat
//! order, every row top to bottom, is what the zones, the keys and the
//! saved layout read. A group whose last window left stays as an empty
//! block: its place, for the app's next window.

use super::Tab;

/// The apps whose windows group in the bar: each opens a window per
/// document. A web app has an app id of its own, so it stays loose though
/// the browser hosts it.
const GROUPED: &[&str] = &[
    "sola-browser",
    "sola-projects",
    "sola-terminal",
    "sola-paint",
    "sola-design",
];

/// Whether `app_id`'s windows group in the bar.
pub fn grouped(app_id: &str) -> bool {
    GROUPED.contains(&app_id)
}

/// The app a remembered key belongs to: a tab key is `<app>/<tab>`, a
/// window without one is remembered by its app id.
pub fn app_of_key(key: &str) -> &str {
    key.split_once('/').map_or(key, |(app, _)| app)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Loose(Tab),
    Group {
        app: String,
        tabs: Vec<Tab>,
        /// Its index in the saved bar, while windows come back to it: the
        /// rows restored around it line up as saved.
        rank: Option<usize>,
    },
}

/// A block, by what it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockId {
    /// A loose row's window; given a grouped window, its group.
    Window(u32),
    Group(String),
}

impl Block {
    /// Its rows: the loose row, or the group's.
    pub fn tabs(&self) -> &[Tab] {
        match self {
            Block::Loose(t) => std::slice::from_ref(t),
            Block::Group { tabs, .. } => tabs,
        }
    }

    fn holds(&self, window_id: u32) -> bool {
        self.tabs().iter().any(|t| t.window_id == window_id)
    }

    fn is(&self, id: &BlockId) -> bool {
        match (self, id) {
            (Block::Group { app, .. }, BlockId::Group(g)) => app == g,
            (_, BlockId::Window(w)) => self.holds(*w),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bar {
    blocks: Vec<Block>,
}

impl Bar {
    /// A bar of empty groups, in order, each at its index in the saved bar
    /// (a saved layout's groups, waiting for their windows).
    pub fn of_groups(groups: Vec<(String, usize)>) -> Self {
        Self {
            blocks: groups
                .into_iter()
                .map(|(app, rank)| Block::Group {
                    app,
                    tabs: Vec::new(),
                    rank: Some(rank),
                })
                .collect(),
        }
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Every row, top to bottom.
    pub fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.blocks.iter().flat_map(Block::tabs)
    }

    pub fn tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        self.blocks.iter_mut().flat_map(|b| match b {
            Block::Loose(t) => std::slice::from_mut(t),
            Block::Group { tabs, .. } => tabs.as_mut_slice(),
        })
    }

    pub fn len(&self) -> usize {
        self.blocks.iter().map(|b| b.tabs().len()).sum()
    }

    pub fn index_of(&self, window_id: u32) -> Option<usize> {
        self.tabs().position(|t| t.window_id == window_id)
    }

    pub fn tab(&self, window_id: u32) -> Option<&Tab> {
        self.tabs().find(|t| t.window_id == window_id)
    }

    /// Where block `b` starts in the flat order.
    fn start_of(&self, b: usize) -> usize {
        self.blocks[..b].iter().map(|b| b.tabs().len()).sum()
    }

    fn group_index(&self, app: &str) -> Option<usize> {
        self.blocks
            .iter()
            .position(|b| matches!(b, Block::Group { app: a, .. } if a == app))
    }

    fn block_index(&self, id: &BlockId) -> Option<usize> {
        self.blocks.iter().position(|b| b.is(id))
    }

    /// `app`'s group: where its rows start and end in the flat order
    /// (equal for an empty group).
    pub fn group_range(&self, app: &str) -> Option<(usize, usize)> {
        let b = self.group_index(app)?;
        let start = self.start_of(b);
        Some((start, start + self.blocks[b].tabs().len()))
    }

    /// Where the block holding `window_id` ends in the flat order.
    pub fn block_end(&self, window_id: u32) -> Option<usize> {
        let b = self.blocks.iter().position(|b| b.holds(window_id))?;
        Some(self.start_of(b + 1))
    }

    /// The block a new block goes before, for flat index `at`: the first
    /// that starts there or later (so before any empty group there), past
    /// a group `at` falls inside.
    fn boundary(&self, at: usize) -> usize {
        let mut start = 0;
        for (b, block) in self.blocks.iter().enumerate() {
            if start >= at {
                return b;
            }
            start += block.tabs().len();
            if start > at {
                return b + 1;
            }
        }
        self.blocks.len()
    }

    /// Put `tab` at flat index `at`: into its app's group when it has one
    /// (kept within the group's rows), else a block of its own (a new
    /// group, for a grouped app) at the block boundary there.
    pub fn insert(&mut self, tab: Tab, at: usize) {
        if grouped(&tab.app_id) {
            if let Some(b) = self.group_index(&tab.app_id) {
                let start = self.start_of(b);
                if let Block::Group { tabs, .. } = &mut self.blocks[b] {
                    let i = at.saturating_sub(start).min(tabs.len());
                    tabs.insert(i, tab);
                }
                return;
            }
            let b = self.boundary(at);
            let app = tab.app_id.clone();
            self.blocks.insert(
                b,
                Block::Group {
                    app,
                    tabs: vec![tab],
                    rank: None,
                },
            );
            return;
        }
        let b = self.boundary(at);
        self.blocks.insert(b, Block::Loose(tab));
    }

    /// Put back `tab`, saved at index `rank` of the bar: before the first
    /// row (or block) saved after it, or not saved (added since). `rank_of`
    /// is each placed window's saved index.
    pub fn insert_ranked(&mut self, tab: Tab, rank: usize, rank_of: &dyn Fn(u32) -> Option<usize>) {
        let later = |w: u32| rank_of(w).is_none_or(|r| r > rank);
        if grouped(&tab.app_id) {
            if let Some(b) = self.group_index(&tab.app_id) {
                if let Block::Group { tabs, .. } = &mut self.blocks[b] {
                    let i = tabs
                        .iter()
                        .position(|t| later(t.window_id))
                        .unwrap_or(tabs.len());
                    tabs.insert(i, tab);
                }
                return;
            }
        }
        let block_later = |block: &Block| match block {
            Block::Loose(t) => later(t.window_id),
            Block::Group { tabs, rank: r, .. } => match r {
                Some(r) => *r > rank,
                None => tabs.iter().all(|t| later(t.window_id)),
            },
        };
        let b = self
            .blocks
            .iter()
            .position(block_later)
            .unwrap_or(self.blocks.len());
        let block = if grouped(&tab.app_id) {
            Block::Group {
                app: tab.app_id.clone(),
                tabs: vec![tab],
                rank: None,
            }
        } else {
            Block::Loose(tab)
        };
        self.blocks.insert(b, block);
    }

    /// Take `window_id` out: its flat index and its tab. Its group stays,
    /// empty when it was the last.
    pub fn remove(&mut self, window_id: u32) -> Option<(usize, Tab)> {
        let at = self.index_of(window_id)?;
        let b = self.blocks.iter().position(|b| b.holds(window_id))?;
        let tab = match &mut self.blocks[b] {
            Block::Loose(_) => match self.blocks.remove(b) {
                Block::Loose(t) => t,
                Block::Group { .. } => unreachable!("matched a loose block"),
            },
            Block::Group { tabs, .. } => {
                let i = tabs.iter().position(|t| t.window_id == window_id)?;
                tabs.remove(i)
            }
        };
        Some((at, tab))
    }

    /// Move `window_id`'s row before `before`'s. A group's row stays in its
    /// group (the end, when `before` is not in it); a loose row's block
    /// goes before the block holding `before` (else to the end). Returns
    /// whether it moved.
    pub fn reorder(&mut self, window_id: u32, before: Option<u32>) -> bool {
        if before == Some(window_id) {
            return false;
        }
        let Some(b) = self.blocks.iter().position(|b| b.holds(window_id)) else {
            return false;
        };
        let Block::Group { tabs, .. } = &mut self.blocks[b] else {
            let before = before.map(BlockId::Window);
            return self.move_block(&BlockId::Window(window_id), before.as_ref());
        };
        let Some(from) = tabs.iter().position(|t| t.window_id == window_id) else {
            return false;
        };
        let tab = tabs.remove(from);
        let to = before
            .and_then(|w| tabs.iter().position(|t| t.window_id == w))
            .unwrap_or(tabs.len());
        tabs.insert(to, tab);
        to != from
    }

    /// Move block `id` before block `before` (else to the end). Returns
    /// whether it moved.
    pub fn move_block(&mut self, id: &BlockId, before: Option<&BlockId>) -> bool {
        let Some(from) = self.block_index(id) else {
            return false;
        };
        if before.and_then(|b| self.block_index(b)) == Some(from) {
            return false;
        }
        let block = self.blocks.remove(from);
        let to = before
            .and_then(|b| self.block_index(b))
            .unwrap_or(self.blocks.len());
        self.blocks.insert(to, block);
        to != from
    }

    /// Forget every row; groups stay, empty, in their places.
    pub fn clear(&mut self) {
        self.blocks.retain(|b| matches!(b, Block::Group { .. }));
        for b in &mut self.blocks {
            if let Block::Group { tabs, .. } = b {
                tabs.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::Zone;

    fn tab(window_id: u32, app: &str) -> Tab {
        Tab {
            window_id,
            app_id: app.to_string(),
            key: app.to_string(),
            zone: Zone::Fullscreen,
        }
    }

    fn ids(bar: &Bar) -> Vec<u32> {
        bar.tabs().map(|t| t.window_id).collect()
    }

    const BROWSER: &str = "sola-browser";
    const MAIL: &str = "sola-mail";

    #[test]
    fn a_grouped_window_joins_its_group_wherever_it_is_put() {
        let mut bar = Bar::default();
        bar.insert(tab(1, BROWSER), 0);
        bar.insert(tab(2, MAIL), 1);
        // At the end of the bar, but its group is first.
        bar.insert(tab(3, BROWSER), 2);
        assert_eq!(ids(&bar), vec![1, 3, 2]);
        assert_eq!(bar.group_range(BROWSER), Some((0, 2)));
    }

    #[test]
    fn a_loose_row_never_splits_a_group() {
        let mut bar = Bar::default();
        bar.insert(tab(1, BROWSER), 0);
        bar.insert(tab(2, BROWSER), 1);
        bar.insert(tab(3, MAIL), 1);
        assert_eq!(ids(&bar), vec![1, 2, 3]);
    }

    #[test]
    fn an_emptied_group_keeps_its_place() {
        let mut bar = Bar::default();
        bar.insert(tab(1, MAIL), 0);
        bar.insert(tab(2, BROWSER), 1);
        bar.insert(tab(3, "sola-settings"), 2);
        assert_eq!(bar.remove(2).map(|(at, _)| at), Some(1));
        assert_eq!(bar.group_range(BROWSER), Some((1, 1)));
        // The browser's next window, opened from the end, comes back there.
        bar.insert(tab(4, BROWSER), 2);
        assert_eq!(ids(&bar), vec![1, 4, 3]);
    }

    #[test]
    fn a_new_block_goes_before_an_empty_group_at_its_boundary() {
        let mut bar = Bar::default();
        bar.insert(tab(1, MAIL), 0);
        bar.insert(tab(2, BROWSER), 1);
        bar.remove(2);
        bar.insert(tab(3, "sola-settings"), 1);
        assert_eq!(bar.group_range(BROWSER), Some((2, 2)));
    }

    #[test]
    fn rows_reorder_within_their_group_and_loose_rows_between_blocks() {
        let mut bar = Bar::default();
        bar.insert(tab(1, BROWSER), 0);
        bar.insert(tab(2, BROWSER), 1);
        bar.insert(tab(3, MAIL), 2);
        // Before a row outside its group: the group's end.
        assert!(bar.reorder(1, Some(3)));
        assert_eq!(ids(&bar), vec![2, 1, 3]);
        assert!(bar.reorder(1, Some(2)));
        assert_eq!(ids(&bar), vec![1, 2, 3]);
        // A loose row before a grouped one: before its group.
        assert!(bar.reorder(3, Some(2)));
        assert_eq!(ids(&bar), vec![3, 1, 2]);
    }

    #[test]
    fn a_group_moves_whole() {
        let mut bar = Bar::default();
        bar.insert(tab(1, BROWSER), 0);
        bar.insert(tab(2, BROWSER), 1);
        bar.insert(tab(3, MAIL), 2);
        assert!(bar.move_block(&BlockId::Group(BROWSER.into()), None));
        assert_eq!(ids(&bar), vec![3, 1, 2]);
        assert!(bar.move_block(&BlockId::Window(3), None));
        assert_eq!(ids(&bar), vec![1, 2, 3]);
        assert!(!bar.move_block(&BlockId::Window(3), None));
    }

    #[test]
    fn ranked_rows_line_up_as_saved_around_waiting_groups() {
        // Saved: mail 0, browser 1–2, settings 3; nothing back yet.
        let mut bar = Bar::of_groups(vec![(BROWSER.into(), 1)]);
        let ranks = [(10, 0), (11, 1), (12, 2), (13, 3)];
        let rank_of = |w: u32| ranks.iter().find(|(id, _)| *id == w).map(|(_, r)| *r);
        bar.insert_ranked(tab(13, "sola-settings"), 3, &rank_of);
        bar.insert_ranked(tab(12, BROWSER), 2, &rank_of);
        bar.insert_ranked(tab(10, MAIL), 0, &rank_of);
        bar.insert_ranked(tab(11, BROWSER), 1, &rank_of);
        assert_eq!(ids(&bar), vec![10, 11, 12, 13]);
    }

    #[test]
    fn clear_keeps_the_groups_places() {
        let mut bar = Bar::default();
        bar.insert(tab(1, MAIL), 0);
        bar.insert(tab(2, BROWSER), 1);
        bar.clear();
        assert_eq!(bar.len(), 0);
        assert_eq!(bar.group_range(BROWSER), Some((0, 0)));
    }

    #[test]
    fn keys_name_their_app() {
        assert_eq!(app_of_key("sola-browser/p12"), "sola-browser");
        assert_eq!(app_of_key("sola-mail"), "sola-mail");
    }
}
