//! Shared mail message/folder types for UI and protocol.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sola_kit::components::prose::ProseBlock;

/// A file on a message (received or outgoing). Bytes are `Arc` so the
/// worker → UI hop does not copy the payload.
#[derive(Debug, Clone)]
pub struct MailAttachment {
    pub filename: String,
    pub mime: String,
    pub size: u64,
    pub bytes: Arc<[u8]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub name: String,
    pub unread: u32,
    pub total: u32,
}

/// A list the sidebar can show: one canonical mailbox (combined across
/// accounts) or a smart mailbox (an INBOX rule view).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FolderRef {
    Mailbox(String),
    Smart(String),
}

impl FolderRef {
    pub fn inbox() -> Self {
        Self::Mailbox("INBOX".into())
    }

    /// The canonical mailbox the rows live in (smart views are INBOX).
    pub fn mailbox(&self) -> &str {
        match self {
            Self::Mailbox(name) => name,
            Self::Smart(_) => "INBOX",
        }
    }

    /// True for the canonical mailbox `name` (not a smart view).
    pub fn is(&self, name: &str) -> bool {
        matches!(self, Self::Mailbox(n) if n.eq_ignore_ascii_case(name))
    }
}

/// Identity of a message: account + canonical mailbox + UID. IMAP UIDs
/// are per mailbox *and* per account — never key UI state on `uid` alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailId {
    pub account: String,
    pub folder: String,
    pub uid: u32,
}

impl MailId {
    pub fn new(account: impl Into<String>, folder: impl Into<String>, uid: u32) -> Self {
        Self {
            account: account.into(),
            folder: folder.into(),
            uid,
        }
    }

    /// Same account, another mailbox / UID (the copy a MOVE left behind).
    pub fn moved(&self, folder: impl Into<String>, uid: u32) -> Self {
        Self::new(self.account.clone(), folder, uid)
    }
}

/// Display label for a mailbox name (`INBOX` → `Inbox`).
pub fn folder_label(name: &str) -> String {
    if name.eq_ignore_ascii_case("INBOX") {
        "Inbox".into()
    } else {
        name.to_string()
    }
}

/// Sidebar badge: unread only. Hidden when the folder is fully read.
pub fn unread_badge(unread: u32) -> Option<String> {
    (unread > 0).then(|| unread.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: MailId,
    /// `Name <addr>` or a bare address, RFC 2047 decoded.
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    /// Unix seconds (Date header, else INTERNALDATE). `0` when unknown.
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    /// Original recipient when mail was forwarded in (`X-Forwarded-For`).
    pub forwarded_for: Option<String>,
    pub has_attachment: bool,
}

impl MessageSummary {
    /// Newest first, then UID, then account (stable across combined IMAP).
    pub fn cmp_recency(a: &Self, b: &Self) -> std::cmp::Ordering {
        b.date
            .cmp(&a.date)
            .then_with(|| b.id.uid.cmp(&a.id.uid))
            .then_with(|| b.id.account.cmp(&a.id.account))
    }
}

#[derive(Debug, Clone)]
pub struct MessageBody {
    pub id: MailId,
    pub from: String,
    pub reply_to: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    /// Unix seconds; `0` when the Date header is missing.
    pub date: i64,
    /// The text/plain part (drafts reopen from it).
    pub text: String,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<MailAttachment>,
    /// Letter blocks for the reading pane, converted on the worker.
    pub blocks: Vec<ProseBlock>,
}

impl MessageBody {
    /// Copy / reply / forward text.
    pub fn display_text(&self) -> String {
        sola_kit::components::prose::flatten(&self.blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_label_is_title_case() {
        assert_eq!(folder_label("INBOX"), "Inbox");
        assert_eq!(folder_label("inbox"), "Inbox");
        assert_eq!(folder_label("Sent"), "Sent");
    }

    #[test]
    fn unread_badge_hides_zero() {
        assert_eq!(unread_badge(0), None);
        assert_eq!(unread_badge(5), Some("5".into()));
    }

    #[test]
    fn smart_rows_live_in_inbox() {
        assert_eq!(FolderRef::Smart("Receipts".into()).mailbox(), "INBOX");
        assert!(FolderRef::Mailbox("Trash".into()).is("trash"));
        assert!(!FolderRef::Smart("Trash".into()).is("Trash"));
    }
}
