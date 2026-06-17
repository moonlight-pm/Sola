//! Menubar system monitors: background sampling + the snapshot model.
//! See docs/specs/2026-06-16-menubar-system-monitors-design.md.

pub mod cpu;

use iced::Color;

/// Which metric an indicator / panel represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    Cpu,
    Gpu,
    Mem,
    Net,
}

/// Threshold colors for level metrics (cpu/gpu/mem). Net is a rate, no level.
pub const AMBER: Color = Color::from_rgb(0.824, 0.600, 0.133); // #d29922
pub const RED: Color = Color::from_rgb(0.973, 0.318, 0.286); // #f85149
pub const WARN_PCT: f32 = 75.0;
pub const CRIT_PCT: f32 = 90.0;

/// Pick the readout color for a percentage level: `neutral` until WARN, then
/// amber, then red at/above CRIT.
pub fn level_color(pct: f32, neutral: Color) -> Color {
    if pct >= CRIT_PCT {
        RED
    } else if pct >= WARN_PCT {
        AMBER
    } else {
        neutral
    }
}

use std::collections::VecDeque;

/// Fixed-capacity sample window for a metric's history graph.
#[derive(Clone, Debug)]
pub struct History {
    cap: usize,
    buf: VecDeque<f32>,
}

impl History {
    pub fn new(cap: usize) -> Self {
        Self { cap, buf: VecDeque::with_capacity(cap) }
    }
    pub fn push(&mut self, v: f32) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }
    /// Samples oldest→newest as a contiguous slice (compacts the deque).
    pub fn samples(&mut self) -> &[f32] {
        self.buf.make_contiguous();
        self.buf.as_slices().0
    }
    pub fn peak(&self) -> f32 {
        self.buf.iter().copied().fold(0.0, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Color;

    #[test]
    fn threshold_color_bands() {
        // neutral below warn, amber in [warn,crit), red at/above crit
        let neutral = Color::from_rgb(0.902, 0.929, 0.953); // #e6edf3
        assert_eq!(level_color(10.0, neutral), neutral);
        assert_eq!(level_color(80.0, neutral), AMBER);
        assert_eq!(level_color(95.0, neutral), RED);
        // boundaries
        assert_eq!(level_color(WARN_PCT, neutral), AMBER);
        assert_eq!(level_color(CRIT_PCT, neutral), RED);
    }

    #[test]
    fn history_keeps_last_n() {
        let mut h = History::new(3);
        for v in [1.0, 2.0, 3.0, 4.0] {
            h.push(v);
        }
        assert_eq!(h.samples(), &[2.0, 3.0, 4.0]);
    }

    #[test]
    fn history_peak() {
        let mut h = History::new(4);
        for v in [5.0, 9.0, 3.0] {
            h.push(v);
        }
        assert_eq!(h.peak(), 9.0);
    }
}
