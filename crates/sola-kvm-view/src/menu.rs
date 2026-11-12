//! Remote Desktop's menus: its own, and View with Mirror or Extend, and
//! Statistics.

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};

use crate::APP_ID;

/// View → Statistics: the window's corner says how the pictures come.
pub const STATISTICS: &str = "statistics";
/// View → Mirror: the other machine's own screen.
pub const MIRROR: &str = "mirror";
/// View → Extend: a screen of the window's size, added to the other
/// machine's and made its main one.
pub const EXTEND: &str = "extend";

fn item(id: &str, label: &str, checked: bool) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut: None,
        disabled: false,
        checked,
    }
}

/// The menus, with Mirror or Extend checked as `mirrored` (what the window
/// shows, not only what was chosen), and Statistics while they show. Mirror
/// is greyed out while the other machine has no screen of its own on (a
/// laptop's lid closed).
pub fn app_menu(statistics: bool, mirrored: bool, can_mirror: bool) -> AppMenuPayload {
    let mirror_item = MenuItem::Action {
        id: MIRROR.into(),
        label: "Mirror".into(),
        shortcut: None,
        disabled: !can_mirror,
        checked: mirrored,
    };
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
                items: vec![
                    mirror_item,
                    item(EXTEND, "Extend", !mirrored),
                    MenuItem::Divider,
                    item(STATISTICS, "Statistics", statistics),
                ],
            },
        ],
    }
}
