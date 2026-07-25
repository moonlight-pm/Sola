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
    /// Agent / leader identity from ACP `initialize` `_meta`.
    AgentInfo {
        agent_version: Option<String>,
        model_id: Option<String>,
        efforts: Vec<EffortOption>,
        current_effort: Option<String>,
    },
    /// Periodic `grok update --check` result.
    GrokVersion {
        current: Option<String>,
        latest: Option<String>,
        update_available: bool,
        channel: Option<String>,
    },
    SessionReady {
        id: String,
        title: Option<String>,
    },
    /// Session config options refreshed after new/load (effort list etc.).
    SessionConfig {
        efforts: Vec<EffortOption>,
        current_effort: Option<String>,
        model_id: Option<String>,
    },
    /// Transcript replace (after load or watch sync). May be a **tail** window only.
    Transcript {
        /// Session this slice belongs to — UI drops stale emits after a fast switch.
        session_id: String,
        turns: Vec<Turn>,
        /// Absolute byte offset of the first line included in `turns`.
        history_start_byte: u64,
        has_older: bool,
        /// When true, this is a live watch re-read — preserve scroll stickiness.
        from_watch: bool,
    },
    /// Older history chunk to **prepend** (scroll-up load).
    HistoryOlder {
        /// Session this slice belongs to — UI drops stale emits after a fast switch.
        session_id: String,
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
    /// Attached to shared `grok agent leader` (only supported mode).
    Leader,
}

/// Selectable reasoning-effort option from model `_meta.reasoningEfforts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffortOption {
    pub id: String,
    pub label: String,
}

/// Permission mode shown in the footer. Wire value is ACP `session/set_mode` id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Skip all tool prompts (`bypassPermissions` / always-approve).
    AlwaysApprove,
    /// Prompt for tools (normal).
    Default,
    /// Auto-approve safe edits where the agent supports it.
    Auto,
    /// Plan mode — explore/plan, limited writes.
    Plan,
}

impl PermissionMode {
    pub fn default_mode() -> Self {
        Self::AlwaysApprove
    }

    pub fn as_mode_id(self) -> &'static str {
        match self {
            // Grok: bypassPermissions / --always-approve / YOLO.
            Self::AlwaysApprove => "bypassPermissions",
            Self::Default => "default",
            // Grok's acceptEdits (auto-approve safe edits); not the string "auto".
            Self::Auto => "acceptEdits",
            Self::Plan => "plan",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AlwaysApprove => "always-approve",
            Self::Default => "default",
            Self::Auto => "auto",
            Self::Plan => "plan",
        }
    }

    /// Client should auto-answer `session/request_permission` without a strip.
    pub fn auto_answers_permissions(self) -> bool {
        matches!(self, Self::AlwaysApprove)
    }

    pub fn all() -> &'static [PermissionMode] {
        &[
            Self::AlwaysApprove,
            Self::Default,
            Self::Auto,
            Self::Plan,
        ]
    }
}

impl ConnectionModeLabel {
    pub fn as_str(self) -> &'static str {
        match self {
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
    /// True when the session is actively working (recent transcript writes).
    #[serde(default)]
    pub busy: bool,
    /// Last known context tokens used (from `usage_update` on disk or live ACP).
    #[serde(default)]
    pub usage_used: Option<u64>,
    /// Context window size when known.
    #[serde(default)]
    pub usage_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Turn {
    User(String),
    Assistant(String),
    Thought(ThoughtTurn),
    Tool(ToolTurn),
    Plan(Vec<PlanEntry>),
    Error(String),
}

/// Reasoning / thinking block. Live turns stream `text`; once the phase ends
/// the UI collapses to "Thought for N sec" using [`Self::elapsed_secs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThoughtTurn {
    pub text: String,
    /// Set when the thinking phase ends (live session). History loads leave
    /// this `None` → collapsed "Thought" without a duration.
    pub elapsed_secs: Option<u32>,
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
    /// Re-read the transcript tail from disk (optional live watch).
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
    /// `session/set_mode` — permission mode id (e.g. `bypassPermissions`).
    SetPermissionMode {
        mode_id: String,
    },
    /// Reasoning effort (`low` / `medium` / `high` …) via `session/set_mode`.
    SetEffort {
        effort_id: String,
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
