//! sola-spotify — kit-native Spotify client (library, Connect, local playback).
//!
//! Playback engine, Web API client, and PKCE flow are adapted from Fastpotify
//! (MIT), https://github.com/crmne/fastpotify. The interface is sola-kit.

mod api;
mod auth;
mod bridge;
mod cache;
mod images;
mod media;
mod mpris;
mod paths;
mod player;
mod settings;
mod ui;
mod worker;

use sola_bus::topics::{MenuDefinition, MenuItem, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;

use crate::ui::App;

const APP_ID: &str = "sola-spotify";

fn main() -> iced::Result {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    startup(APP_ID);
    bridge::init_channels();
    worker::start();

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu_definition(MenuDefinition {
            label: "Spotify".into(),
            items: vec![
                item("sign_in", "Sign In…", None),
                item("play_here", "Play Here", None),
                MenuItem::Divider,
                MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Spotify".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                },
            ],
        })
        .app_menu_definition(MenuDefinition {
            label: "Playback".into(),
            items: vec![
                item("play_pause", "Play/Pause", Some(KeyCode::SPACE.chord())),
                item("next", "Next", Some(KeyCode::RIGHT.meta())),
                item("prev", "Previous", Some(KeyCode::LEFT.meta())),
                MenuItem::Divider,
                item("shuffle", "Shuffle", Some(KeyCode::S.chord())),
                item("repeat", "Repeat", Some(KeyCode::R.chord())),
            ],
        })
        .app_menu_definition(MenuDefinition {
            label: "View".into(),
            items: vec![
                item("home", "Home", Some(KeyCode::H.meta())),
                item("search", "Search", Some(KeyCode::F.meta())),
                item("liked", "Liked Songs", Some(KeyCode::L.meta())),
                item("queue", "Queue", Some(KeyCode::U.meta())),
                MenuItem::Divider,
                item("settings", "Settings", None),
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
