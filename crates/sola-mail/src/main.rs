//! sola-mail — kit-native IMAP/SMTP client for Sola.

mod ui;

use sola_bus::topics::{MenuDefinition, MenuItem, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;
use sola_mail_core::{bridge, worker};

use crate::ui::App;

const APP_ID: &str = "sola-mail";

fn main() -> iced::Result {
    // Rustls crypto provider (required before any TLS).
    sola_mail_core::install_crypto();

    startup(APP_ID);

    bridge::init_channels();
    worker::start();

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu_definition(MenuDefinition {
            label: "Mail".into(),
            items: vec![
                item("refresh", "Get New Mail", Some(KeyCode::N.meta_shift())),
                MenuItem::Divider,
                MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Mail".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                },
            ],
        })
        .app_menu_definition(MenuDefinition {
            label: "Edit".into(),
            items: vec![
                item("cut", "Cut", Some(KeyCode::X.meta())),
                item("copy", "Copy", Some(KeyCode::C.meta())),
                item("paste", "Paste", Some(KeyCode::V.meta())),
                MenuItem::Divider,
                item("select_all", "Select All", Some(KeyCode::A.meta())),
                item("copy_message", "Copy Message", None),
            ],
        })
        .app_menu_definition(MenuDefinition {
            label: "Mailbox".into(),
            items: vec![
                item("empty_junk", "Erase Junk Mail", None),
                item("empty_trash", "Erase Deleted Items", None),
            ],
        })
        .app_menu_definition(MenuDefinition {
            label: "Message".into(),
            items: vec![
                item("compose", "New Message", Some(KeyCode::N.meta())),
                MenuItem::Divider,
                item("reply", "Reply", Some(KeyCode::R.meta())),
                item("reply_all", "Reply All", Some(KeyCode::R.meta_shift())),
                MenuItem::Divider,
                item("archive", "Archive", Some(KeyCode::A.chord())),
                item("inbox", "Move to Inbox", Some(KeyCode::I.chord())),
                item("junk", "Move to Junk", Some(KeyCode::J.chord())),
                item("trash", "Delete", Some(KeyCode::D.chord())),
                MenuItem::Divider,
                item("undo", "Undo Move", Some(KeyCode::U.chord())),
            ],
        })
        .app_menu_definition(MenuDefinition {
            label: "View".into(),
            items: vec![
                item("next", "Next Message", Some(KeyCode::S.chord())),
                item("prev", "Previous Message", Some(KeyCode::W.chord())),
            ],
        })
        .install();

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID))
        .run()
}

fn item(id: &str, label: &str, shortcut: Option<sola_core::KeyChord>) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    }
}
