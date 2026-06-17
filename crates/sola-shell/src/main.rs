//! sola-shell — iced-native desktop shell. Four windows on one
//! iced multi-window application.

use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup};
use sola_kit::fonts::INTER;

mod app;
mod builtins;
pub mod calendar;
pub mod components;
pub mod keys;
pub mod launcher;
pub mod media;
pub mod menu;
pub mod menubar;
pub mod stats;
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
    let iced_daemon =
        iced::daemon(app::Shell::boot, app::Shell::update, app::Shell::view)
            .title(app::Shell::title)
            .subscription(app::Shell::subscription)
            .theme(app::Shell::theme)
            .default_font(INTER);
    iced_daemon.run()
}
