//! The desk menu (`docs/specs/2026-10-31-sola-vault-design.md`): the
//! right-click menu the shell draws for every kit app.
//!
//! An app asks for it at a right-click with [`open`]: what the click
//! landed on (a [`DeskMenuContext`], never the text), its own rows, and
//! which Edit rows apply. The shell adds Edit and, in an editable place,
//! the vault's rows, and answers with a `Topic::DeskMenuPick` for this
//! process ([`pick_of`]): one of the app's rows, an Edit row, a vault
//! value to [`redeem`] and insert, or nothing.
//!
//! A kit text input opens and answers its own menu ([`field`]), so every
//! kit app's fields have it without app code; an app adds rows only for
//! its own surfaces (a page, a terminal).

pub(crate) mod field;

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iced::{Point, Task};
use sola_bus::Message;
use sola_bus::topics::{DeskMenuOpen, Topic};
use sola_vault_proto::{Reply, Request};
use zeroize::Zeroizing;

pub use sola_bus::topics::{DeskEdit, DeskMenuContext, DeskMenuItem, DeskMenuPick, DeskPick};
pub use sola_vault_proto::{ErrorCode as VaultErrorCode, VaultError};

/// The shell's app id: it draws the menu, so its own fields ask for none
/// (the bus never hands an app its own event back).
const SHELL_APP_ID: &str = "sola-shell";

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// A key for one menu, unique in this process; it comes back in the pick.
pub fn next_token() -> u64 {
    NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
}

/// Ask the shell for the desk menu at `(x, y)` in the clicked window's
/// logical pixels ([`pointer`] has them for a widget that cannot know).
/// Nothing happens before [`BusSetup::install`](crate::app::BusSetup::install)
/// or in the shell itself.
pub fn open(
    token: u64,
    x: f32,
    y: f32,
    context: DeskMenuContext,
    items: Vec<DeskMenuItem>,
    edit: Vec<DeskEdit>,
) {
    let Some(app_id) = crate::app::app_id().filter(|id| *id != SHELL_APP_ID) else {
        return;
    };
    let Some(bus) = crate::app::try_bus() else {
        return;
    };
    let open = DeskMenuOpen {
        app_id: app_id.to_string(),
        pid: std::process::id(),
        token,
        x: x.round() as i32,
        y: y.round() as i32,
        context,
        items,
        edit,
    };
    let mut bus = bus.lock().unwrap_or_else(|p| p.into_inner());
    if let Err(e) = bus.emit(Topic::DeskMenuOpen(open)) {
        tracing::warn!(token, "desk menu: asking the shell failed: {e}");
    }
}

/// An app's row: `id` comes back in [`DeskPick::Action`]. Its key hint is
/// the app's menu item with the same id, as the titlebar shows it.
pub fn item(id: &str, label: &str, enabled: bool) -> DeskMenuItem {
    DeskMenuItem::Action {
        id: id.to_string(),
        label: label.to_string(),
        shortcut: menu_key(id),
        enabled,
    }
}

/// The key the app's menus give `id` now.
fn menu_key(id: &str) -> Option<sola_core::KeyChord> {
    use sola_bus::topics::MenuItem;
    crate::menu::current()?
        .menus
        .iter()
        .flat_map(|m| m.items.iter())
        .find_map(|i| match i {
            MenuItem::Action {
                id: item, shortcut, ..
            } if item == id => *shortcut,
            _ => None,
        })
}

/// The desk menu's answer, when `message` is one for this process.
pub fn pick_of(message: &Message) -> Option<DeskMenuPick> {
    if message.topic != "DeskMenuPick" {
        return None;
    }
    match Topic::parse(message) {
        Some(Topic::DeskMenuPick(p)) if p.pid == std::process::id() => Some(p),
        _ => None,
    }
}

