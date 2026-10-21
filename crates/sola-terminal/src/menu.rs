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
            sola_kit::window_menu(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
