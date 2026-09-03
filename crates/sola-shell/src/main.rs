//! sola-shell — iced-native desktop shell. Multi-window daemon
//! (menubar, menu, launcher, switcher, shortcuts, selection marquee,
//! notifications).

use sola_bus::topics::TopicKind;
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
mod power;
mod screenshot;
pub mod selection;
pub mod shortcuts;
pub mod stats;
pub mod switcher;
pub mod zoning;

const APP_ID: &str = "sola-shell";

fn main() -> iced::Result {
    startup(APP_ID);

    // Flower / system menu (and the shell's own app menu when focused).
    // "Restart Shell" exits this process only — the process manager
    // respawns `/opt/sola/bin/sola-shell`. "Quit Sola" shuts the whole
    // session down via `Topic::Shutdown`. "Restart Computer" / "Shut Down"
    // ask logind to reboot or power off. "Launch Application…" opens
    // the launcher overlay.
    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu_definition(menu::state::system_menu())
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
