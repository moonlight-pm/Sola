//! Bots iced app — talks to sola-botsd over HTTP + SSE (same as the phone).

use sola_bots::ui::{self, App};
use sola_bus::topics::{MenuDefinition, MenuItem, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;

fn main() -> iced::Result {
    startup(ui::APP_ID);
    BusSetup::new(ui::APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu_definition(MenuDefinition {
            label: "Bots".into(),
            items: vec![MenuItem::Action {
                id: "quit".into(),
                label: "Quit Bots".into(),
                shortcut: Some(KeyCode::Q.meta()),
                disabled: false,
                checked: false,
            }],
        })
        .app_menu_definition(MenuDefinition {
            label: "Edit".into(),
            items: vec![
                item("cut", "Cut", Some(KeyCode::X.meta())),
                item("copy", "Copy", Some(KeyCode::C.meta())),
                item("paste", "Paste", Some(KeyCode::V.meta())),
                MenuItem::Divider,
                item("select_all", "Select All", Some(KeyCode::A.meta())),
            ],
        })
        .window_menu()
        .install();

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(ui::APP_ID))
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
