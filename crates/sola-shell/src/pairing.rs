//! Which window each announced tab is (Next, slice 2).
//!
//! River gives the window manager no id a client chose, so an app that
//! opens a window per tab announces each tab (`Topic::Tab`: key, pid, and
//! `seq`, how many windows the process had opened) just before the window
//! opens. sola-river mints window ids in creation order, so a process's
//! unpaired tabs (by `seq`) and its unpaired windows (by id) pair up in
//! order. A pair holds while both live; only what is unpaired is paired
//! again, so one late arrival cannot shift the others.
//!
//! Once paired, the app announces the tab again with its window's id, so a
//! restarted shell (every window already up, tabs replaying one at a time)
//! pairs those directly. Only tabs without a live id pair by order.

use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announced {
    pub key: String,
    pub app_id: String,
    pub pid: u32,
    pub seq: u32,
    /// The window id the app says the tab is (from an earlier pairing).
    pub window_id: Option<u32>,
    /// Opened in the background (placed behind, unfocused).
    pub background: bool,
}

/// A window that could be a tab: `(window_id, app_id, pid)`.
pub type Candidate<'a> = (u32, &'a str, u32);

#[derive(Default)]
pub struct Pairing {
    tabs: HashMap<String, Announced>,
    /// window id → tab key.
    paired: HashMap<u32, String>,
}

impl Pairing {
    pub fn announce(&mut self, tab: Announced) {
        self.tabs.insert(tab.key.clone(), tab);
    }

    /// The app closed a tab. Returns the window it was.
    pub fn retract(&mut self, key: &str) -> Option<u32> {
        self.tabs.remove(key);
        let wid = self
            .paired
            .iter()
            .find(|(_, k)| k.as_str() == key)
            .map(|(w, _)| *w)?;
        self.paired.remove(&wid);
        Some(wid)
    }

    /// A window closed. Returns the tab it was. The tab is spent too, so it
    /// cannot pair with the process's next window before the app retracts
    /// it (an app that reopens a tab announces it again).
    pub fn window_closed(&mut self, window_id: u32) -> Option<String> {
        let key = self.paired.remove(&window_id)?;
        self.tabs.remove(&key);
        Some(key)
    }

    pub fn key_of(&self, window_id: u32) -> Option<&str> {
        self.paired.get(&window_id).map(String::as_str)
    }

    pub fn window_of(&self, key: &str) -> Option<u32> {
        self.paired
            .iter()
            .find(|(_, k)| k.as_str() == key)
            .map(|(w, _)| *w)
    }

    /// Was the tab this window is announced as opened in the background?
    pub fn background(&self, window_id: u32) -> bool {
        self.key_of(window_id)
            .and_then(|k| self.tabs.get(k))
            .is_some_and(|t| t.background)
    }

    /// Every paired window and its tab key.
    pub fn pairs(&self) -> impl Iterator<Item = (u32, &str)> {
        self.paired.iter().map(|(w, k)| (*w, k.as_str()))
    }

