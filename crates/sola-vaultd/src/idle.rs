//! When the vault locks itself for idleness (V1).
//!
//! The desk says when it is idle: `sola-river` fades the windows after
//! `Appearance.fade_after_min` minutes without input and says so on the
//! bus (`Topic::IdleFade`). With that fade on, the vault counts idle time
//! from when the desk last had input — the fade's start less its delay —
//! or from its own last use, whichever is later; while the desk is in use
//! it does not lock. With the fade off, or no bus, it counts from its
//! last use alone.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct IdleClock {
    last_use: Instant,
    /// `Appearance.fade_idle` and its delay, once the bus said.
    fade: Option<(bool, Duration)>,
    /// The desk is fading: since when. `None` while it has input.
    fading_since: Option<Instant>,
}

impl IdleClock {
    pub fn new(now: Instant) -> Self {
        Self {
            last_use: now,
            fade: None,
            fading_since: None,
        }
    }

    /// The vault was used.
    pub fn used(&mut self, now: Instant) {
        self.last_use = now;
    }

    /// The desk's fade settings (`Topic::Appearance`).
    pub fn look(&mut self, fade_idle: bool, fade_after_min: u16) {
        self.fade = Some((fade_idle, Duration::from_secs(u64::from(fade_after_min) * 60)));
    }

    /// The desk started or stopped fading (`Topic::IdleFade`).
    pub fn fading(&mut self, fading: bool, now: Instant) {
        match (fading, self.fading_since) {
            (true, None) => self.fading_since = Some(now),
            (false, _) => {
                self.fading_since = None;
                // Input came back: as good as a use.
                self.last_use = self.last_use.max(now);
            }
            (true, Some(_)) => {}
        }
    }

    /// When the vault should lock after `minutes` idle, or `None` while
    /// the desk is in use.
    pub fn deadline(&self, minutes: u32) -> Option<Instant> {
        let limit = Duration::from_secs(u64::from(minutes) * 60);
        let idle_since = match self.fade {
            Some((true, after)) => {
                let since = self.fading_since?;
                let desk = since.checked_sub(after).unwrap_or(since);
                desk.max(self.last_use)
            }
            _ => self.last_use,
        };
        Some(idle_since + limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn without_the_desk_it_counts_from_the_last_use() {
        let t0 = Instant::now();
        let mut c = IdleClock::new(t0);
        assert_eq!(c.deadline(10), Some(t0 + 10 * MIN));
        c.used(t0 + 3 * MIN);
        assert_eq!(c.deadline(10), Some(t0 + 13 * MIN));
        // The fade turned off in Appearance: the same.
        c.look(false, 5);
        assert_eq!(c.deadline(10), Some(t0 + 13 * MIN));
    }

    #[test]
    fn with_the_fade_on_a_desk_in_use_never_locks() {
        let t0 = Instant::now();
        let mut c = IdleClock::new(t0);
        c.look(true, 5);
        assert_eq!(c.deadline(10), None);
        c.fading(false, t0 + 30 * MIN);
        assert_eq!(c.deadline(10), None);
    }

    #[test]
    fn idle_counts_from_the_last_input_before_the_fade() {
        let t0 = Instant::now() + 60 * MIN;
        let mut c = IdleClock::new(t0);
        c.look(true, 5);
        // Last input at t0 + 20; the fade starts 5 minutes later.
        c.fading(true, t0 + 25 * MIN);
        assert_eq!(c.deadline(10), Some(t0 + 30 * MIN));
        // Repeating the fade does not move it.
        c.fading(true, t0 + 26 * MIN);
        assert_eq!(c.deadline(10), Some(t0 + 30 * MIN));
        // A use while fading (a delivery by key) counts.
        c.used(t0 + 28 * MIN);
        assert_eq!(c.deadline(10), Some(t0 + 38 * MIN));
        // Input back: in use again.
        c.fading(false, t0 + 29 * MIN);
        assert_eq!(c.deadline(10), None);
    }
}
