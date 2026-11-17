//! Remote Desktop's menus: its own, then View (Mirror or Extend, and
//! Statistics) for a machine's window, or Connection for the list. They
//! follow the window focused.

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};

use crate::APP_ID;

/// Remote Desktop → Connections: the list of machines comes up.
pub const CONNECTIONS: &str = "connections";
/// View → Statistics: the window's corner says how the pictures come.
pub const STATISTICS: &str = "statistics";
/// View → Mirror: the other machine's own screen.
pub const MIRROR: &str = "mirror";
/// View → Extend: a screen of the window's size, added to the other
/// machine's and made its main one.
pub const EXTEND: &str = "extend";
/// Connection → Connect: the selected machine's window.
pub const CONNECT: &str = "connect";
/// Connection → Forget: this machine no longer views the selected one.
pub const FORGET: &str = "forget";

/// A machine's View menu, as its window has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    pub statistics: bool,
    /// What the window shows, not only what was chosen.
    pub mirrored: bool,
    /// The other machine has a screen of its own on (a laptop's lid open).
    pub can_mirror: bool,
    /// The other machine can make a screen of the window's size.
    pub can_extend: bool,
}

/// What the focused window is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Machine(View),
    /// The list, with the machine its selected row names.
    Connections(Option<String>),
}

fn item(id: &str, label: &str, checked: bool, disabled: bool) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut: None,
        disabled,
        checked,
    }
}

pub fn app_menu(focus: &Focus) -> AppMenuPayload {
    let own = MenuDefinition {
        label: "Remote Desktop".into(),
        items: vec![
            item(CONNECTIONS, "Connections", false, false),
            MenuItem::Divider,
            sola_kit::menu::close_window_menu_item(),
            sola_kit::menu::quit_menu_item("Quit Remote Desktop"),
        ],
    };
    let second = match focus {
        Focus::Machine(view) => MenuDefinition {
            label: "View".into(),
            items: vec![
                item(MIRROR, "Mirror", view.mirrored, !view.can_mirror),
                item(EXTEND, "Extend", !view.mirrored, !view.can_extend),
                MenuItem::Divider,
                item(STATISTICS, "Statistics", view.statistics, false),
            ],
        },
        Focus::Connections(selected) => {
            let forget = match selected {
                Some(host) => format!("Forget {host}"),
                None => "Forget".into(),
            };
            MenuDefinition {
                label: "Connection".into(),
                items: vec![
                    item(CONNECT, "Connect", false, selected.is_none()),
                    MenuItem::Divider,
                    item(FORGET, &forget, false, selected.is_none()),
                ],
            }
        }
    };
    AppMenuPayload {
        app_id: APP_ID.into(),
        menus: vec![own, second],
    }
}
