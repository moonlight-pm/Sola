//! Launcher subsystem.
//!
//! `state` holds `LauncherState` and the filter logic — pure data.
//! Window management (iced surface, input handling) lands in Task 5.
pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Open the launcher parked at 2×2. Show is a Frame + iced resize to the
/// live output, not a new map.
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
