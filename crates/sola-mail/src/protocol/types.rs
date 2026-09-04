//! Shared mail message/folder types for UI and protocol.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

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

/// Identity of a message on a specific IMAP account. UIDs are per
/// mailbox *and* per account — never key UI state on `uid` alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MailId {
    pub account: String,
    pub uid: u32,
}

impl MailId {
    pub fn new(account: impl Into<String>, uid: u32) -> Self {
        Self {
            account: account.into(),
            uid,
        }
    }
}

/// Canonical system mailbox order (matches apocrypha folder-list).
const FOLDER_ORDER: &[(&str, u32)] = &[
    ("INBOX", 0),
    ("Sent", 1),
    ("Drafts", 2),
    ("Archive", 3),
    ("Junk", 4),
    ("Trash", 5),
];

/// Display label for a mailbox name (`INBOX` → `Inbox`).
pub fn folder_label(name: &str) -> String {
    if name.eq_ignore_ascii_case("INBOX") {
        "Inbox".into()
    } else {
        name.to_string()
    }
}

fn folder_rank(name: &str) -> u32 {
    for (n, rank) in FOLDER_ORDER {
        if name.eq_ignore_ascii_case(n) {
            return *rank;
        }
    }
    99
}

/// Sort folders: Inbox, Sent, Drafts, Archive, Junk, Trash, then others A–Z.
pub fn sort_folders(folders: &mut [Folder]) {
    folders.sort_by(|a, b| {
        let ra = folder_rank(&a.name);
        let rb = folder_rank(&b.name);
        ra.cmp(&rb).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
}

/// Sidebar badge: unread only. Hidden when the folder is fully read.
pub fn folder_count_badge(unread: u32, _total: u32) -> Option<String> {
    if unread == 0 {
        None
    } else {
        Some(unread.to_string())
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
    fn sorts_system_folders_first() {
        let mut folders = vec![
            Folder {
                name: "Zebra".into(),
                unread: 0,
                total: 0,
            },
            Folder {
                name: "Trash".into(),
                unread: 0,
                total: 0,
            },
            Folder {
                name: "INBOX".into(),
                unread: 0,
                total: 0,
            },
            Folder {
                name: "Archive".into(),
                unread: 0,
                total: 0,
            },
            Folder {
                name: "Sent".into(),
                unread: 0,
                total: 0,
            },
            Folder {
                name: "Junk".into(),
                unread: 0,
                total: 0,
            },
            Folder {
                name: "Drafts".into(),
                unread: 0,
                total: 0,
            },
        ];
        sort_folders(&mut folders);
        let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "INBOX", "Sent", "Drafts", "Archive", "Junk", "Trash", "Zebra"
            ]
        );
    }

    #[test]
    fn count_badge_formats() {
        assert_eq!(folder_count_badge(0, 0), None);
        assert_eq!(folder_count_badge(0, 12), None);
        assert_eq!(folder_count_badge(5, 12), Some("5".into()));
    }

    #[test]
    fn synthesized_html_plain_keeps_magic_link() {
        let token = "A".repeat(43);
        let url = format!("https://auth.naturalethic.com/login/magic/verify?token={token}");
        let text =
            format!("Use this link to sign in to Wicket:\n\n  {url}\n\nIt expires shortly.\n");
        let html = format!("<html><body>{}</body></html>", text.replace('\n', "<br/>"));
        let body = MessageBody {
            account: "you@example.com".into(),
            uid: 1,
            from: "Wicket <noreply@example.com>".into(),
            to: "you@example.com".into(),
            cc: String::new(),
            subject: "Sign in to Wicket".into(),
            date: String::new(),
            html: Some(html),
            text,
            in_reply_to: None,
            message_id: None,
            attachments: Vec::new(),
        };
        let blocks = body.reading_blocks();
        let has = blocks.iter().any(|b| match b {
            sola_kit::components::prose::ProseBlock::Paragraph(runs)
            | sola_kit::components::prose::ProseBlock::Quote(runs) => {
                runs.iter().any(|r| r.url.as_deref() == Some(url.as_str()))
            }
        });
        assert!(has, "{blocks:?}");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    #[serde(default)]
    pub account: String,
    pub uid: u32,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    /// Unix seconds from `date` (RFC 2822 or IMAP INTERNALDATE). Combined
    /// lists sort on this, not the display string.
    #[serde(default)]
    pub date_sort: i64,
    pub seen: bool,
    pub forwarded_for: Option<String>,
    pub has_attachment: bool,
}

impl MessageSummary {
    pub fn id(&self) -> MailId {
        MailId::new(&self.account, self.uid)
    }

    pub fn stamp_account(&mut self, account: &str) {
        self.account = account.to_string();
    }

    /// Newest first, then UID, then account (stable across combined IMAP).
    pub fn cmp_recency(a: &Self, b: &Self) -> std::cmp::Ordering {
        b.date_sort
            .cmp(&a.date_sort)
            .then_with(|| b.uid.cmp(&a.uid))
            .then_with(|| b.account.cmp(&a.account))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBody {
    #[serde(default)]
    pub account: String,
    pub uid: u32,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date: String,
    pub html: Option<String>,
    pub text: String,
    pub in_reply_to: Option<String>,
    pub message_id: Option<String>,
    #[serde(skip)]
    pub attachments: Vec<MailAttachment>,
}

impl MessageBody {
    pub fn id(&self) -> MailId {
        MailId::new(&self.account, self.uid)
    }
}

impl MessageBody {
    /// Copy / reply text. Prefers a real plaintext part; uses HTML when
    /// the plain part is a generator stub (the usual HTML-mail case).
    pub fn display_text(&self) -> String {
        sola_kit::components::prose::flatten(&self.reading_blocks())
    }

    /// Letter blocks for the reading pane. Prefer HTML whenever it is
    /// present — that is the part mail apps actually render.
    pub fn reading_blocks(&self) -> Vec<sola_kit::components::prose::ProseBlock> {
        use crate::protocol::html_text::to_blocks;
        use sola_kit::components::prose::parse_plain;

        if let Some(html) = &self.html {
            if !html.trim().is_empty() {
                // mail-parser lists text/plain parts in `html_body` too and
                // `body_html` then synthesizes `<br/>` HTML. Prefer the real
                // plaintext so bare URLs (Wicket magic links) stay links.
                if crate::protocol::html_text::is_synthesized_plain_html(html) {
                    if !self.text.trim().is_empty() {
                        return parse_plain(&self.text);
                    }
                }
                return to_blocks(html);
            }
        }
        if !self.text.trim().is_empty() {
            return parse_plain(&self.text);
        }
        Vec::new()
    }
}
