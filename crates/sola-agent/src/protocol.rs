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
    /// Transcript replace (after load). May be a **tail** window only.
    Transcript {
        turns: Vec<Turn>,
        /// Absolute byte offset of the first line included in `turns`.
        history_start_byte: u64,
        has_older: bool,
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
        args: serde_json::Value,
    },
    ToolUpdate {
        call_id: String,
        status: Option<String>,
        title: Option<String>,
        output: Option<String>,
    },
    ToolEnd {
        call_id: String,
        status: String,
        output: Option<String>,
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

#[derive(Debug, Clone)]
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
    /// True when a live Grok TUI process has this session open.
    #[serde(default)]
    pub live: bool,
}

#[derive(Debug, Clone)]
pub enum Turn {
    User(String),
    Assistant(String),
    Thought(String),
    Tool(ToolTurn),
    Plan(Vec<PlanEntry>),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ToolTurn {
    pub call_id: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub status: String,
    pub output: String,
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
    Restart,
    Shutdown,
}
