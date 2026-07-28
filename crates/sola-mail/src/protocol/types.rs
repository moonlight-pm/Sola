//! Shared mail message/folder types for UI and protocol.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub name: String,
    pub unread: u32,
    pub total: u32,
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
