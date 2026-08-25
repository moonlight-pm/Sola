use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::KeyCode;

use sola_terminal::state::TabView;

/// Build the terminal app menu reflecting the open tabs. Each tab is
/// labelled by its cwd (the same label the sidebar shows), not "Tab N".
/// Tabs 1-9 get Cmd+N shortcuts; tabs 10+ have no shortcut.
pub fn terminal_menu(tabs: &[TabView]) -> AppMenuPayload {
    AppMenuPayload {
        app_id: crate::APP_ID.into(),
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
                items: vec![MenuItem::Action {
                    id: "new_tab".into(),
                    label: "New Tab".into(),
                    shortcut: Some(KeyCode::T.meta()),
                    disabled: false,
                    checked: false,
                }],
            },
            MenuDefinition {
                label: "Pane".into(),
                items: vec![
                    // ⌘⇧→ : new pane to the RIGHT (side-by-side).
                    MenuItem::Action {
                        id: "split_vertical".into(),
                        label: "Split Vertical".into(),
                        shortcut: Some(KeyCode::RIGHT.meta_shift()),
                        disabled: false,
                        checked: false,
                    },
                    // ⌘⇧↓ : new pane BELOW (stacked).
                    MenuItem::Action {
                        id: "split_horizontal".into(),
                        label: "Split Horizontal".into(),
                        shortcut: Some(KeyCode::DOWN.meta_shift()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                    // ⌘⇧W : kill the active pane (⌘W is intentionally
                    // unbound — it means "Copy" in muscle memory).
                    MenuItem::Action {
                        id: "close_pane".into(),
                        label: "Close Pane".into(),
                        shortcut: Some(KeyCode::W.meta_shift()),
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
                items: tabs
                    .iter()
                    .enumerate()
                    .map(|(i, tab)| tab_item(i, tab))
                    .collect(),
            },
        ],
    }
}

fn tab_item(index: usize, tab: &TabView) -> MenuItem {
    MenuItem::Action {
        id: format!("select_tab_{index}"),
        // Label by what the tab actually shows (cwd basename, "shell" when
        // unknown) — the same label the sidebar uses — instead of "Tab N".
        // The 1-based position still shows on the right as the ⌘N shortcut.
        label: crate::sidebar::tab_label(&tab.cwd),
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

    /// N tab views with no cwd (label → "shell") for count/shortcut tests.
    fn tabs(n: usize) -> Vec<TabView> {
        (0..n)
            .map(|i| TabView {
                id: format!("t{i}"),
                cwd: None,
                ordinal: i as u32,
            })
            .collect()
    }

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
        assert_eq!(count_tab_items(&terminal_menu(&tabs(0))), 0);
    }

    #[test]
    fn single_tab_menu_has_one_item() {
        let menu = terminal_menu(&tabs(1));
        assert_eq!(count_tab_items(&menu), 1);
    }

    #[test]
    fn nine_tabs_get_shortcuts() {
        let menu = terminal_menu(&tabs(9));
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
        let menu = terminal_menu(&tabs(12));
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

    #[test]
    fn pane_menu_has_split_and_close_with_meta_shift() {
        let menu = terminal_menu(&tabs(0));
        let pane = menu
            .menus
            .iter()
            .find(|m| m.label == "Pane")
            .expect("Pane menu");
        let ids: Vec<&str> = pane
            .items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action { id, .. } => Some(id.as_str()),
                MenuItem::Divider => None,
            })
            .collect();
        assert_eq!(
            ids,
            vec!["split_vertical", "split_horizontal", "close_pane"]
        );
        for item in &pane.items {
            if let MenuItem::Action {
                shortcut: Some(sc), ..
            } = item
            {
                assert!(sc.meta && sc.shift, "pane shortcuts are meta+shift");
            }
        }
    }

    #[test]
    fn no_close_tab_action_remains() {
        let menu = terminal_menu(&tabs(3));
        let has_close_tab = menu.menus.iter().any(|m| {
            m.items
                .iter()
                .any(|i| matches!(i, MenuItem::Action { id, .. } if id == "close_tab"))
        });
        assert!(!has_close_tab, "close_tab should be gone; ⌘W is unbound");
    }
}
