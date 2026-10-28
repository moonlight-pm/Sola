//! Flags we set that the server's lists have not shown back yet.
//!
//! Opening a letter marks it read at once and sends the STORE. A list
//! fetched before the STORE landed (a refresh already in flight, often
//! waiting on a slow account) still has it unread, and merging it put
//! the dot back for a few seconds. As with a move's tombstone, our flag
//! wins until a list agrees.

use std::collections::HashMap;

use crate::protocol::{MailId, MessageSummary};
use crate::worker::Flag;

#[derive(Default)]
pub struct PendingFlags {
    seen: HashMap<MailId, bool>,
    flagged: HashMap<MailId, bool>,
}

impl PendingFlags {
    pub fn set(&mut self, id: &MailId, flag: Flag, on: bool) {
        self.map(flag).insert(id.clone(), on);
    }

    /// The STORE failed: the server's word stands.
    pub fn failed(&mut self, id: &MailId) {
        self.seen.remove(id);
        self.flagged.remove(id);
    }

    /// Rows from the server: a flag they already show is done; one they
    /// don't show yet is ours.
    pub fn settle(&mut self, rows: &mut [MessageSummary]) {
        for m in rows {
            settle_one(&mut self.seen, &m.id, &mut m.seen);
            settle_one(&mut self.flagged, &m.id, &mut m.flagged);
        }
    }

    fn map(&mut self, flag: Flag) -> &mut HashMap<MailId, bool> {
        match flag {
            Flag::Seen => &mut self.seen,
            Flag::Flagged => &mut self.flagged,
        }
    }
}

fn settle_one(pending: &mut HashMap<MailId, bool>, id: &MailId, shown: &mut bool) {
    match pending.get(id) {
        Some(&on) if *shown == on => {
            pending.remove(id);
        }
        Some(&on) => *shown = on,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(uid: u32, seen: bool) -> MessageSummary {
        MessageSummary {
            id: MailId::new("a@x", "INBOX", uid),
            from: String::new(),
            to: Vec::new(),
            subject: String::new(),
            date: 0,
            seen,
            flagged: false,
            forwarded_for: None,
            has_attachment: false,
        }
    }

    #[test]
    fn a_list_from_before_the_store_keeps_our_read() {
        let mut p = PendingFlags::default();
        p.set(&row(7, false).id, Flag::Seen, true);
        let mut stale = vec![row(7, false), row(8, false)];
        p.settle(&mut stale);
        assert!(stale[0].seen);
        assert!(!stale[1].seen, "only ours");
        // The server caught up; later it's the server's again.
        p.settle(&mut [row(7, true)]);
        let mut unread_elsewhere = vec![row(7, false)];
        p.settle(&mut unread_elsewhere);
        assert!(!unread_elsewhere[0].seen);
    }

    #[test]
    fn a_failed_store_gives_the_server_back_its_word() {
        let mut p = PendingFlags::default();
        let id = row(7, false).id;
        p.set(&id, Flag::Flagged, true);
        p.failed(&id);
        let mut rows = vec![row(7, false)];
        p.settle(&mut rows);
        assert!(!rows[0].flagged);
    }
}
