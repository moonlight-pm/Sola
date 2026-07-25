//! UI-facing events and commands. No wire JSON here.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Connected {
        backend: String,
        mode: ConnectionModeLabel,
    },
    Disconnected {
        reason: String,
    },
    SessionReady {
        id: String,
        title: Option<String>,
    },
    /// Transcript replace (after load or watch sync). May be a **tail** window only.
    Transcript {
        turns: Vec<Turn>,
        /// Absolute byte offset of the first line included in `turns`.
        history_start_byte: u64,
        has_older: bool,
        /// When true, this is a live watch re-read — preserve scroll stickiness.
        from_watch: bool,
    },
    /// Older history chunk to **prepend** (scroll-up load).
    HistoryOlder {
        turns: Vec<Turn>,
        history_start_byte: u64,
        has_older: bool,
    },
    UserEcho {
        text: String,
    },
    AgentDelta {
        text: String,
    },
    ThoughtDelta {
        text: String,
    },
    ToolStart {
        call_id: String,
        tool: String,
    },
    ToolUpdate {
        call_id: String,
        status: Option<String>,
        title: Option<String>,
    },
    ToolEnd {
        call_id: String,
        status: String,
    },
    Plan {
        entries: Vec<PlanEntry>,
    },
    Usage {
        used: u64,
        size: Option<u64>,
    },
    PermissionRequired {
        request_id: u64,
        tool: String,
        preview: String,
        options: Vec<PermissionChoice>,
    },
    TurnEnded {
        stop_reason: String,
    },
    Error {
        message: String,
    },
    SessionsListed {
        entries: Vec<SessionSummary>,
    },
    /// Progress while permanently deleting sessions.
    BulkDeleteProgress {
        done: u32,
        total: u32,
        last_id: String,
    },
    /// Bulk delete finished (partial failures possible).
    BulkDeleteFinished {
        deleted: u32,
        failed: u32,
        errors: Vec<String>,
    },
    /// Child / binary missing — show first-run guidance.
    NeedSetup {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionModeLabel {
    Local,
    /// Reserved for leader daemon attach.
    Leader,
}

impl ConnectionModeLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Leader => "leader",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PermissionChoice {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub cwd: String,
    /// Unix secs of last **turn** activity (updates.jsonl mtime), not open time.
    pub updated: u64,
    pub pinned: bool,
    /// True when a live Grok TUI process has this session open in a console.
    /// (Grouping only — activity uses [`Self::busy`].)
    #[serde(default)]
    pub live: bool,
    /// True when the session is actively working (recent transcript writes).
    /// Independent of whether a console has it open.
    #[serde(default)]
    pub busy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Turn {
    User(String),
    Assistant(String),
    Thought(String),
    Tool(ToolTurn),
    Plan(Vec<PlanEntry>),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTurn {
    pub call_id: String,
    /// Tool title/kind (metadata only; not expanded in the transcript).
    pub tool: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub enum AgentCmd {
    /// Ensure child is up and initialize ACP.
    EnsureConnected,
    NewSession {
        cwd: String,
    },
    LoadSession {
        id: String,
        cwd: String,
    },
    /// File-only open for a session held by an external Grok TUI.
    /// Does **not** call ACP `session/load` (read-only viewer).
    OpenReadonly {
        id: String,
        cwd: String,
    },
    /// Re-read the transcript tail for a watched (typically console) session.
    SyncTranscript {
        id: String,
        cwd: String,
        /// When true, leave in-progress tool statuses as-is (live watch).
        live: bool,
    },
    /// Load an older window of `updates.jsonl` ending at `before_byte`.
    LoadOlderHistory {
        id: String,
        cwd: String,
        before_byte: u64,
    },
    Send {
        text: String,
    },
    Cancel,
    /// Respond to `session/request_permission` with the given option id.
    Permission {
        request_id: u64,
        option_id: String,
    },
    /// Deny / cancel the permission request.
    PermissionCancel {
        request_id: u64,
    },
    RefreshSessions {
        cwd: String,
    },
    /// Permanently delete Grok sessions by id (`grok sessions delete`, then overlay scrub).
    BulkDelete {
        ids: Vec<String>,
    },
    Restart,
    Shutdown,
}
