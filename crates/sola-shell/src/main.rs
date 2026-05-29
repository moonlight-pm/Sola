//! sola-shell — iced-native desktop shell. Four windows on one
//! iced multi-window application.

use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup};
use sola_kit::fonts::{self, INTER};

mod app;
mod builtins;
pub mod components;
pub mod keys;
pub mod launcher;
pub mod menu;
pub mod menubar;
pub mod switcher;
pub mod zoning;

const APP_ID: &str = "sola-shell";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Shell", [("quit", "Quit Shell", KeyCode::Q.meta())])
        .install();

    // Use iced::daemon so we can open multiple windows and dispatch view()
    // per window::Id.  The daemon opens no default window; our boot task
    // opens the menubar immediately.
    let mut iced_daemon =
        iced::daemon(app::Shell::boot, app::Shell::update, app::Shell::view)
            .title(app::Shell::title)
            .subscription(app::Shell::subscription)
            .theme(app::Shell::theme)
            .default_font(INTER);
    for bytes in fonts::load_all() {
        iced_daemon = iced_daemon.font(bytes);
    }
    iced_daemon.run()
}
