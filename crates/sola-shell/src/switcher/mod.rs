//! Switcher subsystem — alt-tab equivalent.
//!
//! `state` holds `SwitcherState` and `SwitcherApp` — pure data.
//! `view` renders the full-overlay card strip.
//! `open_window` parks a 2×2 surface; shown while the switcher is active.
pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Open the switcher parked at 2×2. Show Frames to the live usable area.
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
