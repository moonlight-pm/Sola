//! The tab bar (Next): a shell window docked left under the menubar that
//! lists every window on the showing workspace in one run — not grouped by
//! zone, though each window keeps its zone (Full, Left, Right, Floating).
//! A row shows its window in its zone; drag a row to reorder it (its zone
//! stays). The model is
//! [`crate::stacks`]; the spec is `docs/specs/2026-10-21-next-tab-bar-design.md`.

pub mod head;
pub mod view;

use iced::window;
use sola_kit::app::window_settings;

/// Pointer / drag state of the bar's sidebar.
#[derive(Default)]
pub struct TabBarState {
    pub gestures: sola_kit::components::SidebarState,
}

/// Open the bar's window. Placed by `Topic::Frame` like the menubar.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    settings.size = iced::Size::new(crate::zoning::BAR_WIDTH as f32, 1022.0);
    settings.position = iced::window::Position::Specific(iced::Point::new(
        crate::zoning::GAP as f32,
        (crate::zoning::MENUBAR_HEIGHT + crate::zoning::GAP) as f32,
    ));
    settings.resizable = false;
    settings.decorations = false;
    // The island clears its corners: the ground shows there.
    settings.transparent = true;
    window::open(settings)
}

/// A row's id: its window.
pub fn row_id(window_id: u32) -> String {
    format!("w{window_id}")
}

pub fn window_of_row(id: &str) -> Option<u32> {
    id.strip_prefix('w')?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        assert_eq!(window_of_row(&row_id(42)), Some(42));
        assert_eq!(window_of_row("ghost"), None);
    }
}
