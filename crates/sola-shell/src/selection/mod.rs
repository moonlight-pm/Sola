//! Interactive screen-selection overlay for Super+Shift+4.
//!
//! Boot-opened transparent surface (like launcher/switcher), shown via
//! composition while `SelectionState::active`. Drag a rectangle; release
//! emits a region capture after the overlay is hidden so the marquee is
//! not in the PNG.

pub mod state;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Placeholder size until `Topic::OutputGeometry` frames the surface.
const SELECTION_WINDOW_W: f32 = 1920.0;
const SELECTION_WINDOW_H: f32 = 1080.0;

/// Open the selection overlay window and return `(id, Task<Id>)`.
///
/// Full output size at (0, 0) so pointer coords match compositor space.
/// Hidden via composition when `selection.active` is false.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    settings.size = iced::Size::new(SELECTION_WINDOW_W, SELECTION_WINDOW_H);
    settings.position = iced::window::Position::Specific(iced::Point::new(0.0, 0.0));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}
