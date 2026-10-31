use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::KeyCode;

use crate::APP_ID;

/// Menu action id prefix for Project → Open: `open-project:<folder>`.
pub const OPEN_PREFIX: &str = "open-project:";

/// The app menu. `folders` lists `(path, name)` of each folder of
/// `~/Projects` not open, under Project → Open.
pub fn app_menu(folders: &[(String, String)]) -> AppMenuPayload {
    let mut project = vec![
        MenuItem::Action {
            id: "add-project".into(),
            label: "New Project…".into(),
            shortcut: Some(KeyCode::N.meta()),
            disabled: false,
            checked: false,
        },
        MenuItem::Divider,
        // This computer stops watching; the session keeps running.
        MenuItem::Action {
            id: "close-window".into(),
            label: "Close Window".into(),
            shortcut: Some(KeyCode::W.meta()),
            disabled: false,
            checked: false,
        },
        // Ends it for every computer: no shortcut, and a confirm while
        // its agent works or waits.
        MenuItem::Action {
            id: "end-session".into(),
            label: "End Session…".into(),
            shortcut: None,
            disabled: false,
            checked: false,
        },
    ];
    if !folders.is_empty() {
        project.push(MenuItem::Divider);
        for (path, name) in folders {
            project.push(MenuItem::Action {
                id: format!("{OPEN_PREFIX}{path}"),
                label: format!("Open {name}"),
                shortcut: None,
                disabled: false,
                checked: false,
            });
        }
    }
    AppMenuPayload {
        app_id: APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Projects".into(),
                items: vec![
                    MenuItem::Action {
                        id: "spawn-sibling".into(),
                        label: "New Session…".into(),
                        shortcut: Some(KeyCode::T.meta()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                    MenuItem::Action {
                        id: "about".into(),
                        label: "About Projects".into(),
                        shortcut: None,
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                    MenuItem::Action {
                        id: "quit".into(),
                        label: "Quit Projects".into(),
                        shortcut: Some(KeyCode::Q.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
            MenuDefinition {
                label: "Project".into(),
                items: project,
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
        ],
    }
}
