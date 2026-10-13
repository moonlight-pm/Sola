//! Per-board load bookkeeping, fed by the helper's tab snapshot
//! (`is_loading`) and the frame stream (painted).
//!
//! A local file can load between two snapshots and never show `loading`, so
//! "finished a load" is: seen loading, then idle — or idle and painted for
//! [`LOAD_GRACE`]. `auto`-height boards also need their content measured
//! after each load before they count as settled.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

pub const LOAD_GRACE: Duration = Duration::from_secs(1);
/// A board that never settles (no paint, stuck loading) stops being
/// measured after this, so a broken page cannot keep the tick alive.
const MEASURE_GIVE_UP: Duration = Duration::from_secs(15);

#[derive(Default)]
pub struct Loads {
    seen_loading: HashSet<u64>,
    loaded: HashSet<u64>,
    /// Auto-height boards whose height is stale, and since when.
    measure_due: HashMap<u64, Instant>,
    /// Measure calls in flight.
    measuring: HashSet<u64>,
}

impl Loads {
    /// A board started (or restarted) a load. Auto boards need measuring.
    pub fn begin(&mut self, tab: u64, auto_height: bool, now: Instant) {
        self.seen_loading.remove(&tab);
        self.loaded.remove(&tab);
        if auto_height {
            self.measure_due.insert(tab, now);
        }
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

    /// Height is current (or the board is not `auto`).
    pub fn is_measured(&self, tab: u64) -> bool {
        !self.measure_due.contains_key(&tab)
    }

    /// Some `auto` board still needs measuring (keeps the app ticking).
    pub fn has_due(&self) -> bool {
        !self.measure_due.is_empty()
    }

    /// Auto boards to measure now: due, idle and painted (`idle(tab)`), and
    /// either seen finishing a load or idle past the grace. Marks them in
    /// flight.
    pub fn take_measures(&mut self, now: Instant, idle: impl Fn(u64) -> bool) -> Vec<u64> {
        let measuring = &self.measuring;
        self.measure_due.retain(|tab, since| {
            measuring.contains(tab) || now.duration_since(*since) < MEASURE_GIVE_UP
        });
        let mut out = Vec::new();
        for (&tab, &since) in &self.measure_due {
            if self.measuring.contains(&tab) || !idle(tab) {
                continue;
            }
            if self.loaded.contains(&tab) || now.duration_since(since) >= LOAD_GRACE {
                out.push(tab);
            }
        }
        self.measuring.extend(out.iter().copied());
        out
    }

    /// A measure answered (or failed): the height is current again.
    pub fn measured(&mut self, tab: u64) {
        self.measuring.remove(&tab);
        self.measure_due.remove(&tab);
    }

    pub fn forget(&mut self, tab: u64) {
        self.seen_loading.remove(&tab);
        self.loaded.remove(&tab);
        self.measure_due.remove(&tab);
        self.measuring.remove(&tab);
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
        l.begin(1, false, t0);
        l.observe(&snap(&[(1, false)]), [1]);
        assert!(!l.is_loaded(1), "idle before any load is not a finished load");
        l.observe(&snap(&[(1, true)]), [1]);
        l.observe(&snap(&[(1, false)]), [1]);
        assert!(l.is_loaded(1));
        l.begin(1, false, t0);
        assert!(!l.is_loaded(1));
    }

    #[test]
    fn auto_boards_measure_once_after_load_or_grace() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        l.begin(7, true, t0);
        assert!(!l.is_measured(7));
        assert!(l.take_measures(t0, |_| true).is_empty(), "not loaded, no grace yet");
        assert_eq!(l.take_measures(t0 + LOAD_GRACE, |_| true), vec![7]);
        assert!(l.take_measures(t0 + LOAD_GRACE * 2, |_| true).is_empty(), "in flight");
        l.measured(7);
        assert!(l.is_measured(7));

        l.begin(7, true, t0);
        l.observe(&snap(&[(7, true)]), [7]);
        l.observe(&snap(&[(7, false)]), [7]);
        assert_eq!(l.take_measures(t0, |_| true), vec![7], "a seen load skips the grace");
        assert!(l.take_measures(t0, |_| false).is_empty());
    }

    #[test]
    fn a_board_that_never_settles_is_given_up() {
        let mut l = Loads::default();
        let t0 = Instant::now();
        l.begin(9, true, t0);
        assert!(l.take_measures(t0 + MEASURE_GIVE_UP, |_| false).is_empty());
        assert!(!l.has_due());
        assert!(l.is_measured(9));
    }

    #[test]
    fn fixed_boards_are_always_measured() {
        let mut l = Loads::default();
        l.begin(3, false, Instant::now());
        assert!(l.is_measured(3));
        l.forget(3);
        assert!(!l.is_loaded(3));
    }
}
