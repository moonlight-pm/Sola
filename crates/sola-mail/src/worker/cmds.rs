//! Typed commands and events between UI and mail worker.

use sola_bus::topics::MailConfig;

use crate::protocol::{Folder, MessageBody, MessageSummary};

#[derive(Debug, Clone)]
pub enum MailCmd {
    /// Connect (or reconnect) using the worker's current config.
    Connect,
    /// Push latest bus config; reconnects if already connected or credentials changed.
    Reconfigure(MailConfig),
    ListFolders,
    ListMessages {
        folder: String,
        offset: u32,
        limit: u32,
    },
    Search {
        query: String,
    },
    FetchBody {
        folder: String,
        uid: u32,
    },
    MarkRead {
        folder: String,
        uid: u32,
    },
    Move {
        folder: String,
        uid: u32,
        dest: String,
    },
    EmptyFolder {
        folder: String,
    },
    Send {
        from: String,
        to: String,
        cc: String,
        subject: String,
        body: String,
        in_reply_to: Option<String>,
    },
    ApplyRules,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum MailEvent {
    Connected {
        folders: Vec<Folder>,
        smart_counts: Vec<Folder>,
        from_addresses: Vec<String>,
        rules: Vec<sola_bus::topics::MailRule>,
    },
    Folders {
        folders: Vec<Folder>,
        smart_counts: Vec<Folder>,
    },
    Messages {
        folder: String,
        messages: Vec<MessageSummary>,
        total: u32,
        offset: u32,
    },
    SearchResults {
        messages: Vec<MessageSummary>,
        total: u32,
    },
    Body(MessageBody),
    Sent,
    Moved {
        folder: String,
        uid: u32,
        dest: String,
    },
    Emptied {
        folder: String,
    },
    RulesApplied {
        moved: u32,
    },
    /// IDLE saw remaining new mail after move-rules.
    NewMail,
    Error {
        context: String,
        message: String,
    },
    Disconnected {
        reason: String,
    },
    /// Config present but incomplete — UI shows settings prompt.
    NotConfigured,
}
