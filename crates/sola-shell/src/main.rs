//! sola-shell — iced-native desktop shell. Multi-window daemon
//! (menubar, menu, launcher, switcher, selection marquee, notifications).

use sola_bus::topics::{MenuDefinition, MenuItem, TopicKind};
use sola_kit::app::{BusSetup, startup};
use sola_kit::fonts::INTER;

mod app;
pub mod audio;
pub mod bluetooth;
mod builtins;
pub mod calendar;
pub mod components;
pub mod keys;
pub mod launcher;
pub mod media;
pub mod menu;
pub mod menubar;
pub mod notify;
mod screenshot;
pub mod selection;
pub mod stats;
pub mod switcher;
pub mod zoning;

const APP_ID: &str = "sola-shell";

fn main() -> iced::Result {
    startup(APP_ID);

    // Flower / system menu (and the shell's own app menu when focused).
    // "Restart Shell" exits this process only — the process manager
    // respawns `/opt/sola/bin/sola-shell`. "Quit Sola" shuts the whole
    // session down via `Topic::Shutdown`. "Launch Application…" opens
    // the launcher overlay.
    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu_definition(MenuDefinition {
            label: "Shell".into(),
            items: vec![
                MenuItem::Action {
                    id: "launch".into(),
                    label: "Launch Application…".into(),
                    shortcut: None,
                    disabled: false,
                    checked: false,
                },
                MenuItem::Divider,
                MenuItem::Action {
                    id: "restart".into(),
                    label: "Restart Shell".into(),
                    shortcut: None,
                    disabled: false,
                    checked: false,
                },
                MenuItem::Divider,
                MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Sola".into(),
                    // Super+Q closes the focused app (`CloseApp`). Binding it
                    // here would advertise — and could fire — session shutdown.
                    shortcut: None,
                    disabled: false,
                    checked: false,
                },
            ],
        })
        .install();

    // Use iced::daemon so we can open multiple windows and dispatch view()
    // per window::Id.  The daemon opens no default window; our boot task
    // opens the menubar immediately.
    let iced_daemon = iced::daemon(app::Shell::boot, app::Shell::update, app::Shell::view)
        .title(app::Shell::title)
        .subscription(app::Shell::subscription)
        .theme(app::Shell::theme)
        .default_font(INTER);
    iced_daemon.run()
}
