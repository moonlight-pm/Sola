use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::KeyCode;

use crate::APP_ID;

pub fn app_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: APP_ID.into(),
        menus: vec![MenuDefinition {
            label: "Workspaces".into(),
            items: vec![
                MenuItem::Action {
                    id: "about".into(),
                    label: "About Workspaces".into(),
                    shortcut: None,
                    disabled: false,
                    checked: false,
                },
                MenuItem::Divider,
                MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Workspaces".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                },
            ],
        }],
    }
}
