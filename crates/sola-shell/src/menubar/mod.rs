//! Menubar window — state, lifecycle, and view entry point.
//!
//! The menubar is the first of the four shell windows to open. It provides:
//! - Left cluster: system-menu button, focused-app title, app-menu labels.
//! - Right cluster: four phrases, tight within and a breath between —
//!   extras (mail / bell / volume / bluetooth), percents (CPU / GPU / MEM),
//!   rates (RX / TX), clock.
//!
//! Window state lives in [`MenubarState`]; the view is in [`view`].
//! Density and type live in the view (compact chrome sizes, font roles);
//! bar height stays fixed so zoning / overlay Y offsets stay stable.

pub mod report;
pub mod view;

use chrono::Local;
use iced::window;
use sola_kit::app::window_settings;

/// Height of the menubar window in logical pixels.
/// Keep in sync with [`crate::zoning::MENUBAR_HEIGHT`] and overlay Y=28.
pub const WINDOW_HEIGHT: u32 = 28;

/// Horizontal pad for left-side menu titles (macOS menu-title rhythm).
pub const MENU_PAD_H: f32 = 9.0;
/// Horizontal pad for icon extras (bell, volume, bluetooth, mail).
pub const EXTRA_PAD_H: f32 = 5.0;
/// Horizontal pad for CPU / GPU / MEM / RX / TX chips.
pub const STAT_PAD_H: f32 = 6.0;
/// Breath between right-cluster phrases (extras · percents · rates · clock).
pub const PHRASE_GAP: f32 = 12.0;
/// Gap between a stat label and its pixel graph.
pub const STAT_INNER_SPACING: f32 = 4.0;
/// Lucide extras in the right cluster.
pub const ICON_SIZE: u16 = 14;

/// Chrome 13pt advance used only for overlay-anchor estimates.
const CHROME_CHAR_W: f32 = 7.5;

/// Icon extra chip width (glyph + pad). Overlay anchors walk from this.
pub fn extra_chip_w() -> f32 {
    ICON_SIZE as f32 + EXTRA_PAD_H * 2.0
}

/// CPU / GPU / MEM chip width (3-letter label + fixed pixel graph).
pub fn percent_chip_w() -> f32 {
    3.0 * CHROME_CHAR_W + STAT_INNER_SPACING + crate::stats::pixel::GRAPH_W + STAT_PAD_H * 2.0
}

/// RX / TX chip width (2-letter label + fixed pixel graph).
pub fn rate_chip_w() -> f32 {
    2.0 * CHROME_CHAR_W + STAT_INNER_SPACING + crate::stats::pixel::GRAPH_W + STAT_PAD_H * 2.0
}

/// Identifies one menubar label for the keyboard-shortcut "flash" feedback,
/// using the same `(is_system, index)` addressing as the open-menu state:
/// `{ is_system: true, index: 0 }` is the system flower, `{ false, 0 }` the
/// focused app's title, and `{ false, n }` its nth menu (`File`, `Edit`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashTarget {
    pub is_system: bool,
    pub index: usize,
}

/// Runtime state for the menubar window.
pub struct MenubarState {
    /// Active toast message, if any.
    pub toast: Option<String>,
    /// Monotonically-increasing generation counter used to cancel stale
    /// toast-expiry callbacks (see `Msg::ToastExpire`).
    pub toast_generation: u64,
    /// Current local time — updated by `Msg::ClockTick`.
    pub clock_now: chrono::DateTime<Local>,
    /// X-coordinates of each app-menu label button, in left-to-right order.
    /// Populated by Task 7 (menu anchor work); declared now so view can
    /// reference the field without a later structural change.
    pub label_positions: Vec<f32>,
    /// Menubar label currently flashing as keyboard-shortcut feedback, if any.
    pub flash: Option<FlashTarget>,
    /// Generation counter to cancel stale flash-expiry callbacks (see
    /// `Msg::MenuFlashExpire`), mirroring `toast_generation`.
    pub flash_generation: u64,
}

impl MenubarState {
    pub fn new() -> Self {
        Self {
            toast: None,
            toast_generation: 0,
            clock_now: Local::now(),
            label_positions: Vec::new(),
            flash: None,
            flash_generation: 0,
        }
    }

    /// Push a new transient toast message and bump the generation counter.
    /// The caller should schedule a `Msg::ToastExpire(self.toast_generation)`
    /// task after calling this.
    pub fn push_toast(&mut self, message: impl Into<String>) {
        self.toast_generation = self.toast_generation.wrapping_add(1);
        self.toast = Some(message.into());
    }

    /// Clear the toast if `generation` matches the current generation.
    /// Stale expiry callbacks (from a superseded toast) are silently ignored.
    pub fn expire_toast(&mut self, generation: u64) {
        if generation == self.toast_generation {
            self.toast = None;
        }
    }

    /// Begin flashing `target` and bump the generation counter. The caller
    /// should schedule a `Msg::MenuFlashExpire(self.flash_generation)` task
    /// after calling this (the brief on→off pulse).
    pub fn begin_flash(&mut self, target: FlashTarget) -> u64 {
        self.flash_generation = self.flash_generation.wrapping_add(1);
        self.flash = Some(target);
        self.flash_generation
    }

    /// Clear the flash if `generation` matches the current generation.
    /// Stale expiry callbacks (from a superseded flash) are silently ignored.
    pub fn expire_flash(&mut self, generation: u64) {
        if generation == self.flash_generation {
            self.flash = None;
        }
    }
}

/// Open the menubar window and return `(id, Task<Id>)`.
/// The `id` is pre-allocated so `Shell` can store it immediately; the task
/// drives the actual OS-level window creation and resolves to the same id.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    settings.size = iced::Size::new(1920.0, WINDOW_HEIGHT as f32);
    settings.position = iced::window::Position::Specific(iced::Point::new(0.0, 0.0));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_generation_cancels_stale_expiry() {
        let mut mb = MenubarState::new();
        let first = mb.begin_flash(FlashTarget {
            is_system: false,
            index: 1,
        });
        assert_eq!(
            mb.flash,
            Some(FlashTarget {
                is_system: false,
                index: 1
            })
        );

        // A second flash supersedes the first; the first's expiry is now stale.
        let second = mb.begin_flash(FlashTarget {
            is_system: true,
            index: 0,
        });
        assert_ne!(first, second);

        // Stale expiry (from the superseded flash) must NOT clear the live one.
        mb.expire_flash(first);
        assert_eq!(
            mb.flash,
            Some(FlashTarget {
                is_system: true,
                index: 0
            })
        );

        // The current generation's expiry clears it.
        mb.expire_flash(second);
        assert_eq!(mb.flash, None);
    }

    #[test]
    fn cluster_chip_widths_are_tighter_than_old_islands() {
        // Old cluster was 9+4+9 pad/gap per item (~80px percents, ~115px rates).
        assert!(extra_chip_w() < 32.0);
        assert!(percent_chip_w() < 80.0);
        assert!(rate_chip_w() < 80.0);
        assert_eq!(percent_chip_w() - rate_chip_w(), CHROME_CHAR_W);
        assert!(PHRASE_GAP > EXTRA_PAD_H * 2.0);
    }
}
