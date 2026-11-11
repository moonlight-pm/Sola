//! Remote Desktop's menus: its own, and View with Statistics.

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};

use crate::APP_ID;

/// View → Statistics: the window's corner says how the pictures come.
pub const STATISTICS: &str = "statistics";

/// The menus, with Statistics checked while they show.
pub fn app_menu(statistics: bool) -> AppMenuPayload {
    AppMenuPayload {
        app_id: APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Remote Desktop".into(),
                items: vec![
                    sola_kit::menu::close_window_menu_item(),
                    sola_kit::menu::quit_menu_item("Quit Remote Desktop"),
                ],
            },
            MenuDefinition {
                label: "View".into(),
                items: vec![MenuItem::Action {
                    id: STATISTICS.into(),
                    label: "Statistics".into(),
                    shortcut: None,
                    disabled: false,
                    checked: statistics,
                }],
            },
        ],
    }
}
