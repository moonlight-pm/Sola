//! The tab bar (Next): a shell window docked left under the menubar that
//! lists every window on the showing workspace in one run — not grouped by
//! zone, though each window keeps its zone (Full, Left, Right, Floating) —
//! with the windows of each app that opens a window per document under
//! that app's header. A row shows its window in its zone; drag a row to
//! reorder it within its group (its zone stays), a group by its header.
//! The model is [`crate::stacks`]; the spec is
//! `docs/specs/2026-10-21-next-tab-bar-design.md`.

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

/// A row's id: its window and its workspace. The bar animates a row it knows
/// moving, so a common window, with a row on every workspace, must have
/// another row on each: showing that workspace puts it in place at once.
pub fn row_id(workspace: &str, window_id: u32) -> String {
    format!("w{window_id}@{workspace}")
}

pub fn window_of_row(id: &str) -> Option<u32> {
    id.strip_prefix('w')?.split_once('@')?.0.parse().ok()
}

/// A group header's id: its workspace and its app. The bar animates a row it
/// knows moving, so an app's group on another workspace must be another row:
/// showing that workspace puts it in place at once.
pub fn group_row_id(workspace: &str, app: &str) -> String {
    format!("g:{workspace}:{app}")
}

/// The block a row or a group header stands for.
pub fn block_of_row(id: &str) -> Option<crate::stacks::BlockId> {
    use crate::stacks::BlockId;
    match id.strip_prefix("g:") {
        Some(group) => {
            let (_, app) = group.split_once(':')?;
            Some(BlockId::Group(app.to_string()))
        }
        None => window_of_row(id).map(BlockId::Window),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        use crate::stacks::BlockId;
        assert_eq!(window_of_row(&row_id("ws-1", 42)), Some(42));
        assert_ne!(row_id("ws-1", 42), row_id("ws-2", 42));
        assert_eq!(window_of_row("ghost"), None);
        assert_eq!(window_of_row(&group_row_id("ws-1", "sola-browser")), None);
        assert_eq!(
            block_of_row(&group_row_id("ws-1", "sola-browser")),
            Some(BlockId::Group("sola-browser".into()))
        );
        assert_eq!(block_of_row(&row_id("main", 7)), Some(BlockId::Window(7)));
        assert_eq!(block_of_row("ghost"), None);
    }

    #[test]
    fn an_app_s_group_is_another_row_on_another_workspace() {
        assert_ne!(
            group_row_id("ws-1", "sola-terminal"),
            group_row_id("ws-2", "sola-terminal")
        );
    }
}
