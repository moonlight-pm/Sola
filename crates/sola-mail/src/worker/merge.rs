//! Combine per-account replies into the one list / count set the UI shows.

use crate::protocol::boxes::CANONICAL;
use crate::protocol::imap::{Rows, SmartCounts};
use crate::protocol::{Folder, MessageSummary};

/// One account's mailbox counts (only the boxes it has).
pub struct AccountCounts {
    pub boxes: Vec<Folder>,
    pub smart: SmartCounts,
}

/// Newest `limit` rows across accounts.
///
/// Each account sent its own newest `limit` (below its cursor), so the
/// combined top `limit` is exact: no account can have a newer row it
/// did not send.
pub fn pages(parts: Vec<Rows>, limit: u32) -> (Vec<MessageSummary>, u32) {
    let total = parts
        .iter()
        .map(|p| p.total)
        .fold(0u32, u32::saturating_add);
    let mut rows: Vec<MessageSummary> = parts.into_iter().flat_map(|p| p.rows).collect();
    rows.sort_by(MessageSummary::cmp_recency);
    rows.truncate(limit as usize);
    (rows, total)
}

/// Sum the six canonical boxes and each smart mailbox across accounts;
/// INBOX loses what smart mailboxes show instead.
pub fn counts(parts: Vec<AccountCounts>) -> (Vec<Folder>, Vec<Folder>) {
    let mut boxes: Vec<Folder> = CANONICAL
        .iter()
        .map(|name| Folder {
            name: (*name).to_string(),
            unread: 0,
            total: 0,
        })
        .collect();
    let mut smart: Vec<Folder> = Vec::new();
    let (mut total_cut, mut unread_cut) = (0u32, 0u32);
    for part in parts {
        for f in part.boxes {
            if let Some(b) = boxes.iter_mut().find(|b| b.name == f.name) {
                b.unread = b.unread.saturating_add(f.unread);
                b.total = b.total.saturating_add(f.total);
            }
        }
        for f in part.smart.folders {
            match smart.iter_mut().find(|s| s.name == f.name) {
                Some(s) => {
                    s.unread = s.unread.saturating_add(f.unread);
                    s.total = s.total.saturating_add(f.total);
                }
                None => smart.push(f),
            }
        }
        total_cut = total_cut.saturating_add(part.smart.inbox_total_deduction);
        unread_cut = unread_cut.saturating_add(part.smart.inbox_unread_deduction);
    }
    if let Some(inbox) = boxes.iter_mut().find(|b| b.name == "INBOX") {
        inbox.total = inbox.total.saturating_sub(total_cut);
        inbox.unread = inbox.unread.saturating_sub(unread_cut);
    }
    (boxes, smart)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::MailId;

    fn row(account: &str, uid: u32, date: i64) -> MessageSummary {
        MessageSummary {
            id: MailId::new(account, "INBOX", uid),
            from: String::new(),
            to: Vec::new(),
            subject: String::new(),
            date,
            seen: true,
            flagged: false,
            forwarded_for: None,
            has_attachment: false,
        }
    }

    #[test]
    fn pages_interleave_by_date_not_uid() {
        let a = Rows {
            rows: vec![row("a", 90_000, 300), row("a", 89_999, 100)],
            total: 10,
        };
        let b = Rows {
            rows: vec![row("b", 12, 200), row("b", 11, 50)],
            total: 5,
        };
        let (rows, total) = pages(vec![a, b], 3);
        let ids: Vec<(&str, u32)> = rows
            .iter()
            .map(|r| (r.id.account.as_str(), r.id.uid))
            .collect();
        assert_eq!(ids, vec![("a", 90_000), ("b", 12), ("a", 89_999)]);
        assert_eq!(total, 15);
    }

    #[test]
    fn counts_sum_and_deduct_smart_from_inbox() {
        let part = |inbox: (u32, u32), smart: u32| AccountCounts {
            boxes: vec![Folder {
                name: "INBOX".into(),
                unread: inbox.0,
                total: inbox.1,
            }],
            smart: SmartCounts {
                folders: vec![Folder {
                    name: "Receipts".into(),
                    unread: 1,
                    total: smart,
                }],
                inbox_total_deduction: smart,
                inbox_unread_deduction: 1,
            },
        };
        let (boxes, smart) = counts(vec![part((5, 40), 4), part((2, 10), 1)]);
        let inbox = boxes.iter().find(|b| b.name == "INBOX").expect("inbox");
        assert_eq!((inbox.unread, inbox.total), (5, 45));
        assert_eq!(boxes.len(), CANONICAL.len());
        assert_eq!(smart.len(), 1);
        assert_eq!((smart[0].unread, smart[0].total), (2, 5));
    }
}
