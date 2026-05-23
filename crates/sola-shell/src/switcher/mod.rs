//! Switcher subsystem — alt-tab equivalent.
//!
//! `state` holds `SwitcherState` and `SwitcherApp` — pure data.
//! `view` renders the full-overlay card strip.
//! `open_window` opens the persistent overlay at boot.
pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Full-overlay height: 1080 − 28px menubar.  Task 10 corrects from
/// Topic::OutputGeometry.
const SWITCHER_WINDOW_HEIGHT: f32 = 1052.0;

/// Open the switcher overlay window and return `(id, Task<Id>)`.
///
/// Full-screen (1920 × 1052) transparent overlay, positioned immediately
/// below the menubar at Y=28.  Hidden via composition (Task 10) when
/// `switcher.active` is false.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    settings.size = iced::Size::new(1920.0, SWITCHER_WINDOW_HEIGHT);
    settings.position = iced::window::Position::Specific(iced::Point::new(0.0, 28.0));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}

