//! Caps Lock under a binding. Super+CapsLock shows the next workspace, but
//! River updates the keyboard's state after it has matched a key to a
//! binding, so the same press turns Caps Lock on (or, on its release, off).
//! Put it back: note Caps Lock's state when the binding fires, and when
//! River reports a change before the key is up or within [`SETTLE`] after,
//! turn it back (`river_xkb_keyboard_v1.capslock_enable` / `_disable`).

use std::time::{Duration, Instant};

/// XKB `Caps_Lock`.
pub const CAPS_LOCK: u32 = 0xFFE5;

/// How long after the binding's release a change still counts as its own.
const SETTLE: Duration = Duration::from_millis(400);

#[derive(Default)]
pub struct CapsGuard {
    /// Caps Lock as River last reported it.
    on: Option<bool>,
    /// While a Caps Lock binding is held (or just let go): Caps Lock's
    /// state when it fired, and when it was let go.
    hold: Option<(bool, Option<Instant>)>,
}

impl CapsGuard {
    /// A binding on Caps Lock fired.
    pub fn pressed(&mut self) {
        self.hold = self.on.map(|on| (on, None));
    }

    /// That binding's key went up.
    pub fn released(&mut self, now: Instant) {
        if let Some((on, _)) = self.hold {
            self.hold = Some((on, Some(now)));
        }
    }

    /// River reported Caps Lock's state. Returns the state to put back
    /// when the binding changed it.
    pub fn reported(&mut self, on: bool, now: Instant) -> Option<bool> {
        self.on = Some(on);
        let (was, released) = self.hold?;
        if released.is_some_and(|t| now > t + SETTLE) {
            self.hold = None;
            return None;
        }
        (on != was).then_some(was)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_that_locks_is_undone() {
        let t = Instant::now();
        let mut g = CapsGuard::default();
        g.reported(false, t);
        g.pressed();
        assert_eq!(g.reported(true, t), Some(false));
        // Our own put-back comes in as a report too.
        assert_eq!(g.reported(false, t), None);
        g.released(t);
    }

    #[test]
    fn a_release_that_unlocks_is_undone() {
        let t = Instant::now();
        let mut g = CapsGuard::default();
        g.reported(true, t);
        g.pressed();
        g.released(t);
        assert_eq!(g.reported(false, t + Duration::from_millis(5)), Some(true));
    }

    #[test]
    fn caps_lock_alone_later_is_left_alone() {
        let t = Instant::now();
        let mut g = CapsGuard::default();
        g.reported(false, t);
        g.pressed();
        g.released(t);
        assert_eq!(g.reported(true, t + Duration::from_secs(2)), None);
        assert_eq!(g.reported(false, t + Duration::from_secs(3)), None);
    }

    #[test]
    fn nothing_held_changes_nothing() {
        let t = Instant::now();
        let mut g = CapsGuard::default();
        assert_eq!(g.reported(true, t), None);
        assert_eq!(g.reported(false, t), None);
    }
}