    /// Pair tabs with windows: first those whose app named the window,
    /// then each process's remaining tabs with its unpaired windows, in
    /// order. Returns the new pairs `(window_id, key)`.
    pub fn pair(&mut self, windows: &[Candidate<'_>]) -> Vec<(u32, String)> {
        let mut new = Vec::new();
        // Named: the window is up, the process and app match, and it is free.
        let named: Vec<(u32, String)> = self
            .tabs
            .values()
            .filter(|t| !self.paired.values().any(|k| *k == t.key))
            .filter_map(|t| {
                let w = t.window_id?;
                let live = windows
                    .iter()
                    .any(|&(id, app, pid)| id == w && app == t.app_id && pid == t.pid);
                (live && !self.paired.contains_key(&w)).then(|| (w, t.key.clone()))
            })
            .collect();
        for (w, k) in named {
            if !self.paired.contains_key(&w) {
                self.paired.insert(w, k.clone());
                new.push((w, k));
            }
        }

        // By order: tabs with no id, or one that is gone (sola-river
        // restarted and renumbered).
        let taken: HashSet<&str> = self.paired.values().map(String::as_str).collect();
        let mut by_pid: BTreeMap<u32, (Vec<&Announced>, Vec<u32>)> = BTreeMap::new();
        for t in self.tabs.values().filter(|t| !taken.contains(t.key.as_str())) {
            let named_live = t
                .window_id
                .is_some_and(|w| windows.iter().any(|&(id, _, _)| id == w));
            if !named_live {
                by_pid.entry(t.pid).or_default().0.push(t);
            }
        }
        for &(wid, app_id, pid) in windows {
            if self.paired.contains_key(&wid) {
                continue;
            }
            if let Some((tabs, wins)) = by_pid.get_mut(&pid) {
                if tabs.iter().any(|t| t.app_id == app_id) {
                    wins.push(wid);
                }
            }
        }
        let mut ordered = Vec::new();
        for (_, (mut tabs, mut wins)) in by_pid {
            tabs.sort_by_key(|t| t.seq);
            wins.sort_unstable();
            for (t, w) in tabs.into_iter().zip(wins) {
                ordered.push((w, t.key.clone()));
            }
        }
        for (w, k) in &ordered {
            self.paired.insert(*w, k.clone());
        }
        new.extend(ordered);
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(key: &str, pid: u32, seq: u32) -> Announced {
        Announced {
            key: key.into(),
            app_id: "sola-terminal".into(),
            pid,
            seq,
            window_id: None,
            background: false,
        }
    }

    fn named(key: &str, pid: u32, seq: u32, window_id: u32) -> Announced {
        Announced {
            window_id: Some(window_id),
            ..tab(key, pid, seq)
        }
    }

    #[test]
    fn a_restarted_shell_pairs_named_tabs_whatever_order_they_replay() {
        let mut p = Pairing::default();
        let wins = [(9, "sola-terminal", 7), (14, "sola-terminal", 7)];
        // The newer tab replays first; by order it would take window 9.
        p.announce(named("t/new", 7, 3, 14));
        assert_eq!(p.pair(&wins), vec![(14, "t/new".into())]);
        p.announce(named("t/old", 7, 1, 9));
        assert_eq!(p.pair(&wins), vec![(9, "t/old".into())]);
    }

    #[test]
    fn a_gone_window_id_falls_back_to_order() {
        let mut p = Pairing::default();
        // sola-river restarted: the named window 40 is no more.
        p.announce(named("t/a", 7, 1, 40));
        assert_eq!(p.pair(&[(3, "sola-terminal", 7)]), vec![(3, "t/a".into())]);
    }

    #[test]
    fn pairs_by_process_and_order_whatever_arrives_first() {
        let mut p = Pairing::default();
        // Windows first: nothing to pair yet.
        let wins = [(12, "sola-terminal", 7), (10, "sola-terminal", 7)];
        assert!(p.pair(&wins).is_empty());
        p.announce(tab("t/b", 7, 2));
        p.announce(tab("t/a", 7, 1));
        let mut new = p.pair(&wins);
        new.sort();
        assert_eq!(new, vec![(10, "t/a".into()), (12, "t/b".into())]);
        assert_eq!(p.key_of(12), Some("t/b"));
        // Nothing new the second time.
        assert!(p.pair(&wins).is_empty());
    }

    #[test]
    fn processes_and_apps_do_not_mix() {
        let mut p = Pairing::default();
        p.announce(tab("t/a", 7, 1));
        p.announce(tab("t/x", 8, 1));
        let wins = [(3, "sola-browser", 7), (4, "sola-terminal", 8), (5, "sola-terminal", 7)];
        let mut new = p.pair(&wins);
        new.sort();
        assert_eq!(new, vec![(4, "t/x".into()), (5, "t/a".into())]);
    }

    #[test]
    fn pairs_hold_while_both_live() {
        let mut p = Pairing::default();
        p.announce(tab("t/a", 7, 1));
        p.pair(&[(10, "sola-terminal", 7)]);
        // A second tab and window later; the first pair does not move.
        p.announce(tab("t/b", 7, 2));
        let new = p.pair(&[(10, "sola-terminal", 7), (11, "sola-terminal", 7)]);
        assert_eq!(new, vec![(11, "t/b".into())]);
        assert_eq!(p.window_closed(10), Some("t/a".into()));
        // The closed window's tab does not pair with the next window.
        p.announce(tab("t/c", 7, 3));
        let new = p.pair(&[(11, "sola-terminal", 7), (12, "sola-terminal", 7)]);
        assert_eq!(new, vec![(12, "t/c".into())]);
        assert_eq!(p.retract("t/b"), Some(11));
        assert_eq!(p.key_of(11), None);
    }
}
