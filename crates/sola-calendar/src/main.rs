//! sola-calendar — kit-native calendar (local, Google, iCloud, CalDAV, ICS URL).

mod apple;
mod bridge;
mod google;
mod ics;
mod model;
mod paths;
mod store;
mod timeutil;
mod ui;
mod worker;

use sola_bus::topics::{MenuDefinition, MenuItem, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;

use crate::ui::App;

const APP_ID: &str = "sola-calendar";

fn main() -> iced::Result {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    startup(APP_ID);
    bridge::init_channels();
    worker::start();

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu_definition(MenuDefinition {
            label: "Calendar".into(),
            items: vec![
                item("new_event", "New Event", Some(KeyCode::N.meta())),
                item("refresh", "Refresh", Some(KeyCode::R.meta())),
                MenuItem::Divider,
                MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Calendar".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                },
            ],
        })
        .app_menu_definition(MenuDefinition {
            label: "View".into(),
            items: vec![
                item("view_month", "Month", Some(KeyCode::KEY_1.chord())),
                item("view_week", "Week", Some(KeyCode::KEY_2.chord())),
                item("view_day", "Day", Some(KeyCode::KEY_3.chord())),
                MenuItem::Divider,
                item("today", "Go to Today", Some(KeyCode::T.chord())),
            ],
        })
        .window_menu()
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
