use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::KeyCode;

/// The terminal's menus. Its tabs are windows in the shell's tab bar, so
/// there is no tab list here; New Tab opens one.
pub fn terminal_menu() -> AppMenuPayload {
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
                    sola_kit::menu::close_window_menu_item(),
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
                    // ⌘⇧W : kill the active pane (⌘W closes the window).
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
            sola_kit::window_menu(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_w_closes_the_window_and_super_shift_w_the_pane() {
        let items: Vec<_> = terminal_menu().menus.into_iter().flat_map(|m| m.items).collect();
        let key_of = |want: &str| {
            items.iter().find_map(|i| match i {
                MenuItem::Action { id, shortcut, .. } if id == want => *shortcut,
                _ => None,
            })
        };
        assert_eq!(key_of(sola_kit::menu::ACTION_CLOSE_WINDOW), Some(KeyCode::W.meta()));
        assert_eq!(key_of("close_pane"), Some(KeyCode::W.meta_shift()));
    }

    #[test]
    fn no_tab_list_and_new_tab_is_cmd_t() {
        let menu = terminal_menu();
        assert!(!menu.menus.iter().any(|m| m.label == "Tabs"));
        let new_tab = menu
            .menus
            .iter()
            .flat_map(|m| m.items.iter())
            .find(|item| matches!(item, MenuItem::Action { id, .. } if id == "new_tab"));
        assert!(matches!(
            new_tab,
            Some(MenuItem::Action { shortcut: Some(c), .. }) if *c == KeyCode::T.meta()
        ));
    }
}
