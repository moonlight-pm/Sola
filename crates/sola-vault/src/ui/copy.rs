//! Copying a value: onto the clipboard, the row says "Copied", and after
//! the clipboard time it leaves the clipboard again — if nothing was
//! copied over it since. Re-prompt items copy only once verified.

use std::future::Future;
use std::time::Instant;

use iced::Task;
use iced::futures::channel::oneshot;
use sola_kit::clipboard::read_string;
use wl_clipboard_rs::copy::{ClipboardType, Seat, clear};
use zeroize::Zeroize;

use super::{App, COPIED_FOR, Msg, Pane};
use crate::model::clip::ClipGuard;
use crate::model::code::CodeNow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyWhat {
    /// A field of the record by its key (`username`, `password`, `uri0`,
    /// `custom2`, …).
    Field(String),
    Code,
    Notes,
    /// A past password, by its place in the history.
    History(usize),
    /// What the generator made.
    Generated,
    /// An unreadable authenticator key, as stored.
    TotpKey,
}

impl CopyWhat {
    /// The key the row's "Copied" shows under.
    pub fn key(&self) -> String {
        match self {
            CopyWhat::Field(k) => k.clone(),
            CopyWhat::Code => "code".into(),
            CopyWhat::Notes => "notes".into(),
            CopyWhat::History(i) => format!("history:{i}"),
            CopyWhat::Generated => "generated".into(),
            CopyWhat::TotpKey => "totp".into(),
        }
    }
}

impl App {
    pub(super) fn copy(&mut self, what: CopyWhat) -> Task<Msg> {
        let value = match &what {
            CopyWhat::Generated => self.generated_value(),
            _ => {
                let Pane::Open(record) = &self.record else {
                    return Task::none();
                };
                match &what {
                    CopyWhat::Field(key) => record.field(key).map(str::to_string),
                    CopyWhat::Code => CodeNow::of(record, self.now).map(|c| c.code),
                    CopyWhat::Notes => record.notes.clone(),
                    CopyWhat::History(i) => {
                        record.password_history.get(*i).map(|h| h.password.clone())
                    }
                    CopyWhat::TotpKey => record.totp_secret.clone(),
                    CopyWhat::Generated => None,
                }
            }
        };
        match value.filter(|v| !v.is_empty()) {
            Some(v) => self.copy_value(what.key(), v),
            None => Task::none(),
        }
    }

    pub(super) fn copy_value(&mut self, key: String, value: String) -> Task<Msg> {
        let now = Instant::now();
        self.clip = ClipGuard::new(&value, self.settings.clipboard_clear_secs, now);
        self.copied = Some((key, now + COPIED_FOR));
        iced::clipboard::write(value)
    }

    /// The row with `key` says "Copied" now.
    pub(super) fn just_copied(&self, key: &str) -> bool {
        self.copied.as_ref().is_some_and(|(k, _)| k == key)
    }
}

/// Clear the clipboard if it still holds what `guard` copied. Through
/// data-control, so it works while another window has the focus.
pub fn clear_if_ours(guard: ClipGuard) -> impl Future<Output = ()> + Send + 'static {
    let (tx, rx) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("vault-clipboard".into())
        .spawn(move || {
            let ours = match read_string() {
                Some(mut s) => {
                    let ours = guard.holds(&s);
                    s.zeroize();
                    ours
                }
                None => false,
            };
            if ours {
                match clear(ClipboardType::Regular, Seat::All) {
                    Ok(()) => tracing::info!("cleared a copied value from the clipboard"),
                    Err(e) => tracing::warn!(error = %e, "could not clear the clipboard"),
                }
            }
            let _ = tx.send(());
        })
        .is_ok();
    async move {
        if spawned {
            let _ = rx.await;
        }
    }
}
