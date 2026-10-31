//! Optimistic moves and undo.
//!
//! A moved row leaves the list at once and stays tombstoned until a
//! refresh no longer lists it; its summary is kept until the server
//! confirms so a failed MOVE can put it back. Undo reverses the last
//! move using the UID the message got in the destination — IMAP UIDs
//! are per-mailbox, so the source UID is meaningless there.

use std::collections::{HashMap, HashSet};

use crate::protocol::{MailId, MessageSummary};

#[derive(Debug, Clone)]
struct LastMove {
    id: MailId,
    dest: String,
    dest_uid: Option<u32>,
    summary: Option<MessageSummary>,
    undo_requested: bool,
    undo_sent: bool,
}

/// What [`Moves::undo`] did; the caller restores the row and counts.
pub struct Undo {
    pub summary: Option<MessageSummary>,
    pub to: String,
    /// Reverse MOVE to send now (`None`: waiting for the dest UID).
    pub reverse: Option<(MailId, String)>,
}

/// What a confirmed MOVE means for undo.
#[derive(Debug, PartialEq, Eq)]
pub enum Confirmed {
    Nothing,
    /// The dest UID just arrived for a pending undo: send this reverse.
    Reverse(MailId, String),
    /// The undo's reverse MOVE landed; the restored row now has this id.
    Restored {
        old: MailId,
        new: MailId,
    },
    /// No dest UID was reported, so the undo cannot find the copy.
    UndoImpossible,
}

#[derive(Default)]
pub struct Moves {
    gone: HashSet<MailId>,
    removed: HashMap<MailId, (MessageSummary, String)>,
    last: Option<LastMove>,
}

impl Moves {
    pub fn gone(&self) -> &HashSet<MailId> {
        &self.gone
    }

    pub fn can_undo(&self) -> bool {
        self.last.as_ref().is_some_and(|lm| !lm.undo_requested)
    }

    pub fn clear(&mut self) {
        self.gone.clear();
        self.removed.clear();
    }

    pub fn begin(&mut self, id: MailId, dest: String, summary: Option<MessageSummary>) {
        self.gone.insert(id.clone());
        if let Some(s) = &summary {
            self.removed.insert(id.clone(), (s.clone(), dest.clone()));
        }
        self.last = Some(LastMove {
            id,
            dest,
            dest_uid: None,
            summary,
            undo_requested: false,
            undo_sent: false,
        });
    }

    pub fn confirmed(&mut self, id: &MailId, dest: &str, dest_uid: Option<u32>) -> Confirmed {
        self.removed.remove(id);
        let Some(lm) = self.last.as_mut() else {
            return Confirmed::Nothing;
        };
        if lm.undo_requested
            && id.folder.eq_ignore_ascii_case(&lm.dest)
            && id.account == lm.id.account
            && Some(id.uid) == lm.dest_uid
        {
            let old = lm.id.clone();
            self.last = None;
            return match dest_uid {
                Some(uid) => Confirmed::Restored {
                    new: old.moved(dest, uid),
                    old,
                },
                None => Confirmed::Nothing,
            };
        }
        if *id != lm.id {
            return Confirmed::Nothing;
        }
        lm.dest_uid = dest_uid.or(lm.dest_uid);
        if !lm.undo_requested || lm.undo_sent {
            return Confirmed::Nothing;
        }
        match lm.dest_uid {
            Some(uid) => {
                lm.undo_sent = true;
                Confirmed::Reverse(lm.id.moved(&lm.dest, uid), lm.id.folder.clone())
            }
            None => {
                self.last = None;
                Confirmed::UndoImpossible
            }
        }
    }

    /// The MOVE failed: the row (if we still have it) goes back.
    pub fn failed(&mut self, id: &MailId) -> Option<(MessageSummary, String)> {
        self.gone.remove(id);
        if self.last.as_ref().is_some_and(|lm| lm.id == *id) {
            self.last = None;
        }
        self.removed.remove(id)
    }

    pub fn undo(&mut self) -> Option<Undo> {
        let lm = self.last.as_mut().filter(|lm| !lm.undo_requested)?;
        lm.undo_requested = true;
        self.gone.remove(&lm.id);
        let summary = self
            .removed
            .remove(&lm.id)
            .map(|(s, _)| s)
            .or_else(|| lm.summary.clone());
        let reverse = lm.dest_uid.map(|uid| {
            lm.undo_sent = true;
            (lm.id.moved(&lm.dest, uid), lm.id.folder.clone())
        });
        Some(Undo {
            summary,
            to: lm.dest.clone(),
            reverse,
        })
    }

    /// A fresh first page of `folder` no longer lists these tombstones:
    /// the server caught up. An account whose part `failed` sent no rows,
    /// so its tombstones stay.
    pub fn prune(&mut self, folder: &str, incoming: &[MessageSummary], failed: &[String]) {
        let present: HashSet<&MailId> = incoming.iter().map(|m| &m.id).collect();
        self.gone.retain(|id| {
            !id.folder.eq_ignore_ascii_case(folder)
                || present.contains(id)
                || failed.contains(&id.account)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(uid: u32) -> MailId {
        MailId::new("a@x", "INBOX", uid)
    }

    #[test]
    fn undo_after_confirm_reverses_dest_uid() {
        let mut m = Moves::default();
        m.begin(id(10), "Trash".into(), None);
        assert_eq!(m.confirmed(&id(10), "Trash", Some(77)), Confirmed::Nothing);
        let undo = m.undo().expect("undo");
        assert_eq!(
            undo.reverse,
            Some((MailId::new("a@x", "Trash", 77), "INBOX".into()))
        );
        // The reverse lands: the restored row gets its new INBOX UID.
        assert_eq!(
            m.confirmed(&MailId::new("a@x", "Trash", 77), "INBOX", Some(11)),
            Confirmed::Restored {
                old: id(10),
                new: id(11)
            }
        );
        assert!(!m.can_undo());
    }

    #[test]
    fn undo_before_confirm_waits_for_dest_uid() {
        let mut m = Moves::default();
        m.begin(id(10), "Trash".into(), None);
        assert!(m.undo().expect("undo").reverse.is_none());
        assert_eq!(
            m.confirmed(&id(10), "Trash", Some(77)),
            Confirmed::Reverse(MailId::new("a@x", "Trash", 77), "INBOX".into())
        );
    }

    #[test]
    fn undo_without_dest_uid_is_impossible_not_wrong() {
        let mut m = Moves::default();
        m.begin(id(10), "Trash".into(), None);
        m.undo();
        assert_eq!(
            m.confirmed(&id(10), "Trash", None),
            Confirmed::UndoImpossible
        );
    }

    #[test]
    fn prune_drops_only_caught_up_tombstones() {
        let mut m = Moves::default();
        m.begin(id(1), "Trash".into(), None);
        m.begin(id(2), "Trash".into(), None);
        let still_there = MessageSummary {
            id: id(2),
            from: String::new(),
            to: Vec::new(),
            subject: String::new(),
            date: 0,
            seen: true,
            flagged: false,
            forwarded_for: None,
            has_attachment: false,
        };
        m.prune("INBOX", &[still_there], &[]);
        assert!(!m.gone().contains(&id(1)));
        assert!(m.gone().contains(&id(2)));
    }

    #[test]
    fn a_failed_account_keeps_its_tombstones() {
        let mut m = Moves::default();
        m.begin(id(1), "Trash".into(), None);
        m.prune("INBOX", &[], &["a@x".into()]);
        assert!(m.gone().contains(&id(1)));
    }
}
