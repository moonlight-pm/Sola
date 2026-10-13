//! Per-board load bookkeeping, fed by the helper's tab snapshot
//! (`is_loading`) and the frame stream (painted).
//!
//! A local file can load between two snapshots and never show `loading`, so
//! "finished a load" is: seen loading, then idle — or idle and painted for
//! [`LOAD_GRACE`]. After each load a board is **settled** (web fonts ready,
//! layout final; its content height and loaded files read back) before
//! `wait` counts it done. Each load has a generation, so a settle answered
//! after the board navigated on (`Inspected target navigated`) cannot mark
//! the new load settled. An `auto` resize can ask for a confirming settle
//! (the new viewport may reflow), at most [`MAX_CONFIRMS`] per load. After
//! an `auto` board settles its page **watches its height** (one per load)
//! and reports each change, so content that arrives late grows the board;
//! changes that only follow a resize (`vh` units) are bounded by
//! [`MAX_REFLOWS`] in a row.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

pub const LOAD_GRACE: Duration = Duration::from_secs(1);
/// A board that never settles (no paint, stuck loading) is given up on after
/// this, so a broken page cannot keep the tick alive.
const SETTLE_GIVE_UP: Duration = Duration::from_secs(15);
/// Confirming settles after an `auto` resize, per load (bounds `vh` growth).
pub const MAX_CONFIRMS: u8 = 2;
/// Height reports in a row applied when the page's viewport changed since
/// it last measured: a page sized in `vh` grows each time its board does.
pub const MAX_REFLOWS: u8 = 2;

#[derive(Default)]
pub struct Loads {
    seen_loading: HashSet<u64>,
    loaded: HashSet<u64>,
    /// Boards whose settle is due, and since when.
    settle_due: HashMap<u64, Instant>,
    /// Settle calls in flight (tab → load generation).
    settling: HashMap<u64, u64>,
    /// Load generation per tab; bumped by [`Loads::begin`].
    generation: HashMap<u64, u64>,
    /// Confirming settles used by the current load.
    confirms: HashMap<u64, u8>,
    /// Height watch per tab: the load generation it runs for, and reflow
    /// reports applied in a row.
    watch: HashMap<u64, (u64, u8)>,
}

impl Loads {
    /// A board started (or restarted) a load: it needs settling again.
    pub fn begin(&mut self, tab: u64, now: Instant) {
        self.seen_loading.remove(&tab);
        self.loaded.remove(&tab);
        self.settle_due.insert(tab, now);
        self.settling.remove(&tab);
        self.confirms.remove(&tab);
        self.watch.remove(&tab);
        *self.generation.entry(tab).or_default() += 1;
    }

    /// The current load generation of `tab`.
    pub fn generation(&self, tab: u64) -> u64 {
        self.generation.get(&tab).copied().unwrap_or_default()
    }

    /// Start the height watch for this load of `tab`. False when it runs.
    pub fn watch(&mut self, tab: u64) -> bool {
        let generation = self.generation(tab);
        if self.watch.get(&tab).is_some_and(|w| w.0 == generation) {
            return false;
        }
        self.watch.insert(tab, (generation, 0));
        true
    }

    /// The watch ended without a report (it may start again).
    pub fn unwatch(&mut self, tab: u64) {
        self.watch.remove(&tab);
    }

    /// A height report: may it resize the board? A `reflow` (the viewport
    /// changed since the page last measured) is applied [`MAX_REFLOWS`]
    /// times in a row; any other change resets the count.
    pub fn reflowed(&mut self, tab: u64, reflow: bool) -> bool {
        let Some((_, streak)) = self.watch.get_mut(&tab) else {
            return false;
        };
        if !reflow {
            *streak = 0;
            return true;
        }
        if *streak >= MAX_REFLOWS {
            return false;
        }
        *streak += 1;
        true
    }

    /// Settle once more after an `auto` resize (the new viewport may
    /// reflow). False when this load used its confirms.
    pub fn confirm(&mut self, tab: u64, now: Instant) -> bool {
        let used = self.confirms.entry(tab).or_default();
        if *used >= MAX_CONFIRMS {
            return false;
        }
        *used += 1;
        self.settle_due.insert(tab, now);
        true
    }

    /// Fold one tab snapshot (`tab → loading`) for `tabs`.
    pub fn observe(&mut self, loading: &HashMap<u64, bool>, tabs: impl IntoIterator<Item = u64>) {
        for tab in tabs {
            match loading.get(&tab) {
                Some(true) => {
                    self.seen_loading.insert(tab);
                }
                Some(false) if self.seen_loading.contains(&tab) => {
                    self.loaded.insert(tab);
                }
                _ => {}
            }
        }
    }

    pub fn is_loaded(&self, tab: u64) -> bool {
        self.loaded.contains(&tab)
    }

    /// Settled since its last load.
    pub fn is_settled(&self, tab: u64) -> bool {
        !self.settle_due.contains_key(&tab)
    }

    /// Some board still needs settling (keeps the app ticking).
    pub fn has_due(&self) -> bool {
        !self.settle_due.is_empty()
    }

