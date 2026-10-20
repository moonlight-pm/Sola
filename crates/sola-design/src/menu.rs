//! App menu. Shortcuts arrive as `MenuAction` from the shell, so they work
//! while a page has the keyboard.

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::{KeyChord, KeyCode};

use crate::APP_ID;

pub const QUIT: &str = sola_kit::app::QUIT_ACTION_ID;
pub const ADD_PROJECT: &str = "add-project";
pub const NEW_CANVAS: &str = "new-canvas";
pub const DROP_PROJECT: &str = "drop-project";
pub const FIT: &str = "fit";
pub const ACTUAL_SIZE: &str = "actual-size";
pub const ZOOM_IN: &str = "zoom-in";
pub const ZOOM_OUT: &str = "zoom-out";
pub const RELOAD: &str = "reload";
pub const SHARE_COPY: &str = "share-copy";
pub const SHARE_ROTATE: &str = "share-rotate";

pub fn app_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Design".into(),
                items: vec![action(QUIT, "Quit Design", Some(KeyCode::Q.meta()))],
            },
            MenuDefinition {
                label: "Project".into(),
                items: vec![
                    action(ADD_PROJECT, "Add Project…", Some(KeyCode::N.meta())),
                    action(NEW_CANVAS, "New Canvas…", Some(KeyCode::N.meta_shift())),
                    MenuItem::Divider,
                    action(DROP_PROJECT, "Drop Project", None),
                ],
            },
            MenuDefinition {
                label: "Share".into(),
                items: vec![
                    action(SHARE_COPY, "Copy Link", Some(KeyCode::L.meta_shift())),
                    action(SHARE_ROTATE, "New Link (old one stops working)", None),
                ],
            },
            MenuDefinition {
                label: "View".into(),
                items: vec![
                    action(FIT, "Fit All", Some(KeyCode::KEY_0.meta())),
                    action(ACTUAL_SIZE, "Actual Size", Some(KeyCode::KEY_1.meta())),
                    action(ZOOM_IN, "Zoom In", Some(KeyCode::EQUAL.meta())),
                    action(ZOOM_OUT, "Zoom Out", Some(KeyCode::MINUS.meta())),
                    MenuItem::Divider,
                    action(RELOAD, "Reload Canvas", Some(KeyCode::R.meta())),
                ],
            },
        ],
    }
}

fn action(id: &str, label: &str, shortcut: Option<KeyChord>) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    }
}
