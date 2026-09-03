//! Keep the on-screen message list stable across IMAP page refreshes.

use std::collections::HashSet;

use crate::protocol::MessageSummary;

/// Apply a server page onto the visible list without dropping already-loaded
/// later rows, and without resurrecting UIDs the UI has already removed.
///
/// `mailbox_total` is the folder size from IMAP (the `total` on
/// [`crate::worker::MailEvent::Messages`]). Silent refresh re-fetches the
/// loaded window; when the mailbox shrank (another client MOVE/EXPUNGE),
/// `incoming` is shorter than `current` — that is *not* a scrolled tail.
pub fn apply_message_page(
    current: &[MessageSummary],
    incoming: Vec<MessageSummary>,
    offset: u32,
    mailbox_total: u32,
    pending_gone: &HashSet<(String, u32)>,
    folder: &str,
) -> Vec<MessageSummary> {
    let incoming: Vec<MessageSummary> = incoming
        .into_iter()
        .filter(|m| !pending_gone.contains(&(folder.to_string(), m.uid)))
        .collect();

    if offset > 0 {
        let mut out = current.to_vec();
        for m in incoming {
            if !out.iter().any(|e| e.uid == m.uid) {
                out.push(m);
            }
        }
        return out;
    }

    // Prefix of a still-larger mailbox, and shorter than what we already
    // show (first-page refresh of a load-more list). Keep rows older than
    // this page. Missing UIDs that belong *in* the page were deleted.
    let keep_older_tail = incoming.len() < current.len() && mailbox_total > incoming.len() as u32;
    if !keep_older_tail {
        return incoming;
    }
    let Some(oldest_fetched) = incoming.iter().map(|m| m.uid).min() else {
        return incoming;
    };
    let incoming_uids: HashSet<u32> = incoming.iter().map(|m| m.uid).collect();
    let tail: Vec<MessageSummary> = current
        .iter()
        .filter(|m| {
            m.uid < oldest_fetched
                && !incoming_uids.contains(&m.uid)
                && !pending_gone.contains(&(folder.to_string(), m.uid))
        })
        .cloned()
        .collect();
    let mut out = incoming;
    out.extend(tail);
    out.sort_unstable_by(|a, b| b.uid.cmp(&a.uid));
    out
}

/// UIDs the server first page no longer contains can leave the tombstone set.
pub fn prune_pending_gone(
    pending_gone: &mut HashSet<(String, u32)>,
    folder: &str,
    incoming_unfiltered: &[MessageSummary],
    offset: u32,
) {
    if offset != 0 {
        return;
    }
    let present: HashSet<u32> = incoming_unfiltered.iter().map(|m| m.uid).collect();
    pending_gone.retain(|(f, uid)| f != folder || present.contains(uid));
}

/// UID to use when reversing a move. IMAP UIDs are per-mailbox: never fall
/// back to the source UID against the destination folder.
pub fn reverse_move_uid(_source_uid: u32, dest_uid: Option<u32>) -> Option<u32> {
    dest_uid
}

pub fn insert_summary_desc(messages: &mut Vec<MessageSummary>, msg: MessageSummary) {
    if messages.iter().any(|m| m.uid == msg.uid) {
        return;
    }
    match messages.iter().position(|m| m.uid < msg.uid) {
        Some(i) => messages.insert(i, msg),
        None => messages.push(msg),
    }
}

