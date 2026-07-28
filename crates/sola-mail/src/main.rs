//! sola-mail — kit-native IMAP/SMTP client for Sola.

mod bridge;
mod protocol;
mod ui;
mod worker;

use sola_bus::topics::TopicKind;
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
        .app_menu("Mail", [("quit", "Quit Mail", KeyCode::Q.meta())])
        .install();

    iced::application(App::default, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings(APP_ID))
        .run()
}
