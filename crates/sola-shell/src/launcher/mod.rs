//! Launcher subsystem.
//!
//! `state` holds `LauncherState` and the filter logic — pure data.
//! Window management (iced surface, input handling) lands in Task 5.
pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Placeholder height: 1080 − 28px menubar.  Task 10 corrects from
/// Topic::OutputGeometry.
const LAUNCHER_WINDOW_HEIGHT: f32 = 1052.0;

/// Open the launcher overlay window and return `(id, Task<Id>)`.
///
/// Size matches the assumed 1920×1080 output minus the menubar, positioned
/// immediately below the menubar at Y=28.  Transparency is on so the
/// backdrop alpha and the card shadow render correctly.
///
/// The window is opened at boot and stays open for the shell's lifetime,
/// hidden via composition (Task 10) when `launcher.active` is false.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    settings.size = iced::Size::new(1920.0, LAUNCHER_WINDOW_HEIGHT);
    settings.position = iced::window::Position::Specific(iced::Point::new(0.0, 28.0));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}
