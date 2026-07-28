//! Shared mail message/folder types for UI and protocol.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub name: String,
    pub unread: u32,
    pub total: u32,
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

/// Sidebar badge: `unread/total` when unread > 0, else just `total` (apocrypha).
pub fn folder_count_badge(unread: u32, total: u32) -> Option<String> {
    if total == 0 {
        return None;
    }
    if unread > 0 {
        Some(format!("{unread}/{total}"))
    } else {
        Some(total.to_string())
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
            vec!["INBOX", "Sent", "Drafts", "Archive", "Junk", "Trash", "Zebra"]
        );
    }

    #[test]
    fn count_badge_formats() {
        assert_eq!(folder_count_badge(0, 0), None);
        assert_eq!(folder_count_badge(0, 12), Some("12".into()));
        assert_eq!(folder_count_badge(5, 12), Some("5/12".into()));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub uid: u32,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub seen: bool,
    pub forwarded_for: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBody {
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
}

impl MessageBody {
    /// Prefer plain text; if empty, fall back to HTML→text.
    pub fn display_text(&self) -> String {
        let plain = self.text.trim();
        if !plain.is_empty() {
            return self.text.clone();
        }
        if let Some(html) = &self.html {
            return crate::protocol::html_text::to_plain(html);
        }
        String::new()
    }
}
