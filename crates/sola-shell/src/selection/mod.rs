//! Interactive screen-selection overlay for Super+Shift+4.
//!
//! Transparent full-output surface, parked at 2×2 while dismissed and
//! framed to the output while `SelectionState::active`. Drag a rectangle;
//! release emits a region capture after the overlay is hidden so the
//! marquee is not in the PNG.

pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Open the selection overlay parked at 2×2. Show Frames to the live output.
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
