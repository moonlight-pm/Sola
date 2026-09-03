//! Super+K keyboard-shortcuts overlay (Omarchy chord).
//!
//! Lists built-in shell chords plus the focused app's published menu
//! shortcuts. Click or Enter runs the action so the cheatsheet is also
//! a mouse path.

pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Open parked at 2×2. Show Frames to the card, not the output.
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
