//! sola-mail — kit-native IMAP/SMTP client for Sola.

mod bridge;
mod protocol;
mod ui;
mod worker;

use sola_bus::topics::{MenuDefinition, MenuItem, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings};
use sola_kit::fonts;

use crate::ui::App;

const APP_ID: &str = "sola-mail";

fn main() -> iced::Result {
    // Rustls crypto provider (required before any TLS).
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    startup(APP_ID);

    bridge::init_channels();
    worker::start();

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu_definition(MenuDefinition {
            label: "Mail".into(),
            items: vec![MenuItem::Action {
                id: "quit".into(),
                label: "Quit Mail".into(),
                shortcut: Some(KeyCode::Q.meta()),
                disabled: false,
                checked: false,
            }],
        })
        // Edit menu — shell routes chords (⌘C/⌘A/…) here as MenuAction.
        .app_menu_definition(MenuDefinition {
            label: "Edit".into(),
            items: vec![
                MenuItem::Action {
                    id: "cut".into(),
                    label: "Cut".into(),
                    shortcut: Some(KeyCode::X.meta()),
                    disabled: false,
                    checked: false,
                },
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
                MenuItem::Divider,
                MenuItem::Action {
                    id: "select_all".into(),
                    label: "Select All".into(),
                    shortcut: Some(KeyCode::A.meta()),
                    disabled: false,
                    checked: false,
                },
            ],
        })
        .install();

    iced::application(App::default, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings(APP_ID))
        .run()
}