/// A vault value: cleared from memory when dropped, and never printed.
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// Fetch the value a [`DeskPick::VaultInsert`] ticket stands for, once,
/// from the vault's socket (only this process can). Blocking: off the iced
/// thread, or [`redeem_task`].
pub fn redeem(ticket: &str) -> Result<Secret, VaultError> {
    let name = crate::app::app_id().unwrap_or("sola-kit");
    match sola_vault_proto::call(
        name,
        Request::Redeem {
            ticket: ticket.to_string(),
        },
    )? {
        Reply::Value(v) => Ok(Secret(Zeroizing::new(v))),
        _ => Err(VaultError::new(
            sola_vault_proto::ErrorCode::Internal,
            "The vault answered a redeem with something else.",
        )),
    }
}

/// [`redeem`] on a thread of its own.
pub fn redeem_task(ticket: String) -> Task<Result<Secret, VaultError>> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(redeem(&ticket));
            });
            rx.await.unwrap_or_else(|_| {
                Err(VaultError::new(
                    sola_vault_proto::ErrorCode::Internal,
                    "The redeem thread went away.",
                ))
            })
        },
        |r| r,
    )
}

/// Copy for a desk menu's row: from the compositor's side, since the pick
/// can come before the window has the keyboard back; the window's own
/// clipboard if that fails.
pub fn copy<T>(text: String) -> Task<T> {
    match crate::clipboard::write_text(text.clone()) {
        Ok(()) => Task::none(),
        Err(e) => {
            tracing::debug!(%e, "desk menu: data-control copy failed; using the window's clipboard");
            iced::clipboard::write(text)
        }
    }
}

/// The clipboard's text for a desk menu's Paste, read from the
/// compositor's side on a thread of its own.
pub fn paste_task() -> Task<Option<String>> {
    Task::perform(
        async {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::clipboard::read_string());
            });
            rx.await.ok().flatten()
        },
        |text| text,
    )
}

/// How long a right press's window position stays good: the same event.
const POINTER_FRESH: Duration = Duration::from_millis(250);

thread_local! {
    /// Where the last right press landed in its window (the frame sees
    /// window coordinates; a field in a scrolled list does not).
    static POINTER: Cell<Option<(Point, Instant)>> = const { Cell::new(None) };
}

/// The kit's window frame saw a right press at `at` (window logical px).
pub(crate) fn note_pointer(at: Point) {
    POINTER.with(|p| p.set(Some((at, Instant::now()))));
}

/// Where the right press being handled landed in its window, when the
/// window's kit frame saw it. Taken: a later read gets `None`.
pub fn pointer() -> Option<Point> {
    POINTER
        .with(Cell::take)
        .filter(|(_, at)| at.elapsed() <= POINTER_FRESH)
        .map(|(p, _)| p)
}

/// The kit's bus poller shows every message here; a pick for one of the
/// kit's own fields is kept for that field ([`field::observe`]).
pub(crate) fn observe(message: &Message) {
    if let Some(pick) = pick_of(message) {
        field::observe(&pick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique() {
        let a = next_token();
        let b = next_token();
        assert_ne!(a, b);
        assert!(a > 0 && b > 0);
    }

    #[test]
    fn a_pointer_is_read_once() {
        note_pointer(Point::new(12.0, 34.0));
        assert_eq!(pointer(), Some(Point::new(12.0, 34.0)));
        assert_eq!(pointer(), None);
    }

    #[test]
    fn a_secret_never_prints() {
        let s = Secret(Zeroizing::new("hunter2".into()));
        assert_eq!(format!("{s:?}"), "Secret(..)");
        assert_eq!(s.as_str(), "hunter2");
    }

    #[test]
    fn picks_for_other_processes_are_not_ours() {
        let pick = |pid| {
            Topic::DeskMenuPick(DeskMenuPick {
                app_id: "sola-mail".into(),
                pid,
                token: 3,
                pick: DeskPick::Dismissed,
            })
            .to_message()
        };
        assert!(pick_of(&pick(std::process::id())).is_some());
        assert!(pick_of(&pick(std::process::id().wrapping_add(1))).is_none());
    }
}
