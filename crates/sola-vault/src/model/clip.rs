//! A copied secret leaves the clipboard after the clipboard time — but
//! only if the clipboard still holds it: something you copied since stays.
//! The guard keeps a keyed hash of the value, never the value.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ClipGuard {
    hash: u64,
    pub due: Instant,
}

impl ClipGuard {
    /// `None` when the clipboard is never cleared (`clear_secs` 0).
    pub fn new(value: &str, clear_secs: u32, now: Instant) -> Option<Self> {
        (clear_secs > 0).then(|| Self {
            hash: hash(value),
            due: now + Duration::from_secs(u64::from(clear_secs)),
        })
    }

    pub fn is_due(&self, now: Instant) -> bool {
        now >= self.due
    }

    /// The clipboard's text now is what we put there.
    pub fn holds(&self, current: &str) -> bool {
        hash(current) == self.hash
    }
}

/// SipHash with this process's random key: two runs never agree, and the
/// hash says nothing outside it.
fn hash(value: &str) -> u64 {
    static KEY: OnceLock<RandomState> = OnceLock::new();
    KEY.get_or_init(RandomState::new).hash_one(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_clears_at_zero() {
        assert!(ClipGuard::new("x", 0, Instant::now()).is_none());
    }

    #[test]
    fn due_after_the_clear_time() {
        let now = Instant::now();
        let g = ClipGuard::new("hunter2", 30, now).unwrap();
        assert!(!g.is_due(now + Duration::from_secs(29)));
        assert!(g.is_due(now + Duration::from_secs(30)));
    }

    #[test]
    fn clears_only_our_value() {
        let g = ClipGuard::new("hunter2", 30, Instant::now()).unwrap();
        assert!(g.holds("hunter2"));
        assert!(!g.holds("something you copied since"));
        assert!(!g.holds(""));
    }
}
