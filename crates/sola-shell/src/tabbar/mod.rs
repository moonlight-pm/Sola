//! The tab bar (Next): a shell window docked left under the menubar that
//! lists every window by zone (Full, Left, Right, Floating). A row shows
//! its window in its zone; drag a row to reorder it or move it to another
//! zone. The model is
//! [`crate::stacks`]; the spec is `docs/specs/2026-10-21-next-tab-bar-design.md`.

pub mod view;

use iced::window;
use sola_bus::topics::Zone;
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
    window::open(settings)
}

/// The bar's section id for a zone (drop target).
pub fn section_id(zone: Zone) -> &'static str {
    match zone {
        Zone::Fullscreen => "zone-full",
        Zone::Left => "zone-left",
        Zone::Right => "zone-right",
        _ => "zone-float",
    }
}

pub fn zone_of_section(id: &str) -> Option<Zone> {
    match id {
        "zone-full" => Some(Zone::Fullscreen),
        "zone-left" => Some(Zone::Left),
        "zone-right" => Some(Zone::Right),
        "zone-float" => Some(Zone::Float),
        _ => None,
    }
}

/// A row's id: its window.
pub fn row_id(window_id: u32) -> String {
    format!("w{window_id}")
}

pub fn window_of_row(id: &str) -> Option<u32> {
    id.strip_prefix('w')?.parse().ok()
}

pub fn zone_label(zone: Zone) -> &'static str {
    match zone {
        Zone::Fullscreen => "Full",
        Zone::Left => "Left",
        Zone::Right => "Right",
        _ => "Floating",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        assert_eq!(window_of_row(&row_id(42)), Some(42));
        assert_eq!(window_of_row("zone-left"), None);
        for zone in crate::stacks::ZONES {
            assert_eq!(zone_of_section(section_id(zone)), Some(zone));
        }
    }
}
