//! Interactive screen-selection overlay for Super+Shift+4.
//!
//! Super+Shift+4 first freezes the live output (RGBA dump, no PNG encode)
//! so menus and text selections stay in the shot, then shows that still
//! full-output (no dim — it is the desktop) with a cyan marquee while
//! dragging. The overlay joins composition only after the freeze texture
//! is on the GPU, so the first visible frame matches the live output.
//! Drag a rectangle; release crops the freeze in memory. Parked at 2×2
//! while dismissed.

pub mod freeze;
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
