//! Which window each announced tab is (Next, slice 2).
//!
//! River gives the window manager no id a client chose, so an app that
//! opens a window per tab announces each tab (`Topic::Tab`: key, pid, and
//! `seq`, how many windows the process had opened) just before the window
//! opens. sola-river mints window ids in creation order, so a process's
//! unpaired tabs (by `seq`) and its unpaired windows (by id) pair up in
//! order — per app, since one process can hold windows of several (the
//! browser hosts web apps' windows under their own app ids). A pair holds while both live; only what is unpaired is paired
//! again, so one late arrival cannot shift the others.
//!
//! Once paired, the app announces the tab again with its window's id, so a
//! restarted shell (every window already up, tabs replaying one at a time)
//! pairs those directly. Only tabs without a live id pair by order.
//!
//! A kit app's window names its tab outright: the token of its key in its
//! Wayland app id (`sola_bus::topics::tab_token`), which sola-river takes
//! off and publishes as `Window.tab`. Such a window pairs with that tab
//! and no other — whatever came first, in whatever order windows were made,
//! and whatever it was paired with before (an app restarted under the same
//! keys while its old windows were still up). Order and named ids are for
//! windows without a token.

use std::collections::{BTreeMap, HashMap, HashSet};

use sola_bus::topics::tab_token;

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

