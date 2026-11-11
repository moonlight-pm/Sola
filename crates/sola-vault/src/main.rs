//! sola-vault — Vault, Sola's Bitwarden client. A kit app on the vault
//! daemon (`sola-vaultd`): it holds no keys and reads values only over the
//! daemon's own socket (`docs/specs/2026-10-31-sola-vault-design.md`).
//!
//! `sola-vault --item <id>` opens on that item (the daemon's call-plane
//! `vault.open` launches it so); a second launch hands its item to the
//! window already open.

mod daemon;
mod instance;
mod menus;
mod model;
mod ui;

use sola_bus::topics::TopicKind;
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;

use crate::model::sort::Sort;
use crate::ui::App;

const APP_ID: &str = "sola-vault";

fn main() -> iced::Result {
    startup(APP_ID);
    let item = instance::item_arg();
    if instance::claim() == instance::Claim::Handoff {
        if let Err(e) = instance::handoff(item.as_deref()) {
            tracing::warn!(error = %e, "handoff to the open vault failed");
        }
        return Ok(());
    }

    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::Windows,
            TopicKind::WindowPlace,
        ])
        .install();
    sola_kit::menu::publish(menus::app_menu(APP_ID, Sort::default()));

    iced::application(move || App::boot(item.clone()), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID))
        .run()
}
