//! The flower menu's Lock Vault, shown while `Topic::VaultState` says the
//! vault is unlocked: the shell, a full client of the vault's socket,
//! asks it to lock, off the iced thread. A failure is a toast.

use std::time::Duration;

use iced::Task;

use super::{Msg, Shell};
use crate::desk_menu::vault;

impl Shell {
    pub(crate) fn lock_vault(&mut self) -> Task<Msg> {
        let _ = self.dismiss_open_menu();
        self.emit_overlay_frames();
        self.emit_composition();
        self.emit_registered_chords();
        Task::perform(vault::off_thread(vault::lock), |result| {
            Msg::VaultLocked(result.unwrap_or_else(|| Err("The vault went away.".into())))
        })
    }

    pub(crate) fn on_vault_locked(&mut self, result: Result<(), String>) -> Task<Msg> {
        let Err(message) = result else {
            tracing::info!("vault locked from the flower menu");
            return Task::none();
        };
        tracing::warn!(error = %message, "flower menu: the vault did not lock");
        self.menubar.push_toast(format!("Vault: {message}"));
        let generation = self.menubar.toast_generation;
        Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
            Msg::ToastExpire(generation)
        })
    }
}
