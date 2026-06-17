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
}