pub fn hidden_on_server(
    incoming: &[MessageSummary],
    pending_gone: &HashSet<(String, u32)>,
    folder: &str,
) -> u32 {
    incoming
        .iter()
        .filter(|m| pending_gone.contains(&(folder.to_string(), m.uid)))
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(uid: u32) -> MessageSummary {
        MessageSummary {
            uid,
            from: String::new(),
            to: String::new(),
            subject: format!("{uid}"),
            date: String::new(),
            seen: true,
            forwarded_for: None,
            has_attachment: false,
        }
    }

    fn page(from: u32, to: u32) -> Vec<MessageSummary> {
        (to..=from).rev().map(sum).collect()
    }

    fn uids(rows: &[MessageSummary]) -> Vec<u32> {
        rows.iter().map(|m| m.uid).collect()
    }

    #[test]
    fn first_page_refresh_keeps_scrolled_tail() {
        let current = page(150, 1);
        let incoming = page(150, 101);
        let out = apply_message_page(&current, incoming, 0, 150, &HashSet::new(), "INBOX");
        assert_eq!(out.len(), 150);
        assert_eq!(uids(&out)[..3], [150, 149, 148]);
        assert_eq!(*uids(&out).last().unwrap(), 1);
    }

    #[test]
    fn remote_expunge_of_whole_mailbox_drops_rows() {
        // One page loaded; another client moved 5 out. Incoming is the remaining
        // mailbox — must not treat the missing UIDs as a scrolled-off tail.
        let current = page(50, 1);
        let incoming = page(50, 6);
        let out = apply_message_page(&current, incoming, 0, 45, &HashSet::new(), "INBOX");
        let ids = uids(&out);
        assert_eq!(ids, (6..=50).rev().collect::<Vec<_>>());
        assert!(!ids.contains(&1));
        assert!(!ids.contains(&5));
    }

    #[test]
    fn remote_expunge_in_prefix_drops_uid_keeps_true_tail() {
        let current = page(150, 1);
        // First page after uid 120 left: 50 newest remaining, then the load-more tail.
        let mut incoming = page(150, 101);
        incoming.retain(|m| m.uid != 120);
        incoming.push(sum(100));
        let out = apply_message_page(&current, incoming, 0, 149, &HashSet::new(), "INBOX");
        let ids = uids(&out);
        assert!(!ids.contains(&120));
        assert_eq!(ids[0], 150);
        assert_eq!(*ids.last().unwrap(), 1);
        assert_eq!(ids.len(), 149);
    }

    #[test]
    fn tombstone_stays_hidden_when_server_has_not_caught_up() {
        let mut current = page(50, 1);
        current.retain(|m| m.uid != 40);
        let incoming = page(50, 1);
        let mut gone = HashSet::new();
        gone.insert(("INBOX".into(), 40));
        let out = apply_message_page(&current, incoming, 0, 50, &gone, "INBOX");
        assert!(!uids(&out).contains(&40));
        assert_eq!(out.len(), 49);
    }

    #[test]
    fn merge_after_delete_does_not_duplicate_or_drop_tail() {
        let mut current = page(100, 1);
        current.retain(|m| m.uid != 90);
        let incoming = page(100, 51);
        let mut gone = HashSet::new();
        gone.insert(("INBOX".into(), 90));
        let out = apply_message_page(&current, incoming, 0, 99, &gone, "INBOX");
        let ids = uids(&out);
        assert!(!ids.contains(&90));
        assert_eq!(ids.len(), 99);
        assert!(ids.windows(2).all(|w| w[0] > w[1]));
        assert_eq!(ids[0], 100);
        assert_eq!(*ids.last().unwrap(), 1);
    }

    #[test]
    fn load_more_skips_duplicates_and_tombstones() {
        let current = page(50, 1);
        let incoming = page(20, 1);
        let mut gone = HashSet::new();
        gone.insert(("INBOX".into(), 10));
        let out = apply_message_page(&current, incoming, 50, 50, &gone, "INBOX");
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn prune_only_on_first_page() {
        let incoming = page(50, 1);
        let mut gone = HashSet::new();
        gone.insert(("INBOX".into(), 90));
        gone.insert(("INBOX".into(), 40));
        prune_pending_gone(&mut gone, "INBOX", &incoming, 0);
        assert!(!gone.contains(&("INBOX".into(), 90)));
        assert!(gone.contains(&("INBOX".into(), 40)));
    }

    #[test]
    fn undo_does_not_reuse_source_uid() {
        assert_eq!(reverse_move_uid(123, Some(456)), Some(456));
        assert_eq!(reverse_move_uid(123, None), None);
    }

    #[test]
    fn insert_keeps_uid_desc() {
        let mut rows = page(5, 3);
        insert_summary_desc(&mut rows, sum(4));
        insert_summary_desc(&mut rows, sum(9));
        insert_summary_desc(&mut rows, sum(1));
        assert_eq!(uids(&rows), vec![9, 5, 4, 3, 1]);
    }
}
