//! Menu subsystem.
//!
//! `state` holds `MenuCache` and `synthesized_menu` — pure data, no window
//! logic. Window management (opening, closing, rendering the dropdown) lands
//! in Task 7.
//! Menu dropdown window — state, lifecycle, and view entry point.
//!
//! The menu window is a full-overlay transparent surface that sits below
//! the menubar and above all other windows in composition. Kept mapped at
//! 2×2 while dismissed so show is Frame + stack, not a new map.
//! Anchor X positioning is computed from `Shell::menu_anchor_x`, which is
//! populated from `MenubarState::label_positions` (font-metric estimates).

pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Open the menu parked at 2×2. Show Frames to the live usable area.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    let p = crate::zoning::OVERLAY_PARK as f32;
    settings.size = iced::Size::new(p, p);
    settings.position = iced::window::Position::Specific(iced::Point::new(
        crate::zoning::OVERLAY_PARK_X as f32,
        crate::zoning::OVERLAY_PARK_Y as f32,
    ));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}
