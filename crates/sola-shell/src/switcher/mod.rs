//! Switcher subsystem — alt-tab equivalent.
//!
//! `state` holds `SwitcherState` and `SwitcherApp` — pure data.
//! `view` renders the full-overlay card strip.
//! `open_window` opens the persistent overlay at boot.
//! `activate` populates the app list and sets `active = true`.
pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

use crate::app::Shell;
use state::{rebuild_apps, SwitcherState};

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

/// Activate the switcher: rebuild the app list from current MRU + open
/// windows, reset selection to 0, and set `active = true`.
///
/// Called from the chord handler (Task 10) when Meta+Tab fires.
/// Also call `rebuild_apps` again on subsequent Meta+Tab/Right presses
/// to keep the list fresh while the switcher is open.
pub fn activate(switcher: &mut SwitcherState, shell: &Shell) {
    rebuild_apps(switcher, &shell.mru_apps, &shell.known_windows);
    switcher.selected = 0;
    switcher.active = true;
}
