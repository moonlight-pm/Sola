//! Menubar window — state, lifecycle, and view entry point.
//!
//! The top row of the desk (Next board 03, "a bubble per group"): the
//! shell's islands on the ground, a gap in from every edge, no band behind
//! them. Left: the flower (system menu), then the workspaces. Center: a
//! toast while one shows. Right, an island per group: extras (mail / bell
//! / mouse battery / bluetooth), the spectrum, the meters (CPU / GPU /
//! MEM), the network (RX / TX), the clock. Only the look of the islands
//! is here; the surfaces that grow out of them come later.
//!
//! Window state lives in [`MenubarState`]; the view is in [`view`]; where
//! the right-hand items sit, for hanging their panels, in [`anchors`].

pub mod anchors;
pub mod pills;
pub mod view;

use chrono::Local;
use iced::window;
use sola_kit::app::window_settings;

/// Height of the menubar window in logical pixels: a gap, then the
/// islands ([`crate::zoning::MENUBAR_HEIGHT`]).
pub const WINDOW_HEIGHT: u32 = crate::zoning::MENUBAR_HEIGHT as u32;
/// An island's height (a pill: its corners are half of it).
pub const ISLAND_H: f32 = crate::zoning::ISLAND_H as f32;
/// The ground between the row and the output's edges.
pub const EDGE: f32 = crate::zoning::GAP as f32;
/// Between islands.
pub const ISLAND_GAP: f32 = 6.0;
/// Inside an island, either side of its content.
pub const ISLAND_PAD_H: f32 = 12.0;
/// Between items in one island (CPU · GPU · MEM, the extras).
pub const ITEM_GAP: f32 = 12.0;
/// Between a stat label and its pixel graph.
pub const STAT_INNER_SPACING: f32 = 5.0;
/// The spectrum's island: inset the same all round, so the matrix's round
/// ends sit concentric with the pill's.
pub const SPECTRUM_PAD_H: f32 = (ISLAND_H - crate::audio::wave::SPECTRUM_H) / 2.0;
/// Lucide extras and the flower.
pub const ICON_SIZE: u16 = 14;
/// The row's type size (labels, clock, workspace names).
pub const CHROME_SIZE: f32 = 13.0;

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
    fn the_row_is_a_gap_then_an_island() {
        assert_eq!(WINDOW_HEIGHT as f32, EDGE + ISLAND_H);
        assert!(ISLAND_GAP < ITEM_GAP);
    }
}
