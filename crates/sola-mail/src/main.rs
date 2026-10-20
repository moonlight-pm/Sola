//! sola-mail — kit-native IMAP/SMTP client for Sola.

mod bridge;
mod protocol;
mod snapshot;
mod ui;
mod worker;

use sola_bus::topics::{MenuDefinition, MenuItem, TopicKind};
use sola_core::{KeyChord, KeyCode};
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;

use crate::ui::App;

const APP_ID: &str = "sola-mail";

fn main() -> iced::Result {
    // Rustls crypto provider (required before any TLS).
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    startup(APP_ID);
    // After logging init, so it chains to the log hook.
    protocol::imap::install_panic_filter();

    bridge::init_channels();
    worker::start();

    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::Windows,
            TopicKind::WindowFloating,
            TopicKind::MailConfig,
        ])
        .app_menu_definition(menu(
            "Mail",
            vec![
                item("refresh", "Get New Mail", Some(KeyCode::N.meta_shift())),
                MenuItem::Divider,
                item("quit", "Quit Mail", Some(KeyCode::Q.meta())),
            ],
        ))
        .app_menu_definition(menu(
            "Edit",
            vec![
                item("cut", "Cut", Some(KeyCode::X.meta())),
                item("copy", "Copy", Some(KeyCode::C.meta())),
                item("paste", "Paste", Some(KeyCode::V.meta())),
                MenuItem::Divider,
                item("select_all", "Select All", Some(KeyCode::A.meta())),
                item("copy_message", "Copy Message", None),
            ],
        ))
        .app_menu_definition(menu(
            "Mailbox",
            vec![
                item("empty_junk", "Erase Junk Mail…", None),
                item("empty_trash", "Erase Deleted Items…", None),
            ],
        ))
        .app_menu_definition(menu(
            "Message",
            vec![
                item("compose", "New Message", Some(KeyCode::N.meta())),
                item("save_draft", "Save Draft", Some(KeyCode::S.meta())),
                item("attach", "Attach Files…", Some(KeyCode::A.meta_shift())),
                MenuItem::Divider,
                item("reply", "Reply", Some(KeyCode::R.meta())),
                item("reply_all", "Reply All", Some(KeyCode::R.meta_shift())),
                item("forward", "Forward", Some(KeyCode::F.meta_shift())),
                MenuItem::Divider,
                item("toggle_seen", "Mark as Read / Unread", Some(KeyCode::U.meta_shift())),
                item("toggle_flag", "Flag / Unflag", Some(KeyCode::L.meta_shift())),
                MenuItem::Divider,
                item("archive", "Archive", Some(KeyCode::A.chord())),
                item("inbox", "Move to Inbox", Some(KeyCode::I.chord())),
                item("junk", "Move to Junk", Some(KeyCode::X.chord())),
                item("trash", "Delete", Some(KeyCode::D.chord())),
                MenuItem::Divider,
                item("undo", "Undo Move", Some(KeyCode::U.chord())),
            ],
        ))
        .app_menu_definition(menu(
            "View",
            vec![
                item("next", "Next Message", Some(KeyCode::S.chord())),
                item("prev", "Previous Message", Some(KeyCode::W.chord())),
            ],
        ))
        .install();

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID))
        .run()
}

fn menu(label: &str, items: Vec<MenuItem>) -> MenuDefinition {
    MenuDefinition {
        label: label.into(),
        items,
    }
}

fn item(id: &str, label: &str, shortcut: Option<KeyChord>) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    }
}
