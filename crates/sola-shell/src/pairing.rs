//! Which window each announced tab is (Next, slice 2).
//!
//! River gives the window manager no id a client chose, so an app that
//! opens a window per tab announces each tab (`Topic::Tab`: key, pid, and
//! `seq`, how many windows the process had opened) just before the window
//! opens. sola-river mints window ids in creation order, so a process's
//! unpaired tabs (by `seq`) and its unpaired windows (by id) pair up in
//! order. A pair holds while both live; only what is unpaired is paired
//! again, so one late arrival cannot shift the others.

use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announced {
    pub key: String,
    pub app_id: String,
    pub pid: u32,
    pub seq: u32,
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

    /// Pair each process's unpaired tabs with its unpaired windows, in
    /// order. Returns the new pairs `(window_id, key)`.
    pub fn pair(&mut self, windows: &[Candidate<'_>]) -> Vec<(u32, String)> {
        let taken: HashSet<&str> = self.paired.values().map(String::as_str).collect();
        let mut by_pid: BTreeMap<u32, (Vec<&Announced>, Vec<u32>)> = BTreeMap::new();
        for t in self.tabs.values().filter(|t| !taken.contains(t.key.as_str())) {
            by_pid.entry(t.pid).or_default().0.push(t);
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
        let mut new = Vec::new();
        for (_, (mut tabs, mut wins)) in by_pid {
            tabs.sort_by_key(|t| t.seq);
            wins.sort_unstable();
            for (t, w) in tabs.into_iter().zip(wins) {
                new.push((w, t.key.clone()));
            }
        }
        for (w, k) in &new {
            self.paired.insert(*w, k.clone());
        }
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
        }
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
