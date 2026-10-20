//! Typed commands and events between UI and mail worker.

use sola_bus::topics::MailConfig;

use crate::protocol::sender::Outgoing;
use crate::protocol::{Folder, FolderRef, MailId, MessageBody, MessageSummary};

/// How a list page relates to what the UI already shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// A new list (folder switch): replace the rows.
    Replace,
    /// Quiet re-fetch of the newest rows: merge.
    Refresh,
    /// The next rows below the loaded ones: append.
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Seen,
    Flagged,
}

impl Flag {
    pub fn imap(self) -> &'static str {
        match self {
            Self::Seen => "\\Seen",
            Self::Flagged => "\\Flagged",
        }
    }
}

#[derive(Debug, Clone)]
pub enum MailCmd {
    /// Latest bus config. Reconnects only accounts whose IMAP login
    /// changed; a rules-only edit keeps every session.
    Reconfigure(MailConfig),
    /// Get New Mail: reconnect failed accounts now.
    Retry,
    ListFolders,
    List {
        /// Echoed back so the UI can drop pages for a list it left.
        epoch: u64,
        folder: FolderRef,
        page: Page,
        /// [`Page::More`]: per account, the oldest UID already shown.
        cursors: Vec<(String, u32)>,
        limit: u32,
    },
    Search {
        epoch: u64,
        query: String,
        /// Extra canonical mailbox to search (the open Junk / Trash).
        also: Option<String>,
    },
    FetchBody(MailId),
    SetFlag {
        id: MailId,
        flag: Flag,
        on: bool,
    },
    Move {
        id: MailId,
        dest: String,
    },
    Empty {
        folder: String,
    },
    Send(Outgoing),
    SaveDraft(Outgoing),
    Shutdown,
}

/// What failed, for the UI's wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    List,
    Search,
    Open(MailId),
    Flag(MailId),
    Erase,
    Send,
    SaveDraft,
}

#[derive(Debug, Clone)]
pub enum MailEvent {
    /// One IMAP account's connection state.
    AccountLink {
        account: String,
        email: String,
        state: LinkState,
    },
    /// Connected accounts or rules changed: refresh folders and list.
    Changed,
    /// No IMAP account is configured.
    NotConfigured,
    Folders {
        folders: Vec<Folder>,
        smart: Vec<Folder>,
    },
    Messages {
        epoch: u64,
        folder: FolderRef,
        page: Page,
        rows: Vec<MessageSummary>,
        total: u32,
        /// Accounts whose part failed: keep their rows as they are.
        failed: Vec<String>,
    },
    SearchResults {
        epoch: u64,
        rows: Vec<MessageSummary>,
        capped: bool,
    },
    Body(Box<MessageBody>),
    Moved {
        id: MailId,
        dest: String,
        /// UID in `dest` (IMAP UIDs are per-mailbox).
        dest_uid: Option<u32>,
    },
    MoveFailed {
        id: MailId,
        message: String,
    },
    Emptied {
        folder: String,
        /// `account: error` for accounts that could not erase.
        failed: Vec<String>,
    },
    Sent,
    DraftSaved,
    /// INBOX changed on the server (IDLE, after move rules).
    NewMail,
    Failed {
        op: Op,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Ready,
    Failed(String),
}
