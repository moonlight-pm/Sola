//! The desk menu of a kit text input: which menus its fields opened, and
//! the pick each waits for.
//!
//! The field opens the menu ([`opened`]); the kit's bus poller sees the
//! pick ([`observe`]) — redeeming a vault value or reading the clipboard
//! there, off the iced thread — and leaves it here; the field applies it on its next event ([`take`]),
//! which the app's update for the same bus message brings (every window
//! redraws after an update).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sola_bus::topics::{DeskEdit, DeskMenuPick, DeskPick};

use super::Secret;

/// What a field does with its pick.
#[derive(Debug)]
pub(crate) enum FieldOp {
    /// The clipboard's text, read here off the iced thread (`None`: the
    /// compositor would not say; the field asks iced's clipboard).
    Paste(Option<String>),
    Cut,
    Copy,
    SelectAll,
    /// A vault value, at the caret.
    Insert(Secret),
}

/// Menus fields opened and not yet answered. A field whose menu never
/// comes back (the shell restarted) is forgotten past this many.
const OPEN_MAX: usize = 16;

/// A pick no field took (the field went away) is dropped after this.
const PENDING_FOR: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Fields {
    open: VecDeque<u64>,
    pending: HashMap<u64, (FieldOp, Instant)>,
}

impl Fields {
    fn opened(&mut self, token: u64) {
        self.open.push_back(token);
        while self.open.len() > OPEN_MAX {
            self.open.pop_front();
        }
    }

    fn is_open(&self, token: u64) -> bool {
        self.open.contains(&token)
    }

    /// The menu `token` was answered: keep what its field must do.
    fn answered(&mut self, token: u64, op: Option<FieldOp>, now: Instant) {
        self.open.retain(|t| *t != token);
        self.pending
            .retain(|_, (_, at)| now.duration_since(*at) < PENDING_FOR);
        if let Some(op) = op {
            self.pending.insert(token, (op, now));
        }
    }

    fn take(&mut self, token: u64) -> Option<FieldOp> {
        self.pending.remove(&token).map(|(op, _)| op)
    }
}

static FIELDS: Mutex<Option<Fields>> = Mutex::new(None);

/// Something waits in [`FIELDS`]: lets every field skip the lock on every
/// event.
static PENDING: AtomicBool = AtomicBool::new(false);

fn with<R>(f: impl FnOnce(&mut Fields) -> R) -> R {
    let mut slot = FIELDS.lock().unwrap_or_else(|p| p.into_inner());
    let fields = slot.get_or_insert_with(Fields::default);
    let out = f(fields);
    PENDING.store(!fields.pending.is_empty(), Ordering::Release);
    out
}

/// A field opened the menu with `token`.
pub(crate) fn opened(token: u64) {
    with(|f| f.opened(token));
}

/// A pick for this process: kept for its field when a field opened it.
/// Returns whether it was a field's.
pub(crate) fn observe(pick: &DeskMenuPick) -> bool {
    let token = pick.token;
    if !with(|f| f.is_open(token)) {
        return false;
    }
    let op = match &pick.pick {
        DeskPick::Edit(DeskEdit::Paste) => Some(FieldOp::Paste(crate::clipboard::read_string())),
        DeskPick::Edit(DeskEdit::Cut) => Some(FieldOp::Cut),
        DeskPick::Edit(DeskEdit::Copy) => Some(FieldOp::Copy),
        DeskPick::Edit(DeskEdit::SelectAll) => Some(FieldOp::SelectAll),
        DeskPick::VaultInsert { ticket } => match super::redeem(ticket) {
            Ok(value) => Some(FieldOp::Insert(value)),
            Err(e) => {
                tracing::warn!(token, code = ?e.code, "desk menu: the vault value did not come");
                None
            }
        },
        // Fields add no rows and fill no page.
        DeskPick::Action(_) | DeskPick::VaultFill { .. } | DeskPick::Dismissed => None,
    };
    with(|f| f.answered(token, op, Instant::now()));
    true
}

/// The pick waiting for the field that opened `token`.
pub(crate) fn take(token: u64) -> Option<FieldOp> {
    if !PENDING.load(Ordering::Acquire) {
        return None;
    }
    with(|f| f.take(token))
}

/// The Edit rows a field offers: Cut with a selection in an editable,
/// non-secret field; Copy with a selection in a non-secret one; Paste in
/// an editable one; Select All always.
pub(crate) fn edit_rows(editable: bool, secret: bool, selection: bool) -> Vec<DeskEdit> {
    let mut rows = Vec::with_capacity(4);
    if selection && !secret && editable {
        rows.push(DeskEdit::Cut);
    }
    if selection && !secret {
        rows.push(DeskEdit::Copy);
    }
    if editable {
        rows.push(DeskEdit::Paste);
    }
    rows.push(DeskEdit::SelectAll);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fields_pick_waits_for_it_once() {
        let mut f = Fields::default();
        f.opened(7);
        assert!(f.is_open(7));
        f.answered(7, Some(FieldOp::Paste(Some("x".into()))), Instant::now());
        assert!(!f.is_open(7));
        assert!(matches!(f.take(7), Some(FieldOp::Paste(Some(_)))));
        assert!(f.take(7).is_none());
    }

    #[test]
    fn another_menus_pick_is_the_apps() {
        let pick = DeskMenuPick {
            app_id: "sola-terminal".into(),
            pid: std::process::id(),
            token: super::super::next_token(),
            pick: DeskPick::Action("copy".into()),
        };
        assert!(!observe(&pick));
        assert!(take(pick.token).is_none());
    }

    #[test]
    fn dismissed_leaves_nothing() {
        let mut f = Fields::default();
        f.opened(3);
        f.answered(3, None, Instant::now());
        assert!(!f.is_open(3));
        assert!(f.take(3).is_none());
    }

    #[test]
    fn old_menus_and_untaken_picks_are_forgotten() {
        let mut f = Fields::default();
        for t in 0..=OPEN_MAX as u64 {
            f.opened(t);
        }
        assert!(!f.is_open(0));
        assert!(f.is_open(OPEN_MAX as u64));
        let then = Instant::now();
        f.answered(5, Some(FieldOp::Copy), then);
        f.answered(6, None, then + PENDING_FOR);
        assert!(f.take(5).is_none());
    }

    #[test]
    fn edit_rows_follow_the_field() {
        use DeskEdit::*;
        assert_eq!(edit_rows(true, false, true), vec![Cut, Copy, Paste, SelectAll]);
        assert_eq!(edit_rows(true, false, false), vec![Paste, SelectAll]);
        // Read-only: copy and select only.
        assert_eq!(edit_rows(false, false, true), vec![Copy, SelectAll]);
        // A password field never gives its text out.
        assert_eq!(edit_rows(true, true, true), vec![Paste, SelectAll]);
        assert_eq!(edit_rows(false, true, true), vec![SelectAll]);
    }
}
