use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_core::KeyCode;

use crate::MailApp;
use sola_app::SolaApp;

pub fn mail_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: MailApp::APP_ID.into(),
        menus: vec![MenuDefinition {
            label: "Mail".into(),
            items: vec![MenuItem::Action {
                id: "quit".into(),
                label: "Quit Mail".into(),
                shortcut: Some(KeyCode::Q.meta()),
                disabled: false,
                checked: false,
            }],
        }],
    }
}
