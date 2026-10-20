//! Keep the on-screen message list stable across IMAP page refreshes.

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::protocol::{MailId, MessageSummary};

/// Merge a quiet re-fetch of the newest rows into the visible list.
///
/// - Rows inside the refreshed window that the server no longer sent
///   were moved / expunged elsewhere: they go.
/// - Rows older than the window (a load-more tail) stay, unless the
///   window already covered the whole mailbox.
/// - Rows of accounts whose part `failed` stay exactly as they were.
/// - `gone` rows (our own moves the server has not caught up with) never
///   come back.
///
/// "Older" is list order ([`MessageSummary::cmp_recency`]): UIDs from
/// different accounts are not comparable.
pub fn merge_refresh(
    current: &[MessageSummary],
    incoming: Vec<MessageSummary>,
    total: u32,
    gone: &HashSet<MailId>,
    failed: &[String],
) -> Vec<MessageSummary> {
    let window_len = incoming.len() as u32;
    let mut out: Vec<MessageSummary> = incoming
        .into_iter()
        .filter(|m| !gone.contains(&m.id))
        .collect();
    let oldest = out.iter().max_by(|a, b| MessageSummary::cmp_recency(a, b)).cloned();
    let seen: HashSet<MailId> = out.iter().map(|m| m.id.clone()).collect();
    let partial = total > window_len;
    for m in current {
        if seen.contains(&m.id) || gone.contains(&m.id) {
            continue;
        }
        let keep = failed.contains(&m.id.account)
            || (partial
                && oldest
                    .as_ref()
                    .is_none_or(|o| MessageSummary::cmp_recency(m, o) == Ordering::Greater));
        if keep {
            out.push(m.clone());
        }
    }
    out.sort_by(MessageSummary::cmp_recency);
    out
}

/// Append a load-more page, skipping rows already shown or tombstoned.
pub fn append_more(
    current: &mut Vec<MessageSummary>,
    incoming: Vec<MessageSummary>,
    gone: &HashSet<MailId>,
) {
    let seen: HashSet<MailId> = current.iter().map(|m| m.id.clone()).collect();
    current.extend(
        incoming
            .into_iter()
            .filter(|m| !seen.contains(&m.id) && !gone.contains(&m.id)),
    );
    current.sort_by(MessageSummary::cmp_recency);
}

/// Put a row back (undo / failed move) in list order.
pub fn insert(rows: &mut Vec<MessageSummary>, msg: MessageSummary) {
    if rows.iter().any(|m| m.id == msg.id) {
        return;
    }
    rows.push(msg);
    rows.sort_by(MessageSummary::cmp_recency);
}

/// Per account, the oldest UID on screen in `folder`: where the next
/// page continues.
pub fn cursors(rows: &[MessageSummary]) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for m in rows {
        match out.iter_mut().find(|(a, _)| *a == m.id.account) {
            Some((_, uid)) => *uid = (*uid).min(m.id.uid),
            None => out.push((m.id.account.clone(), m.id.uid)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum_on(account: &str, uid: u32, date: i64) -> MessageSummary {
        MessageSummary {
            id: MailId::new(account, "INBOX", uid),
            from: String::new(),
            to: Vec::new(),
            subject: format!("{uid}"),
            date,
            seen: true,
            flagged: false,
            forwarded_for: None,
            has_attachment: false,
        }
    }

    fn sum(uid: u32) -> MessageSummary {
        sum_on("a", uid, uid as i64)
    }

    fn page(from: u32, to: u32) -> Vec<MessageSummary> {
        (to..=from).rev().map(sum).collect()
    }

    fn uids(rows: &[MessageSummary]) -> Vec<u32> {
        rows.iter().map(|m| m.id.uid).collect()
    }

    fn gone(ids: &[u32]) -> HashSet<MailId> {
        ids.iter().map(|u| MailId::new("a", "INBOX", *u)).collect()
    }

    #[test]
    fn refresh_keeps_scrolled_tail() {
        let out = merge_refresh(&page(150, 1), page(150, 101), 150, &gone(&[]), &[]);
        assert_eq!(out.len(), 150);
        assert_eq!(uids(&out)[..3], [150, 149, 148]);
        assert_eq!(*uids(&out).last().unwrap(), 1);
    }

    #[test]
    fn remote_expunge_of_whole_mailbox_drops_rows() {
        // Another client moved 5 out; incoming is the whole remaining
        // mailbox — the missing rows are not a scrolled-off tail.
        let out = merge_refresh(&page(50, 1), page(50, 6), 45, &gone(&[]), &[]);
        assert_eq!(uids(&out), (6..=50).rev().collect::<Vec<_>>());
    }

    #[test]
    fn remote_expunge_in_window_drops_row_keeps_tail() {
        let mut incoming = page(150, 101);
        incoming.retain(|m| m.id.uid != 120);
        let out = merge_refresh(&page(150, 1), incoming, 149, &gone(&[]), &[]);
        let ids = uids(&out);
        assert!(!ids.contains(&120));
        assert_eq!(ids[0], 150);
        assert_eq!(*ids.last().unwrap(), 1);
        assert_eq!(ids.len(), 149);
    }

    #[test]
    fn tombstone_stays_hidden_until_server_catches_up() {
        let mut current = page(50, 1);
        current.retain(|m| m.id.uid != 40);
        let out = merge_refresh(&current, page(50, 1), 50, &gone(&[40]), &[]);
        assert!(!uids(&out).contains(&40));
        assert_eq!(out.len(), 49);
    }

    #[test]
    fn tail_is_by_date_across_accounts() {
        // Account a has huge UIDs, b small ones. The refreshed window is
        // the newest 2 by date; older rows of *both* accounts stay.
        let current = vec![
            sum_on("a", 90_000, 400),
            sum_on("b", 20, 300),
            sum_on("a", 89_000, 200),
            sum_on("b", 10, 100),
        ];
        let incoming = vec![sum_on("a", 90_000, 400), sum_on("b", 20, 300)];
        let out = merge_refresh(&current, incoming, 4, &HashSet::new(), &[]);
        assert_eq!(out.len(), 4, "{out:?}");
    }

    #[test]
    fn failed_account_rows_stay() {
        let current = vec![sum_on("a", 5, 500), sum_on("b", 9, 400)];
        let out = merge_refresh(&current, vec![sum_on("a", 5, 500)], 1, &HashSet::new(), &["b".into()]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn load_more_skips_duplicates_and_tombstones() {
        let mut current = page(50, 21);
        append_more(&mut current, page(25, 1), &gone(&[10]));
        assert_eq!(current.len(), 49);
        assert!(!uids(&current).contains(&10));
        assert!(uids(&current).windows(2).all(|w| w[0] > w[1]));
    }

    #[test]
    fn insert_keeps_recency_order() {
        let mut rows = page(5, 3);
        insert(&mut rows, sum(4));
        insert(&mut rows, sum(9));
        insert(&mut rows, sum(1));
        insert(&mut rows, sum(9));
        assert_eq!(uids(&rows), vec![9, 5, 4, 3, 1]);
    }

    #[test]
    fn cursors_take_each_accounts_oldest_uid() {
        let rows = vec![sum_on("a", 90, 9), sum_on("b", 7, 8), sum_on("a", 80, 7)];
        assert_eq!(cursors(&rows), vec![("a".into(), 80), ("b".into(), 7)]);
    }
}
