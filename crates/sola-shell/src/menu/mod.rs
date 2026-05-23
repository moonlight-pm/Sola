//! Menu subsystem.
//!
//! `state` holds `MenuCache` and `synthesized_menu` — pure data, no window
//! logic. Window management (opening, closing, rendering the dropdown) lands
//! in Task 7.
//! Menu dropdown window — state, lifecycle, and view entry point.
//!
//! The menu window is a full-overlay transparent surface that sits below
//! the menubar and above all other windows in composition.  It is opened
//! at startup and hidden by composition until `Shell::menu_open` is true.
//! Anchor X positioning is computed from `Shell::menu_anchor_x`, which is
//! populated from `MenubarState::label_positions` (font-metric estimates).

pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Placeholder height below the menubar (28 px).  Real geometry is wired
/// in Task 10 when Topic::OutputGeometry fires.
const MENU_WINDOW_HEIGHT: f32 = 1052.0; // 1080 − 28

/// Open the menu overlay window and return `(id, Task<Id>)`.
/// Width matches the assumed 1920px output; Task 10 corrects geometry from
/// Topic::OutputGeometry.  Position is (0, 28) — immediately below the menubar.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    settings.size = iced::Size::new(1920.0, MENU_WINDOW_HEIGHT);
    settings.position =
        iced::window::Position::Specific(iced::Point::new(0.0, 28.0));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}