    /// Boards to settle now: due, idle and painted (`idle(tab)`), and either
    /// seen finishing a load or idle past the grace. Marks them in flight.
    /// Returned as `(tab, generation)`; pass the generation to
    /// [`Loads::settled`].
    pub fn take_settles(&mut self, now: Instant, idle: impl Fn(u64) -> bool) -> Vec<(u64, u64)> {
        let settling = &self.settling;
        self.settle_due.retain(|tab, since| {
            settling.contains_key(tab) || now.duration_since(*since) < SETTLE_GIVE_UP
        });
        let mut out = Vec::new();
        for (&tab, &since) in &self.settle_due {
            if self.settling.contains_key(&tab) || !idle(tab) {
                continue;
            }
            if self.loaded.contains(&tab) || now.duration_since(since) >= LOAD_GRACE {
                out.push((tab, self.generation.get(&tab).copied().unwrap_or_default()));
            }
        }
        self.settling.extend(out.iter().copied());
        out
    }

    /// A settle answered (or failed). False when it belongs to an earlier
    /// load of the tab: ignore it, the current load still needs settling.
    pub fn settled(&mut self, tab: u64, generation: u64) -> bool {
        if self.generation.get(&tab).copied().unwrap_or_default() != generation {
            return false;
        }
        self.settling.remove(&tab);
        self.settle_due.remove(&tab);
        true
    }

    pub fn forget(&mut self, tab: u64) {
        self.seen_loading.remove(&tab);
        self.loaded.remove(&tab);
        self.settle_due.remove(&tab);
        self.settling.remove(&tab);
        self.generation.remove(&tab);
        self.confirms.remove(&tab);
        self.watch.remove(&tab);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pairs: &[(u64, bool)]) -> HashMap<u64, bool> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn loading_then_idle_is_loaded() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        l.begin(1, t0);
        l.observe(&snap(&[(1, false)]), [1]);
        assert!(!l.is_loaded(1), "idle before any load is not a finished load");
        l.observe(&snap(&[(1, true)]), [1]);
        l.observe(&snap(&[(1, false)]), [1]);
        assert!(l.is_loaded(1));
        l.begin(1, t0);
        assert!(!l.is_loaded(1));
    }

    #[test]
    fn boards_settle_once_after_load_or_grace() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        l.begin(7, t0);
        assert!(!l.is_settled(7));
        assert!(l.take_settles(t0, |_| true).is_empty(), "not loaded, no grace yet");
        assert_eq!(l.take_settles(t0 + LOAD_GRACE, |_| true), vec![(7, 1)]);
        assert!(l.take_settles(t0 + LOAD_GRACE * 2, |_| true).is_empty(), "in flight");
        assert!(l.settled(7, 1));
        assert!(l.is_settled(7));

        l.begin(7, t0);
        l.observe(&snap(&[(7, true)]), [7]);
        l.observe(&snap(&[(7, false)]), [7]);
        assert_eq!(l.take_settles(t0, |_| true), vec![(7, 2)], "a seen load skips the grace");
        assert!(l.take_settles(t0, |_| false).is_empty());
    }

    #[test]
    fn a_reply_for_an_earlier_load_is_ignored() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        l.begin(4, t0);
        let taken = l.take_settles(t0 + LOAD_GRACE, |_| true);
        assert_eq!(taken, vec![(4, 1)]);
        l.begin(4, t0); // navigated before the settle answered
        assert!(!l.settled(4, 1), "stale generation");
        assert!(!l.is_settled(4), "the new load still needs settling");
        assert_eq!(l.take_settles(t0 + LOAD_GRACE, |_| true), vec![(4, 2)]);
        assert!(l.settled(4, 2));
    }

    #[test]
    fn confirms_are_bounded_per_load() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        l.begin(5, t0);
        assert!(l.confirm(5, t0));
        assert!(l.confirm(5, t0));
        assert!(!l.confirm(5, t0), "max {MAX_CONFIRMS}");
        l.begin(5, t0);
        assert!(l.confirm(5, t0), "a new load gets new confirms");
    }

    #[test]
    fn one_height_watch_per_load() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        l.begin(3, t0);
        assert_eq!(l.take_settles(t0 + LOAD_GRACE, |_| true), vec![(3, 1)]);
        assert!(l.settled(3, 1));
        assert!(l.watch(3));
        assert!(!l.watch(3), "confirming settles do not start another");
        assert!(!l.has_due(), "a watch needs no tick");
        l.unwatch(3);
        assert!(l.watch(3), "a failed watch may start again");
        l.begin(3, t0);
        assert!(l.watch(3), "a new load watches again");
    }

    #[test]
    fn reflows_in_a_row_are_bounded() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        assert!(!l.reflowed(6, false), "no watch, no resize");
        l.begin(6, t0);
        l.watch(6);
        for _ in 0..MAX_REFLOWS {
            assert!(l.reflowed(6, true));
        }
        assert!(!l.reflowed(6, true), "a vh page stops growing");
        assert!(l.reflowed(6, false), "new content still resizes");
        assert!(l.reflowed(6, true), "and the count starts over");
    }

    #[test]
    fn a_board_that_never_settles_is_given_up() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        l.begin(9, t0);
        assert!(l.take_settles(t0 + SETTLE_GIVE_UP, |_| false).is_empty());
        assert!(!l.has_due());
        assert!(l.is_settled(9));
        l.forget(9);
        assert!(!l.is_loaded(9));
    }
}
