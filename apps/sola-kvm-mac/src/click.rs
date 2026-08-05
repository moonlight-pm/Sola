//! Multi-click counting for synthetic CG mouse events.
//!
//! macOS double-click / triple-click recognition depends on
//! `kCGMouseEventClickState` (1, 2, 3…). Always posting `1` makes Finder
//! and AppKit treat every click as a fresh single-click.

use std::time::{Duration, Instant};

/// Default double-click window (macOS is usually ~500 ms; slightly generous).
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(500);

/// Max distance (points) between consecutive downs that still counts as multi-click.
pub const DEFAULT_SLOP: i32 = 6;

/// Tracks press timing/position so we can emit the right click count.
#[derive(Debug, Clone)]
pub struct MultiClick {
    interval: Duration,
    slop: i32,
    last_button: Option<u8>,
    last_down: Option<Instant>,
    last_x: i32,
    last_y: i32,
    /// Count established on the last *down* for that button (used for matching up).
    count: i64,
    /// Per-button count active while held (0..2 → left/right/middle).
    held_count: [i64; 3],
}

impl Default for MultiClick {
    fn default() -> Self {
        Self::new(DEFAULT_INTERVAL, DEFAULT_SLOP)
    }
}

impl MultiClick {
    pub const fn new(interval: Duration, slop: i32) -> Self {
        Self {
            interval,
            slop,
            last_button: None,
            last_down: None,
            last_x: 0,
            last_y: 0,
            count: 0,
            held_count: [0; 3],
        }
    }

    /// Call on button **down**. Returns the click-state to put on the CGEvent (1+).
    pub fn on_down(&mut self, button: u8, x: i32, y: i32, now: Instant) -> i64 {
        let multi = match (self.last_button, self.last_down) {
            (Some(b), Some(t)) if b == button => {
                let dt = now.saturating_duration_since(t);
                let dx = (x - self.last_x).abs();
                let dy = (y - self.last_y).abs();
                dt <= self.interval && dx <= self.slop && dy <= self.slop
            }
            _ => false,
        };

        let count = if multi { self.count + 1 } else { 1 };
        self.last_button = Some(button);
        self.last_down = Some(now);
        self.last_x = x;
        self.last_y = y;
        self.count = count;
        if let Some(slot) = self.held_slot(button) {
            self.held_count[slot] = count;
        }
        count
    }

    /// Call on button **up**. Returns the click-state matching the prior down.
    pub fn on_up(&mut self, button: u8) -> i64 {
        if let Some(slot) = self.held_slot(button) {
            let c = self.held_count[slot].max(1);
            self.held_count[slot] = 0;
            c
        } else {
            1
        }
    }

    /// Reset on leave / enter so a remote session starts clean.
    pub fn reset(&mut self) {
        *self = Self::new(self.interval, self.slop);
    }

    fn held_slot(&self, button: u8) -> Option<usize> {
        match button {
            0..=2 => Some(button as usize),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_click() {
        let mut m = MultiClick::default();
        let t0 = Instant::now();
        assert_eq!(m.on_down(0, 10, 10, t0), 1);
        assert_eq!(m.on_up(0), 1);
    }

    #[test]
    fn double_click_same_spot() {
        let mut m = MultiClick::default();
        let t0 = Instant::now();
        assert_eq!(m.on_down(0, 100, 200, t0), 1);
        assert_eq!(m.on_up(0), 1);
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(m.on_down(0, 101, 201, t1), 2);
        assert_eq!(m.on_up(0), 2);
    }

    #[test]
    fn triple_click() {
        let mut m = MultiClick::default();
        let t0 = Instant::now();
        assert_eq!(m.on_down(0, 0, 0, t0), 1);
        m.on_up(0);
        assert_eq!(m.on_down(0, 0, 0, t0 + Duration::from_millis(80)), 2);
        m.on_up(0);
        assert_eq!(m.on_down(0, 1, 0, t0 + Duration::from_millis(160)), 3);
        assert_eq!(m.on_up(0), 3);
    }

    #[test]
    fn too_far_resets() {
        let mut m = MultiClick::default();
        let t0 = Instant::now();
        assert_eq!(m.on_down(0, 0, 0, t0), 1);
        m.on_up(0);
        assert_eq!(
            m.on_down(0, 100, 0, t0 + Duration::from_millis(50)),
            1
        );
    }

    #[test]
    fn too_slow_resets() {
        let mut m = MultiClick::default();
        let t0 = Instant::now();
        assert_eq!(m.on_down(0, 0, 0, t0), 1);
        m.on_up(0);
        assert_eq!(
            m.on_down(0, 0, 0, t0 + Duration::from_millis(800)),
            1
        );
    }

    #[test]
    fn other_button_resets() {
        let mut m = MultiClick::default();
        let t0 = Instant::now();
        assert_eq!(m.on_down(0, 0, 0, t0), 1);
        m.on_up(0);
        assert_eq!(m.on_down(1, 0, 0, t0 + Duration::from_millis(50)), 1);
    }
}
