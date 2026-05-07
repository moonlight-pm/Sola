use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::KeyCode;

use crate::TerminalApp;
use sola_app::SolaApp;

/// Build the terminal app menu reflecting the actual tab count.
/// Tabs 1-9 get Cmd+N shortcuts; tabs 10+ have no shortcut.
pub fn terminal_menu(tab_count: usize) -> AppMenuPayload {
    AppMenuPayload {
        app_id: TerminalApp::APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Terminal".into(),
                items: vec![
                    MenuItem::Action {
                        id: "about".into(),
                        label: "About Terminal".into(),
                        shortcut: None,
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                    MenuItem::Action {
                        id: "quit".into(),
                        label: "Quit Terminal".into(),
                        shortcut: Some(KeyCode::Q.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
            MenuDefinition {
                label: "Shell".into(),
                items: vec![
                    MenuItem::Action {
                        id: "new_tab".into(),
                        label: "New Tab".into(),
                        shortcut: Some(KeyCode::T.meta()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Action {
                        id: "close_tab".into(),
                        label: "Close Tab".into(),
                        shortcut: Some(KeyCode::W.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
            MenuDefinition {
                label: "Edit".into(),
                items: vec![
                    MenuItem::Action {
                        id: "copy".into(),
                        label: "Copy".into(),
                        shortcut: Some(KeyCode::C.meta()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Action {
                        id: "paste".into(),
                        label: "Paste".into(),
                        shortcut: Some(KeyCode::V.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
            MenuDefinition {
                label: "Tabs".into(),
                items: (0..tab_count).map(tab_item).collect(),
            },
        ],
    }
}

fn tab_item(index: usize) -> MenuItem {
    MenuItem::Action {
        id: format!("select_tab_{index}"),
        label: format!("Tab {}", index + 1),
        shortcut: tab_shortcut(index),
        disabled: false,
        checked: false,
    }
}

fn tab_shortcut(index: usize) -> Option<sola_core::KeyChord> {
    let key = match index {
        0 => KeyCode::KEY_1,
        1 => KeyCode::KEY_2,
        2 => KeyCode::KEY_3,
        3 => KeyCode::KEY_4,
        4 => KeyCode::KEY_5,
        5 => KeyCode::KEY_6,
        6 => KeyCode::KEY_7,
        7 => KeyCode::KEY_8,
        8 => KeyCode::KEY_9,
        _ => return None,
    };
    Some(key.meta())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tab_items(payload: &AppMenuPayload) -> usize {
        payload
            .menus
            .iter()
            .find(|m| m.label == "Tabs")
            .map(|m| m.items.len())
            .unwrap_or(0)
    }

    #[test]
    fn empty_menu_has_no_tab_items() {
        assert_eq!(count_tab_items(&terminal_menu(0)), 0);
    }

    #[test]
    fn single_tab_menu_has_one_item() {
        let menu = terminal_menu(1);
        assert_eq!(count_tab_items(&menu), 1);
    }

    #[test]
    fn nine_tabs_get_shortcuts() {
        let menu = terminal_menu(9);
        let tabs = menu.menus.iter().find(|m| m.label == "Tabs").unwrap();
        for item in &tabs.items {
            if let MenuItem::Action { shortcut, .. } = item {
                assert!(shortcut.is_some(), "tabs 1-9 should have shortcuts");
            } else {
                panic!("expected Action items only");
            }
        }
    }

    #[test]
    fn tenth_tab_has_no_shortcut() {
        let menu = terminal_menu(12);
        let tabs = menu.menus.iter().find(|m| m.label == "Tabs").unwrap();
        // First 9 should have shortcuts; 10-12 should not.
        for (i, item) in tabs.items.iter().enumerate() {
            let MenuItem::Action { shortcut, .. } = item else {
                panic!("expected Action");
            };
            if i < 9 {
                assert!(shortcut.is_some(), "tab {} expected shortcut", i + 1);
            } else {
                assert!(shortcut.is_none(), "tab {} should have no shortcut", i + 1);
            }
        }
    }
}
