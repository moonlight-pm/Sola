//! App menu. Shortcuts arrive as `MenuAction` from the shell, so they work
//! while a page has the keyboard.

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::{KeyChord, KeyCode};

use crate::APP_ID;

pub const OPEN_PROJECT: &str = "open-project";
pub const CLOSE_WINDOW: &str = "close-window";
pub const FIT: &str = "fit";
pub const ACTUAL_SIZE: &str = "actual-size";
pub const ZOOM_IN: &str = "zoom-in";
pub const ZOOM_OUT: &str = "zoom-out";
pub const RELOAD: &str = "reload";
pub const SHARE_COPY: &str = "share-copy";
pub const SHARE_ROTATE: &str = "share-rotate";

/// The app menu. Design opens projects; it makes and drops none (agents
/// make them), so Project → Open Project… is all the Project menu has.
pub fn app_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Design".into(),
                // Only the window: Open Project… brings it back.
                items: vec![
                    action(CLOSE_WINDOW, "Close Window", Some(sola_kit::menu::CLOSE_WINDOW_KEY)),
                    sola_kit::menu::quit_menu_item("Quit Design"),
                ],
            },
            MenuDefinition {
                label: "Project".into(),
                items: vec![action(OPEN_PROJECT, "Open Project…", Some(KeyCode::O.meta()))],
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
                    // Super+digit is the workspaces'; Super+F and Super+A,
                    // Every App's Find and Select All.
                    action(FIT, "Fit All", Some(KeyCode::F.meta_shift())),
                    action(ACTUAL_SIZE, "Actual Size", Some(KeyCode::A.meta_shift())),
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