/// A window that could be a tab: `(window_id, app_id, pid, tab token)`.
pub type Candidate<'a> = (u32, &'a str, u32, Option<&'a str>);

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
    /// it (an app that reopens a tab announces it again). Not when the
    /// window `named` its tab: it pairs only with that tab, which waits for
    /// it, as the same window comes back after a restarted sola-river, until
    /// the app retracts the tab.
    pub fn window_closed(&mut self, window_id: u32, named: bool) -> Option<String> {
        let key = self.paired.remove(&window_id)?;
        if !named {
            self.tabs.remove(&key);
        }
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
        // Told: the window names its tab (its process's and app's).
        let by_token: HashMap<String, &Announced> =
            self.tabs.values().map(|t| (tab_token(&t.key), t)).collect();
        let told: Vec<(u32, String)> = windows
            .iter()
            .filter_map(|&(w, app, pid, tab)| {
                let t = by_token.get(tab?)?;
                (t.app_id == app && t.pid == pid && self.paired.get(&w) != Some(&t.key))
                    .then(|| (w, t.key.clone()))
            })
            .collect();
        for (w, key) in told {
            self.paired.retain(|_, k| *k != key);
            self.paired.insert(w, key.clone());
            new.push((w, key));
        }
        // A window with a token waits for its own tab; the rest pair below.
        let windows: Vec<Candidate<'_>> = windows.iter().copied().filter(|c| c.3.is_none()).collect();
        let windows = windows.as_slice();
        // Named: the window is up, the process and app match, and it is free.
        let named: Vec<(u32, String)> = self
            .tabs
            .values()
            .filter(|t| !self.paired.values().any(|k| *k == t.key))
            .filter_map(|t| {
                let w = t.window_id?;
                let live = windows
                    .iter()
                    .any(|&(id, app, pid, _)| id == w && app == t.app_id && pid == t.pid);
                (live && !self.paired.contains_key(&w)).then(|| (w, t.key.clone()))
            })
            .collect();
        for (w, k) in named {
            if !self.paired.contains_key(&w) {
                self.paired.insert(w, k.clone());
                new.push((w, k));
            }
        }

        // By order, per process and app: tabs with no id, or one that is
        // gone (sola-river restarted and renumbered).
        let taken: HashSet<&str> = self.paired.values().map(String::as_str).collect();
        let mut by_app: BTreeMap<(u32, &str), (Vec<&Announced>, Vec<u32>)> = BTreeMap::new();
        for t in self.tabs.values().filter(|t| !taken.contains(t.key.as_str())) {
            let named_live = t
                .window_id
                .is_some_and(|w| windows.iter().any(|&(id, _, _, _)| id == w));
            if !named_live {
                by_app.entry((t.pid, t.app_id.as_str())).or_default().0.push(t);
            }
        }
        for &(wid, app_id, pid, _) in windows {
            if self.paired.contains_key(&wid) {
                continue;
            }
            if let Some((_, wins)) = by_app.get_mut(&(pid, app_id)) {
                wins.push(wid);
            }
        }
        let mut ordered = Vec::new();
        for (_, (mut tabs, mut wins)) in by_app {
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
        let wins = [(9, "sola-terminal", 7, None), (14, "sola-terminal", 7, None)];
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
        assert_eq!(p.pair(&[(3, "sola-terminal", 7, None)]), vec![(3, "t/a".into())]);
    }

    #[test]
    fn pairs_by_process_and_order_whatever_arrives_first() {
        let mut p = Pairing::default();
        // Windows first: nothing to pair yet.
        let wins = [(12, "sola-terminal", 7, None), (10, "sola-terminal", 7, None)];
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
        let wins = [(3, "sola-browser", 7, None), (4, "sola-terminal", 8, None), (5, "sola-terminal", 7, None)];
        let mut new = p.pair(&wins);
        new.sort();
        assert_eq!(new, vec![(4, "t/x".into()), (5, "t/a".into())]);
    }

    #[test]
    fn one_process_pairs_each_apps_windows_with_its_own_tabs() {
        // The browser hosts a web app: its page and the app's window were
        // opened in the other order than they were announced.
        let mut p = Pairing::default();
        p.announce(Announced {
            app_id: "sola-browser".into(),
            ..tab("sola-browser/page-1", 7, 1)
        });
        p.announce(Announced {
            app_id: "web-1".into(),
            ..tab("web-1/main", 7, 2)
        });
        let wins = [(20, "web-1", 7, None), (21, "sola-browser", 7, None)];
        let mut new = p.pair(&wins);
        new.sort();
        assert_eq!(
            new,
            vec![(20, "web-1/main".into()), (21, "sola-browser/page-1".into())]
        );
    }

    #[test]
    fn pairs_hold_while_both_live() {
        let mut p = Pairing::default();
        p.announce(tab("t/a", 7, 1));
        p.pair(&[(10, "sola-terminal", 7, None)]);
        // A second tab and window later; the first pair does not move.
        p.announce(tab("t/b", 7, 2));
        let new = p.pair(&[(10, "sola-terminal", 7, None), (11, "sola-terminal", 7, None)]);
        assert_eq!(new, vec![(11, "t/b".into())]);
        assert_eq!(p.window_closed(10, false), Some("t/a".into()));
        // The closed window's tab does not pair with the next window.
        p.announce(tab("t/c", 7, 3));
        let new = p.pair(&[(11, "sola-terminal", 7, None), (12, "sola-terminal", 7, None)]);
        assert_eq!(new, vec![(12, "t/c".into())]);
        assert_eq!(p.retract("t/b"), Some(11));
        assert_eq!(p.key_of(11), None);
    }

    fn told(key: &str, pid: u32, window_id: u32) -> (u32, &'static str, u32, Option<String>) {
        (window_id, "sola-browser", pid, Some(tab_token(key)))
    }

    fn browser(key: &str, pid: u32, seq: u32) -> Announced {
        Announced {
            app_id: "sola-browser".into(),
            ..tab(key, pid, seq)
        }
    }

    /// Today's failure: windows made in another order than their tabs, and
    /// a tab naming a window that is now another's. Each window says which
    /// tab it is, so each pairs with its own.
    #[test]
    fn a_window_that_names_its_tab_pairs_with_it_whatever_the_order() {
        let mut p = Pairing::default();
        for (i, k) in ["b/1", "b/2", "b/3"].iter().enumerate() {
            p.announce(Announced {
                window_id: Some(11), // stale: another window has this id now
                ..browser(k, 7, i as u32 + 1)
            });
        }
        let wins = [told("b/3", 7, 10), told("b/1", 7, 11), told("b/2", 7, 12)];
        let refs: Vec<Candidate<'_>> = wins.iter().map(|(w, a, p, t)| (*w, *a, *p, t.as_deref())).collect();
        let mut new = p.pair(&refs);
        new.sort();
        assert_eq!(new, vec![(10, "b/3".into()), (11, "b/1".into()), (12, "b/2".into())]);
        assert!(p.pair(&refs).is_empty(), "nothing new the second time");
    }

    /// The app restarted while its old windows were still up: the new
    /// process's windows take the tabs (same keys), the old ones let go.
    #[test]
    fn a_restarted_app_takes_its_tabs_from_its_old_windows() {
        let mut p = Pairing::default();
        p.announce(browser("b/1", 7, 1));
        let old = [told("b/1", 7, 20)];
        let refs: Vec<Candidate<'_>> = old.iter().map(|(w, a, p, t)| (*w, *a, *p, t.as_deref())).collect();
        assert_eq!(p.pair(&refs), vec![(20, "b/1".into())]);
        // Process 8 announces the same tab; its window comes while 20 is up.
        p.announce(browser("b/1", 8, 1));
        let both = [told("b/1", 7, 20), told("b/1", 8, 30)];
        let refs: Vec<Candidate<'_>> = both.iter().map(|(w, a, p, t)| (*w, *a, *p, t.as_deref())).collect();
        assert_eq!(p.pair(&refs), vec![(30, "b/1".into())]);
        assert_eq!(p.key_of(30), Some("b/1"));
        assert_eq!(p.key_of(20), None);
    }

    /// A window with a token waits for its own tab: it is not paired by
    /// order with another of its process's tabs.
    #[test]
    fn a_window_with_a_token_waits_for_its_tab() {
        let mut p = Pairing::default();
        p.announce(browser("b/1", 7, 1));
        let wins = [told("b/2", 7, 10)];
        let refs: Vec<Candidate<'_>> = wins.iter().map(|(w, a, p, t)| (*w, *a, *p, t.as_deref())).collect();
        assert!(p.pair(&refs).is_empty());
        p.announce(browser("b/2", 7, 2));
        assert_eq!(p.pair(&refs), vec![(10, "b/2".into())]);
    }

    #[test]
    fn a_window_that_named_its_tab_pairs_again_when_it_comes_back() {
        let mut p = Pairing::default();
        p.announce(Announced {
            key: "t/a".into(),
            app_id: "t".into(),
            pid: 7,
            seq: 1,
            window_id: None,
            background: false,
        });
        let token = tab_token("t/a");
        let window = [(10, "t", 7, Some(token.as_str()))];
        assert_eq!(p.pair(&window), [(10, "t/a".to_string())]);
        assert_eq!(p.window_closed(10, true), Some("t/a".into()));
        assert_eq!(p.pair(&window), [(10, "t/a".to_string())]);
        assert_eq!(p.window_closed(10, false), Some("t/a".into()));
        assert!(p.pair(&window).is_empty(), "an unnamed close spends the tab");
    }

}
