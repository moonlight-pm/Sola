//! Shared menubar menus that kit apps can include (and replace).
//!
//! The **Window** menu is the mouse path for compositor actions that used
//! to live only on Super+numpad / Super+H / Super+`. Apps publish it via
//! [`window_menu`] / [`BusSetup::window_menu`](crate::app::BusSetup::window_menu);
//! the shell injects the same definition when an app omits it, so XWayland
//! and other external windows still get a Window menu.

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::{KeyChord, KeyCode};

/// Menubar label. The shell uses this to decide whether to inject a
/// default Window menu or honor the app's own.
pub const WINDOW_MENU_LABEL: &str = "Window";

pub const ACTION_HIDE: &str = "window.hide";
pub const ACTION_CYCLE: &str = "window.cycle";
pub const ACTION_TILE: &str = "window.tile";
/// Historical id from the zone menu; still parsed as [`WindowAction::Tile`].
pub const ACTION_FLOAT: &str = "window.float";
pub const ACTION_FULLSCREEN: &str = "window.fullscreen";
pub const ACTION_CINEMA: &str = "window.cinema";

/// Compositor action the shell handles instead of forwarding `MenuAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Hide,
    Cycle,
    Tile,
    Fullscreen,
    Cinema,
}

/// One row of the default Window menu (not a divider).
#[derive(Debug, Clone, Copy)]
pub struct WindowMenuItem {
    pub id: &'static str,
    pub label: &'static str,
    pub shortcut: KeyChord,
    pub action: WindowAction,
}

/// Default Window menu, dividers included. Apps that want extras start
/// from [`window_menu`] and push more items; unknown ids still go to the
/// app as `MenuAction`.
#[derive(Debug, Clone, Copy)]
pub enum WindowMenuEntry {
    Item(WindowMenuItem),
    Divider,
}

const fn item(
    id: &'static str,
    label: &'static str,
    shortcut: KeyChord,
    action: WindowAction,
) -> WindowMenuEntry {
    WindowMenuEntry::Item(WindowMenuItem {
        id,
        label,
        shortcut,
        action,
    })
}

/// Canonical Window menu. Keep in lockstep with shell screen/tiling keys.
pub const WINDOW_MENU_ENTRIES: &[WindowMenuEntry] = &[
    item(ACTION_HIDE, "Hide", KeyCode::H.meta(), WindowAction::Hide),
    item(
        ACTION_CYCLE,
        "Cycle Windows",
        KeyCode::GRAVE.meta(),
        WindowAction::Cycle,
    ),
    WindowMenuEntry::Divider,
    item(
        ACTION_TILE,
        "Tile",
        KeyCode::Y.meta(),
        WindowAction::Tile,
    ),
    WindowMenuEntry::Divider,
    item(
        ACTION_FULLSCREEN,
        "Fullscreen",
        KeyCode::M.meta(),
        WindowAction::Fullscreen,
    ),
    item(
        ACTION_CINEMA,
        "Cinema",
        KeyCode::M.meta_shift(),
        WindowAction::Cinema,
    ),
];

/// Default Window menu for [`crate::app::BusSetup::app_menu_definition`].
pub fn window_menu() -> MenuDefinition {
    MenuDefinition {
        label: WINDOW_MENU_LABEL.into(),
        items: WINDOW_MENU_ENTRIES
            .iter()
            .map(|entry| match entry {
                WindowMenuEntry::Divider => MenuItem::Divider,
                WindowMenuEntry::Item(i) => MenuItem::Action {
                    id: i.id.into(),
                    label: i.label.into(),
                    shortcut: Some(i.shortcut),
                    disabled: false,
                    checked: false,
                },
            })
            .collect(),
    }
}

/// Map a menu action id to a compositor window action. `None` means the
/// id is not a kit Window item (forward to the app).
pub fn parse_window_action(id: &str) -> Option<WindowAction> {
    if id == ACTION_FLOAT {
        return Some(WindowAction::Tile);
    }
    WINDOW_MENU_ENTRIES.iter().find_map(|entry| match entry {
        WindowMenuEntry::Item(i) if i.id == id => Some(i.action),
        _ => None,
    })
}

/// Append the default Window menu when the payload does not already
/// declare one (so apps can replace it by publishing their own).
pub fn ensure_window_menu(mut payload: AppMenuPayload) -> AppMenuPayload {
    if payload.menus.iter().any(|m| m.label == WINDOW_MENU_LABEL) {
        payload
    } else {
        payload.menus.push(window_menu());
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_menu_covers_tiling_actions() {
        assert_eq!(parse_window_action(ACTION_TILE), Some(WindowAction::Tile));
        assert_eq!(parse_window_action(ACTION_FLOAT), Some(WindowAction::Tile));
        assert_eq!(
            parse_window_action(ACTION_FULLSCREEN),
            Some(WindowAction::Fullscreen)
        );
        assert_eq!(parse_window_action(ACTION_CINEMA), Some(WindowAction::Cinema));
        assert_eq!(parse_window_action("window.left"), None);
    }

    #[test]
    fn hide_and_cycle_parse() {
        assert_eq!(parse_window_action(ACTION_HIDE), Some(WindowAction::Hide));
        assert_eq!(parse_window_action(ACTION_CYCLE), Some(WindowAction::Cycle));
        assert_eq!(parse_window_action("quit"), None);
        assert_eq!(parse_window_action("window.new"), None);
    }

    #[test]
    fn ensure_window_menu_appends_once() {
        let empty = AppMenuPayload {
            app_id: "firefox".into(),
            menus: vec![MenuDefinition {
                label: "Firefox".into(),
                items: vec![],
            }],
        };
        let once = ensure_window_menu(empty);
        assert_eq!(once.menus.len(), 2);
        assert_eq!(once.menus[1].label, WINDOW_MENU_LABEL);
        let twice = ensure_window_menu(once);
        assert_eq!(twice.menus.len(), 2);
    }

    #[test]
    fn ensure_window_menu_keeps_app_window_menu() {
        let custom = AppMenuPayload {
            app_id: "sola-browser".into(),
            menus: vec![MenuDefinition {
                label: WINDOW_MENU_LABEL.into(),
                items: vec![MenuItem::Action {
                    id: "window.new".into(),
                    label: "New Window".into(),
                    shortcut: None,
                    disabled: false,
                    checked: false,
                }],
            }],
        };
        let out = ensure_window_menu(custom);
        assert_eq!(out.menus.len(), 1);
        match &out.menus[0].items[0] {
            MenuItem::Action { id, .. } => assert_eq!(id, "window.new"),
            other => panic!("expected Action, got {other:?}"),
        }
    }

    #[test]
    fn window_menu_tile_is_super_y() {
        let chord = WINDOW_MENU_ENTRIES.iter().find_map(|e| match e {
            WindowMenuEntry::Item(i) if i.id == ACTION_TILE => Some(i.shortcut),
            _ => None,
        });
        let chord = chord.expect("tile item");
        assert!(chord.meta);
        assert!(!chord.shift);
        assert_eq!(chord.keycode, KeyCode::Y);
    }
}
