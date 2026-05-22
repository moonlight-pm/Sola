//! sola-shell — iced-native desktop shell. Replaces the CEF/Remix v3
//! shell (preserved as `sola-shell-legacy`). Four windows on one
//! iced multi-window application.

use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings};
use sola_kit::fonts::{self, NORMAL as F_NORMAL};

mod app;

const APP_ID: &str = "sola-shell";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Shell", [("quit", "Quit Shell", KeyCode::Q.meta())])
        .install();

    let mut iced_app = iced::application(app::Shell::default, app::Shell::update, app::Shell::view)
        .title(app::Shell::title)
        .subscription(app::Shell::subscription)
        .theme(app::Shell::theme)
        .default_font(F_NORMAL)
        .window(window_settings(APP_ID));
    for bytes in fonts::load_all() {
        iced_app = iced_app.font(bytes);
    }
    iced_app.run()
}
