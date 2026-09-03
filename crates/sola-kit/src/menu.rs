//! Shared menubar menus that kit apps can include (and replace).
//!
//! The **Window** menu is the mouse path for compositor actions that used
//! to live only on Super+numpad / Super+H / Super+`. Apps publish it via
//! [`window_menu`] / [`BusSetup::window_menu`](crate::app::BusSetup::window_menu);
//! the shell injects the same definition when an app omits it, so XWayland
//! and other external windows still get a Window menu.

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem, Zone};
use sola_core::{KeyChord, KeyCode};

/// Menubar label. The shell uses this to decide whether to inject a
/// default Window menu or honor the app's own.
pub const WINDOW_MENU_LABEL: &str = "Window";

pub const ACTION_HIDE: &str = "window.hide";
pub const ACTION_CYCLE: &str = "window.cycle";
pub const ACTION_FLOAT: &str = "window.float";
pub const ACTION_LEFT: &str = "window.left";
pub const ACTION_RIGHT: &str = "window.right";
pub const ACTION_TOP: &str = "window.top";
pub const ACTION_BOTTOM: &str = "window.bottom";
pub const ACTION_TOP_MIDDLE: &str = "window.top-middle";
pub const ACTION_BOTTOM_MIDDLE: &str = "window.bottom-middle";
pub const ACTION_FULL_MIDDLE: &str = "window.full-middle";
pub const ACTION_MIDDLE_RIGHT: &str = "window.middle-right";
pub const ACTION_FULLSCREEN: &str = "window.fullscreen";
pub const ACTION_CINEMA: &str = "window.cinema";

/// Compositor action the shell handles instead of forwarding `MenuAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Hide,
    Cycle,
    Zone(Zone),
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

/// Canonical Window menu. Keep in lockstep with shell zoning keys.
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
        ACTION_FLOAT,
        "Float",
        kp(KeyCode::KP_MULTIPLY),
        WindowAction::Zone(Zone::Float),
    ),
    WindowMenuEntry::Divider,
    item(
        ACTION_LEFT,
        "Left",
        kp(KeyCode::KP_4),
        WindowAction::Zone(Zone::Left),
    ),
    item(
        ACTION_RIGHT,
        "Right",
        kp(KeyCode::KP_6),
        WindowAction::Zone(Zone::Right),
    ),
    item(
        ACTION_TOP,
        "Top",
        kp(KeyCode::KP_EQUAL),
        WindowAction::Zone(Zone::Top),
    ),
    item(
        ACTION_BOTTOM,
        "Bottom",
        kp(KeyCode::KP_DECIMAL),
        WindowAction::Zone(Zone::Bottom),
    ),
    WindowMenuEntry::Divider,
    item(
        ACTION_TOP_MIDDLE,
        "Top Middle",
        kp(KeyCode::KP_8),
        WindowAction::Zone(Zone::TopMiddle),
    ),
    item(
        ACTION_FULL_MIDDLE,
        "Full Middle",
        kp(KeyCode::KP_5),
        WindowAction::Zone(Zone::FullMiddle),
    ),
    item(
        ACTION_BOTTOM_MIDDLE,
        "Bottom Middle",
        kp(KeyCode::KP_2),
        WindowAction::Zone(Zone::BottomMiddle),
    ),
    item(
        ACTION_MIDDLE_RIGHT,
        "Middle Right",
        kp(KeyCode::KP_ADD),
        WindowAction::Zone(Zone::MiddleRight),
    ),
    WindowMenuEntry::Divider,
    item(
        ACTION_FULLSCREEN,
        "Fullscreen",
        kp(KeyCode::KP_0),
        WindowAction::Zone(Zone::Fullscreen),
    ),
    item(
        ACTION_CINEMA,
        "Cinema",
        kp(KeyCode::KP_ENTER),
        WindowAction::Zone(Zone::Cinema),
    ),
];

const fn kp(keycode: KeyCode) -> KeyChord {
    KeyChord {
        keycode,
        meta: true,
        alt: false,
        ctrl: false,
        shift: false,
    }
}

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

    fn zone_action_id(zone: Zone) -> &'static str {
        // Exhaustive: a new Zone variant fails to compile until the Window
        // menu names it.
        match zone {
            Zone::Left => ACTION_LEFT,
            Zone::Right => ACTION_RIGHT,
            Zone::Top => ACTION_TOP,
            Zone::Bottom => ACTION_BOTTOM,
            Zone::TopMiddle => ACTION_TOP_MIDDLE,
            Zone::BottomMiddle => ACTION_BOTTOM_MIDDLE,
            Zone::FullMiddle => ACTION_FULL_MIDDLE,
            Zone::MiddleRight => ACTION_MIDDLE_RIGHT,
            Zone::Fullscreen => ACTION_FULLSCREEN,
            Zone::Cinema => ACTION_CINEMA,
            Zone::Float => ACTION_FLOAT,
        }
    }

    #[test]
    fn window_menu_covers_every_zone() {
        for zone in [
            Zone::Left,
            Zone::Right,
            Zone::Top,
            Zone::Bottom,
            Zone::TopMiddle,
            Zone::BottomMiddle,
            Zone::FullMiddle,
            Zone::MiddleRight,
            Zone::Fullscreen,
            Zone::Cinema,
            Zone::Float,
        ] {
            let id = zone_action_id(zone);
            assert_eq!(parse_window_action(id), Some(WindowAction::Zone(zone)));
        }
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
    fn window_menu_float_is_super_kp_multiply() {
        let chord = WINDOW_MENU_ENTRIES.iter().find_map(|e| match e {
            WindowMenuEntry::Item(i) if i.id == ACTION_FLOAT => Some(i.shortcut),
            _ => None,
        });
        let chord = chord.expect("float item");
        assert!(chord.meta);
        assert_eq!(chord.keycode, KeyCode::KP_MULTIPLY);
    }
}
