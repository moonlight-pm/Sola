//! Pane / tab status vocabulary.
//!
//! Hooks (Grok, Codex, and Claude) and OSC 9999 write this. Process-tree
//! only names *who*. Never infer from OSC 0/2 titles.
//!
//! Codex sessions hosted by the app-server daemon do not deliver those
//! hooks into the pane (the daemon is not that pane's process). For a
//! pane whose process tree is Codex, `task_started` / `task_complete` in
//! that cwd's rollout jsonl raise working / done. That is the same file
//! the compaction count already reads.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sola_bus::topics::Activity;

use crate::hooks::Incoming;
use crate::machine::{Agent, Entry, Machine};
use crate::presence::Presence;
use sola_terminal::osc9999::{OscState, OscStatus};

/// What a pane (or the tab roll-up) is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Working,
    Waiting,
    Done,
    #[default]
    Idle,
}

impl AgentStatus {

    /// The mark on a project window's row in the shell's tab bar: only
    /// working and waiting show there; done reads as idle.
    pub fn activity(self) -> Activity {
        match self {
            Self::Working => Activity::Working,
            Self::Waiting => Activity::Waiting,
            Self::Done | Self::Idle => Activity::Idle,
        }
    }

    /// Tab row: waiting (needs attention) beats working beats
    /// done beats idle. Shell panes are filtered out by [`rollup_tracked`].
    pub fn rollup(statuses: impl IntoIterator<Item = Self>) -> Self {
        let mut best = Self::Idle;
        for s in statuses {
            best = match (best, s) {
                (Self::Waiting, _) | (_, Self::Waiting) => Self::Waiting,
                (Self::Working, _) | (_, Self::Working) => Self::Working,
                (Self::Done, _) | (_, Self::Done) => Self::Done,
                _ => Self::Idle,
            };
        }
        best
    }
}

/// Mark for a tab tab: Grok, Codex, and Claude panes. A sibling
/// shell stays off the disc even if it still holds a leftover status.
pub fn rollup_tracked<'a>(panes: impl IntoIterator<Item = &'a PaneStatus>) -> AgentStatus {
    AgentStatus::rollup(
        panes
            .into_iter()
            .filter(|p| p.is_tracked())
            .map(|p| p.status),
    )
}

/// Agent name for desk cards / `tab.agent`: the tracked pane whose
/// status matches the roll-up (waiting first).
pub fn loudest_agent<'a>(panes: impl IntoIterator<Item = &'a PaneStatus>) -> Option<String> {
    let panes: Vec<&PaneStatus> = panes.into_iter().filter(|p| p.is_tracked()).collect();
    let rolled = AgentStatus::rollup(panes.iter().map(|p| p.status));
    panes
        .iter()
        .find(|p| p.status == rolled)
        .and_then(|p| p.agent.clone())
        .or_else(|| panes.first().and_then(|p| p.agent.clone()))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaneStatus {
    pub status: AgentStatus,
    pub agent: Option<String>,
    pub tool: Option<String>,
    pub prompt: Option<String>,
    /// First live session that claimed this pane. A child CLI inheriting
    /// `SOLA_PANE_ID` carries a different session id and must not `done` us.
    pub owner_session: Option<String>,
    pub restored_unconfirmed: bool,
    /// Compaction count for `owner_session` (Grok session dir or Codex
    /// rollout `type: compacted` records).
    pub compaction_count: u32,
    /// Last Codex rollout length counted — presence ticks skip a rescan
    /// of a multi-megabyte jsonl when the file has not grown.
    pub codex_scan_len: u64,
    /// Offset in the bound rollout already folded into [`Self::codex_turn_open`].
    pub codex_phase_len: u64,
    /// `Some(true)` while a Codex turn is open. `None` until a
    /// `task_started` or `task_complete` marker has been seen.
    pub codex_turn_open: Option<bool>,
    /// Last Claude transcript length counted. Presence ticks skip a
    /// rescan when the jsonl has not grown.
    pub claude_scan_len: u64,
}

impl PaneStatus {
    pub fn apply_hook(&mut self, incoming: &Incoming) {
        let sid = incoming.mapped.session_id.as_deref();
        let who = if incoming.agent.is_empty() {
            "grok".to_string()
        } else {
            incoming.agent.clone()
        };
        // SessionStart / UserPromptSubmit are lead events for the agent
        // in this pane. They must reclaim after `/new`, `grok -r`, or a
        // child CLI that inherited SOLA_PANE_ID — otherwise the mark
        // freezes on the previous session. Grok does not fire those
        // events for a subagent's own session.
        if incoming.mapped.claim || incoming.mapped.clear_turn {
            if let Some(sid) = incoming.mapped.session_id.clone() {
                self.bind_owner(sid);
            }
            // SessionStart names this pane even before presence ticks.
            self.agent = Some(who.clone());
        } else if self.is_foreign(sid) {
            return;
        } else if let Some(sid) = incoming.mapped.session_id.clone() {
            if self.owner_session.is_none() {
                self.bind_owner(sid);
            }
        }
        if incoming.mapped.clear_turn {
            self.tool = None;
            if incoming.mapped.status.is_none() && !incoming.mapped.compacted {
                // SessionStart / grok -r: at the prompt, not mid-turn.
                // Do not keep a leftover Working ring from the old session.
                self.status = AgentStatus::Idle;
                self.restored_unconfirmed = false;
                return;
            }
        }
        if incoming.mapped.compacted {
            self.agent = Some(who.clone());
        }
        if incoming.mapped.compacted && incoming.mapped.status.is_none() {
            return;
        }
        let Some(status) = incoming.mapped.status else {
            return;
        };
        self.status = status;
        self.restored_unconfirmed = false;
        self.agent = Some(who);
        if let Some(tool) = &incoming.mapped.tool {
            self.tool = Some(tool.clone());
        }
        if let Some(prompt) = &incoming.mapped.prompt {
            self.prompt = Some(prompt.clone());
        }
        if status == AgentStatus::Done {
            self.tool = None;
        }
        // Owner session ended — the next lead event (or first hook)
        // may claim. Child SessionEnd is dropped in map_grok.
        if incoming.mapped.session_end {
            self.owner_session = None;
        }
    }

    pub fn apply_osc(&mut self, osc: &OscStatus) {
        self.status = match osc.state {
            OscState::Working => AgentStatus::Working,
            OscState::Waiting => AgentStatus::Waiting,
            OscState::Done => AgentStatus::Done,
            OscState::Idle => AgentStatus::Idle,
        };
        self.restored_unconfirmed = false;
        if let Some(agent) = &osc.agent_type {
            self.agent = Some(agent.clone());
        }
        if let Some(tool) = &osc.tool_name {
            self.tool = Some(tool.clone());
        }
        if let Some(prompt) = &osc.prompt {
            self.prompt = Some(prompt.clone());
        }
        if self.status == AgentStatus::Done {
            self.tool = None;
        }
    }

    /// Refresh `compaction_count` from the pane's session artifacts.
    ///
    /// Only a Grok, Codex, or Claude pane gets a count — a sibling shell
    /// must not inherit the newest session under this cwd. Grok: session
    /// dir segments/checkpoints, then `signals.json` `compactionCount`
    /// (that field often stays 0). Codex: `type: compacted` records in
    /// `~/.codex/sessions/**/rollout-*-{session_id}.jsonl`. Claude:
    /// `subtype: compact_boundary` in
    /// `~/.claude/projects/<encoded-cwd>/<session_id>.jsonl`. No Grok or
    /// Claude owner session yet → newest session under this cwd. Codex
    /// needs `owner_session`; a missed file keeps the hook-incremented
    /// count.
    pub fn refresh_compaction(&mut self, m: &Machine, cwd: &Path) {
        if !self.shows_compaction() {
            self.compaction_count = 0;
            self.codex_scan_len = 0;
            self.claude_scan_len = 0;
            return;
        }
        if self.is_grok() {
            if let Some(n) = read_compaction_count(m, cwd, self.owner_session.as_deref()) {
                self.compaction_count = n;
            }
            return;
        }
        if self.is_codex() {
            if let Some((n, len)) = read_codex_compaction_in(
                m,
                &m.agent_home(Agent::Codex),
                self.owner_session.as_deref(),
                self.codex_scan_len,
                self.compaction_count,
            ) {
                self.compaction_count = n;
                self.codex_scan_len = len;
            }
            return;
        }
        if self.is_claude() {
            if let Some((n, len)) = read_claude_compaction_in(
                m,
                &m.agent_home(Agent::Claude),
                cwd,
                self.owner_session.as_deref(),
                self.claude_scan_len,
                self.compaction_count,
            ) {
                self.compaction_count = n;
                self.claude_scan_len = len;
            }
        }
    }

    fn bind_owner(&mut self, sid: String) {
        if self.owner_session.as_deref() != Some(sid.as_str()) {
            self.codex_scan_len = 0;
            self.codex_phase_len = 0;
            self.codex_turn_open = None;
            self.claude_scan_len = 0;
        }
        self.owner_session = Some(sid);
    }

    pub fn is_grok(&self) -> bool {
        self.agent.as_deref() == Some("grok")
    }

    pub fn is_codex(&self) -> bool {
        self.agent.as_deref() == Some("codex")
    }

    pub fn is_claude(&self) -> bool {
        self.agent.as_deref() == Some("claude")
    }

    /// Bind this Codex pane to the live rollout for `cwd` and raise
    /// working/done from `task_started` / `task_complete`.
    ///
    /// `taken` is session ids already bound to other panes. Our own
    /// `owner_session` may be in that set; it is not skipped.
    ///
    /// Waiting (a hook asked the user something) is left alone while the
    /// turn is open, and becomes done when the turn closes. Returns true
    /// when status or the bound session changed.
    pub fn observe_codex_turn(&mut self, m: &Machine, cwd: &Path, taken: &HashSet<String>) -> bool {
        self.observe_codex_turn_in(m, &m.agent_home(Agent::Codex), cwd, taken)
    }

    fn observe_codex_turn_in(
        &mut self,
        m: &Machine,
        home: &Path,
        cwd: &Path,
        taken: &HashSet<String>,
    ) -> bool {
        if !self.is_codex() {
            return false;
        }
        let Some((sid, path)) =
            locate_codex_rollout(m, home, cwd, self.owner_session.as_deref(), taken)
        else {
            return false;
        };
        let before = (self.status, self.owner_session.clone());
        if self.owner_session.as_deref() != Some(sid.as_str()) {
            self.owner_session = Some(sid);
            self.codex_phase_len = 0;
            self.codex_turn_open = None;
            self.codex_scan_len = 0;
        }
        let len = m.stat(&path).map(|e| e.len).unwrap_or(0);
        let start = if self.codex_phase_len > len {
            self.codex_turn_open = None;
            0
        } else {
            self.codex_phase_len
        };
        if let Some((open, pos)) = fold_codex_turn(m, &path, start, self.codex_turn_open) {
            self.codex_turn_open = open;
            self.codex_phase_len = pos;
        }
        if let Some(open) = self.codex_turn_open {
            self.status = if open {
                if self.status == AgentStatus::Waiting {
                    AgentStatus::Waiting
                } else {
                    AgentStatus::Working
                }
            } else if self.status == AgentStatus::Waiting
                || self.status == AgentStatus::Working
                || self.status == AgentStatus::Idle
            {
                AgentStatus::Done
            } else {
                self.status
            };
        }
        before != (self.status, self.owner_session.clone())
    }

    /// Session to resume after a lost tmux. Grok (`grok -r`) or Claude
    /// (`claude --resume`). Needs that agent still named on the pane
    /// (exited-to-shell is not a resume) and a non-empty id.
    pub fn resume_session_id(&self) -> Option<&str> {
        if !self.is_grok() && !self.is_claude() {
            return None;
        }
        self.owner_session
            .as_deref()
            .filter(|s| !s.is_empty() && is_session_id(s))
    }

    pub fn is_tracked(&self) -> bool {
        self.agent
            .as_deref()
            .is_some_and(crate::cli::is_first_class)
    }

    fn shows_compaction(&self) -> bool {
        self.is_grok() || self.is_codex() || self.is_claude()
    }

    /// Presence names who is here. It never *raises* working/waiting/done
    /// (hooks/OSC own those). When the process tree is a shell, the mark
    /// returns to idle — SessionEnd leaves `done`, and that must not stick
    /// after `/exit` back to the prompt.
    pub fn apply_presence(&mut self, who: Presence) {
        match who {
            Presence::Unknown => {}
            Presence::Agent(name) => self.agent = Some(name.to_string()),
            Presence::Shell => {
                self.agent = None;
                self.tool = None;
                self.status = AgentStatus::Idle;
                self.restored_unconfirmed = false;
            }
        }
    }

    fn is_foreign(&self, session: Option<&str>) -> bool {
        match (&self.owner_session, session) {
            (Some(owner), Some(sid)) => owner != sid,
            _ => false,
        }
    }
}

pub fn encode_session_cwd(path: &Path) -> String {
    path.to_string_lossy()
        .bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                vec![b as char]
            } else {
                format!("%{b:02X}").chars().collect()
            }
        })
        .collect()
}

pub fn encode_claude_project(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub fn grok_session_exists(m: &Machine, session_id: &str) -> bool {
    grok_session_exists_in(m, &m.agent_home(Agent::Grok), session_id)
}

/// `sessions/<group>/<session_id>/` is there.
fn grok_session_exists_in(m: &Machine, home: &Path, session_id: &str) -> bool {
    is_session_id(session_id) && m.find(&home.join("sessions"), 2, 2, true, session_id).is_some()
}

fn is_session_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains('\0')
        && !s.contains('*')
}

pub fn read_compaction_count(m: &Machine, cwd: &Path, session_id: Option<&str>) -> Option<u32> {
    read_compaction_count_in(m, &m.agent_home(Agent::Grok), cwd, session_id)
}

fn read_compaction_count_in(
    m: &Machine,
    home: &Path,
    cwd: &Path,
    session_id: Option<&str>,
) -> Option<u32> {
    let group = home.join("sessions").join(encode_session_cwd(cwd));
    // Each session folder, and the files in it (its signals.json, its
    // compaction folders): one look.
    let tree = m.walk(&group, 3);
    let sid = session_id
        .filter(|s| !s.is_empty() && tree.iter().any(|e| e.dir && e.name == *s))
        .map(|s| s.to_string())
        .or_else(|| newest_session_id(&tree))?;
    let signals = m
        .read_to_string(&group.join(&sid).join("signals.json"))
        .and_then(|t| parse_compaction_count(&t))
        .unwrap_or(0);
    let files_in = |sub: &str, pred: &dyn Fn(&str) -> bool| {
        let prefix = format!("{sid}/{sub}/");
        tree.iter()
            .filter(|e| e.file)
            .filter_map(|e| e.name.strip_prefix(&prefix))
            .filter(|n| !n.contains('/') && pred(n))
            .count() as u32
    };
    let segments = files_in("compaction", &|n| n.starts_with("segment_") && n.ends_with(".md"));
    let checkpoints = files_in("compaction_checkpoints", &|_| true);
    Some(signals.max(segments).max(checkpoints))
}

/// The session folder (of a group's walk) whose `signals.json` (else
/// itself) changed last.
fn newest_session_id(tree: &[Entry]) -> Option<String> {
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for dir in tree.iter().filter(|e| e.dir && !e.name.contains('/')) {
        if dir.name.starts_with('.') {
            continue;
        }
        let signals = format!("{}/signals.json", dir.name);
        let t = tree
            .iter()
            .find(|e| e.name == signals)
            .map(|e| e.modified)
            .unwrap_or(dir.modified);
        if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
            best = Some((t, dir.name.clone()));
        }
    }
    best.map(|(_, n)| n)
}

fn parse_compaction_count(text: &str) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    v.get("compactionCount")
        .or_else(|| v.get("compaction_count"))
        .and_then(|n| n.as_u64())
        .map(|n| n as u32)
}

fn read_codex_compaction_in(
    m: &Machine,
    home: &Path,
    session_id: Option<&str>,
    last_len: u64,
    last_count: u32,
) -> Option<(u32, u64)> {
    let sid = session_id.filter(|s| is_session_id(s))?;
    let path = find_codex_rollout(m, home, sid)?;
    let len = m.stat(&path)?.len;
    if len == last_len && last_count > 0 {
        return Some((last_count, len));
    }
    Some((count_codex_compactions(m, &path), len))
}

pub fn codex_rollout_exists(m: &Machine, session_id: &str) -> bool {
    find_codex_rollout(m, &m.agent_home(Agent::Codex), session_id).is_some()
}

fn find_codex_rollout(m: &Machine, home: &Path, session_id: &str) -> Option<PathBuf> {
    if !is_session_id(session_id) {
        return None;
    }
    let glob = format!("rollout-*-{session_id}.jsonl");
    m.find(&home.join("sessions"), 1, 5, false, &glob)
}

fn locate_codex_rollout(
    m: &Machine,
    home: &Path,
    cwd: &Path,
    owner: Option<&str>,
    taken: &HashSet<String>,
) -> Option<(String, PathBuf)> {
    let mut skip = taken.clone();
    if let Some(sid) = owner {
        skip.remove(sid);
    }
    if let Some(found) = newest_codex_rollout_for_cwd(m, home, cwd, &skip) {
        return Some(found);
    }
    let sid = owner.filter(|s| is_session_id(s))?;
    let path = find_codex_rollout(m, home, sid)?;
    Some((sid.to_string(), path))
}

/// The newest rollout (by change) that names `cwd`, not one in `skip`.
fn newest_codex_rollout_for_cwd(
    m: &Machine,
    home: &Path,
    cwd: &Path,
    skip: &HashSet<String>,
) -> Option<(String, PathBuf)> {
    let sessions = home.join("sessions");
    let mut rollouts: Vec<(std::time::SystemTime, String, PathBuf)> = m
        .walk(&sessions, 8)
        .into_iter()
        .filter(|e| e.file)
        .filter_map(|e| {
            let name = e.name.rsplit('/').next()?;
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                return None;
            }
            let sid = session_id_from_rollout_name(name)?;
            (!skip.contains(&sid)).then(|| (e.modified, sid, sessions.join(&e.name)))
        })
        .collect();
    rollouts.sort_by(|a, b| b.0.cmp(&a.0));
    rollouts
        .into_iter()
        .find(|(_, _, path)| rollout_cwd_matches(m, path, cwd))
        .map(|(_, sid, path)| (sid, path))
}

fn session_id_from_rollout_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let t = rest.find('T')?;
    let after_t = &rest[t + 1..];
    let sid = after_t.get(9..)?;
    if sid.is_empty() || !is_session_id(sid) {
        return None;
    }
    Some(sid.to_string())
}

fn rollout_cwd_matches(m: &Machine, path: &Path, cwd: &Path) -> bool {
    let Some(head) = m.read_head(path, 4096) else {
        return false;
    };
    let Ok(head) = std::str::from_utf8(&head) else {
        return false;
    };
    let want = cwd.to_string_lossy();
    let quoted = format!("\"cwd\":\"{want}\"");
    let spaced = format!("\"cwd\": \"{want}\"");
    head.contains(&quoted) || head.contains(&spaced)
}

/// Fold `task_started` / `task_complete` from `start` to the last complete
/// line. A trailing partial line is left for the next tick (`pos` stops
/// before it).
fn fold_codex_turn(
    m: &Machine,
    path: &Path,
    start: u64,
    mut open: Option<bool>,
) -> Option<(Option<bool>, u64)> {
    let read = m.line_heads(path, start, 240)?;
    for head in &read.heads {
        open = note_turn_marker(head, open);
    }
    Some((open, read.end))
}

fn note_turn_marker(head: &[u8], open: Option<bool>) -> Option<bool> {
    let end = head
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(head.len());
    let Ok(s) = std::str::from_utf8(&head[..end]) else {
        return open;
    };
    if s.contains(r#""payload":{"type":"task_started""#)
        || s.contains(r#""payload": {"type": "task_started""#)
    {
        Some(true)
    } else if s.contains(r#""payload":{"type":"task_complete""#)
        || s.contains(r#""payload": {"type": "task_complete""#)
    {
        Some(false)
    } else {
        open
    }
}

/// Count jsonl records whose top-level `type` is `compacted`.
/// Compact payloads are huge; only the first bytes of each line are
/// inspected ([`Machine::line_heads`]).
fn count_codex_compactions(m: &Machine, path: &Path) -> u32 {
    m.line_heads(path, 0, 192)
        .map(|read| read.heads.iter().filter(|h| line_is_codex_compacted(h)).count() as u32)
        .unwrap_or(0)
}

fn line_is_codex_compacted(head: &[u8]) -> bool {
    let end = head
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(head.len());
    let Ok(s) = std::str::from_utf8(&head[..end]) else {
        return false;
    };
    s.contains(r#""type":"compacted""#) || s.contains(r#""type": "compacted""#)
}

/// True when `~/.claude/projects/*/<session_id>.jsonl` exists.
pub fn claude_session_exists(m: &Machine, session_id: &str) -> bool {
    find_claude_transcript(m, &m.agent_home(Agent::Claude), session_id).is_some()
}

fn find_claude_transcript(m: &Machine, home: &Path, session_id: &str) -> Option<PathBuf> {
    if !is_session_id(session_id) {
        return None;
    }
    m.find(&home.join("projects"), 2, 2, false, &format!("{session_id}.jsonl"))
}

fn newest_claude_transcript(m: &Machine, home: &Path, cwd: &Path) -> Option<PathBuf> {
    let dir = home.join("projects").join(encode_claude_project(cwd));
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for e in m.list(&dir)? {
        if !e.file {
            continue;
        }
        let Some(sid) = e.name.strip_suffix(".jsonl") else {
            continue;
        };
        if !is_session_id(sid) {
            continue;
        }
        let newer = best.as_ref().map(|(t, _)| e.modified > *t).unwrap_or(true);
        if newer {
            best = Some((e.modified, e.name.clone()));
        }
    }
    best.map(|(_, name)| dir.join(name))
}

fn claude_transcript_for(
    m: &Machine,
    home: &Path,
    cwd: &Path,
    session_id: Option<&str>,
) -> Option<PathBuf> {
    if let Some(sid) = session_id.filter(|s| is_session_id(s)) {
        return find_claude_transcript(m, home, sid);
    }
    newest_claude_transcript(m, home, cwd)
}

fn read_claude_compaction_in(
    m: &Machine,
    home: &Path,
    cwd: &Path,
    session_id: Option<&str>,
    last_len: u64,
    last_count: u32,
) -> Option<(u32, u64)> {
    let path = claude_transcript_for(m, home, cwd, session_id)?;
    let len = m.stat(&path)?.len;
    if len == last_len && last_len > 0 {
        return Some((last_count, len));
    }
    Some((count_claude_compactions(m, &path), len))
}

/// `compact_boundary` rows, or `isCompactSummary` when an older transcript
/// has no boundary. Both on the same compaction would double-count.
fn count_claude_compactions(m: &Machine, path: &Path) -> u32 {
    let Some(read) = m.line_heads(path, 0, 384) else {
        return 0;
    };
    let (mut boundaries, mut summaries) = (0u32, 0u32);
    for head in &read.heads {
        match line_claude_compact_kind(head) {
            2 => boundaries += 1,
            1 => summaries += 1,
            _ => {}
        }
    }
    if boundaries > 0 {
        boundaries
    } else {
        summaries
    }
}

/// 2 = `compact_boundary`, 1 = compact summary, 0 = neither.
fn line_claude_compact_kind(head: &[u8]) -> u8 {
    let end = head
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(head.len());
    let Ok(s) = std::str::from_utf8(&head[..end]) else {
        return 0;
    };
    if s.contains(r#""subtype":"compact_boundary""#)
        || s.contains(r#""subtype": "compact_boundary""#)
    {
        return 2;
    }
    if s.contains(r#""isCompactSummary":true"#) || s.contains(r#""isCompactSummary": true"#) {
        return 1;
    }
    0
}

fn last_status_path() -> PathBuf {
    crate::paths::config_dir().join("last-status.json")
}

#[derive(Serialize, Deserialize)]
struct DiskPane {
    status: AgentStatus,
    agent: Option<String>,
    /// Grok or Claude `owner_session`. Resume `grok -r` or
    /// `claude --resume` after a lost tmux.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct DiskSnapshot {
    #[serde(default)]
    panes: std::collections::HashMap<String, DiskPane>,
    /// Pre-multi-pane shape. Read on hydrate; never written.
    #[serde(default)]
    pane_id: Option<String>,
    #[serde(default)]
    status: Option<AgentStatus>,
    #[serde(default)]
    agent: Option<String>,
}

fn read_snapshot_from(path: &Path) -> Option<DiskSnapshot> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn persist_all(panes: &std::collections::HashMap<String, PaneStatus>) {
    persist_all_to(&last_status_path(), panes);
}

fn persist_all_to(path: &Path, panes: &std::collections::HashMap<String, PaneStatus>) {
    let mut snap = DiskSnapshot {
        panes: std::collections::HashMap::new(),
        pane_id: None,
        status: None,
        agent: None,
    };
    for (id, pane) in panes {
        snap.panes.insert(
            id.clone(),
            DiskPane {
                status: pane.status,
                agent: pane.agent.clone(),
                session_id: pane.owner_session.clone().filter(|s| is_session_id(s)),
            },
        );
    }
    write_snapshot_to(path, &snap);
}

fn write_snapshot_to(path: &Path, snap: &DiskSnapshot) {
    if let Ok(text) = serde_json::to_string_pretty(snap) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

/// Hydrate last hook status. Caller must mark unconfirmed and not toast.
pub fn hydrate(pane_id: &str) -> Option<PaneStatus> {
    hydrate_from(&last_status_path(), pane_id)
}

fn hydrate_from(path: &Path, pane_id: &str) -> Option<PaneStatus> {
    let snap = read_snapshot_from(path)?;
    if let Some(p) = snap.panes.get(pane_id) {
        return Some(from_disk_pane(p));
    }
    if snap.pane_id.as_deref() == Some(pane_id) {
        let status = snap.status.unwrap_or_default();
        return Some(PaneStatus {
            status,
            agent: snap.agent,
            restored_unconfirmed: status != AgentStatus::Idle,
            ..PaneStatus::default()
        });
    }
    None
}

fn from_disk_pane(p: &DiskPane) -> PaneStatus {
    PaneStatus {
        status: p.status,
        agent: p.agent.clone(),
        owner_session: p.session_id.clone().filter(|s| is_session_id(s)),
        restored_unconfirmed: p.status != AgentStatus::Idle,
        ..PaneStatus::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::hooks::map::MappedHook;
    use crate::hooks::event::Incoming;

    fn hook(session: &str, status: AgentStatus) -> Incoming {
        Incoming {
            host: None,
            pane_id: "p".into(),
            cwd: None,
            ask: None,
            transcript: None,
            agent: "grok".into(),
            mapped: MappedHook {
                status: Some(status),
                clear_turn: false,
                claim: false,
                session_end: false,
                compacted: false,
                prompt: None,
                tool: None,
                session_id: Some(session.into()),
            },
        }
    }

    fn session_start(session: &str) -> Incoming {
        Incoming {
            host: None,
            pane_id: "p".into(),
            cwd: None,
            ask: None,
            transcript: None,
            agent: "grok".into(),
            mapped: MappedHook {
                status: None,
                clear_turn: true,
                claim: true,
                session_end: false,
                compacted: false,
                prompt: None,
                tool: None,
                session_id: Some(session.into()),
            },
        }
    }

    fn prompt_submit(session: &str) -> Incoming {
        Incoming {
            host: None,
            pane_id: "p".into(),
            cwd: None,
            ask: None,
            transcript: None,
            agent: "grok".into(),
            mapped: MappedHook {
                status: Some(AgentStatus::Working),
                clear_turn: false,
                claim: true,
                session_end: false,
                compacted: false,
                prompt: Some("hi".into()),
                tool: None,
                session_id: Some(session.into()),
            },
        }
    }

    fn session_end(session: &str) -> Incoming {
        Incoming {
            host: None,
            pane_id: "p".into(),
            cwd: None,
            ask: None,
            transcript: None,
            agent: "grok".into(),
            mapped: MappedHook {
                status: Some(AgentStatus::Done),
                clear_turn: false,
                claim: false,
                session_end: true,
                compacted: false,
                prompt: None,
                tool: None,
                session_id: Some(session.into()),
            },
        }
    }

    #[test]
    fn encode_cwd_matches_grok_sessions() {
        assert_eq!(
            encode_session_cwd(Path::new("/home/joshua/Workspace/Sola")),
            "%2Fhome%2Fjoshua%2FWorkspace%2FSola"
        );
    }

    #[test]
    fn parse_compaction_count_from_signals() {
        assert_eq!(parse_compaction_count(r#"{"compactionCount":3}"#), Some(3));
        assert_eq!(parse_compaction_count(r#"{"compaction_count":2}"#), Some(2));
        assert_eq!(parse_compaction_count(r#"{}"#), None);
    }

    fn write_session(
        home: &Path,
        cwd: &Path,
        sid: &str,
        signals: &str,
        segments: u32,
        checkpoints: u32,
    ) {
        let dir = home
            .join("sessions")
            .join(encode_session_cwd(cwd))
            .join(sid);
        std::fs::create_dir_all(dir.join("compaction")).unwrap();
        std::fs::create_dir_all(dir.join("compaction_checkpoints")).unwrap();
        std::fs::write(dir.join("signals.json"), signals).unwrap();
        for i in 0..segments {
            std::fs::write(
                dir.join("compaction").join(format!("segment_{i:03}.md")),
                "x",
            )
            .unwrap();
        }
        for i in 0..checkpoints {
            std::fs::write(
                dir.join("compaction_checkpoints").join(format!("{i}.json")),
                "{}",
            )
            .unwrap();
        }
    }

    #[test]
    fn compaction_count_uses_segments_when_signals_zero() {
        let root = std::env::temp_dir().join(format!(
            "sola-ws-compact-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cwd = Path::new("/home/joshua/Workspace/Sola/.worktrees/workspaces-polish");
        write_session(&root, cwd, "sid-a", r#"{"compactionCount":0}"#, 1, 1);
        assert_eq!(read_compaction_count_in(&Machine::Local, &root, cwd, Some("sid-a")), Some(1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compaction_count_prefers_higher_signals() {
        let root = std::env::temp_dir().join(format!(
            "sola-ws-compact-hi-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cwd = Path::new("/tmp/proj");
        write_session(&root, cwd, "sid-b", r#"{"compactionCount":8}"#, 4, 4);
        assert_eq!(read_compaction_count_in(&Machine::Local, &root, cwd, Some("sid-b")), Some(8));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compaction_count_falls_back_to_newest_session() {
        let root = std::env::temp_dir().join(format!(
            "sola-ws-compact-new-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cwd = Path::new("/tmp/proj");
        write_session(&root, cwd, "old", r#"{"compactionCount":3}"#, 0, 0);
        std::thread::sleep(std::time::Duration::from_millis(10));
        write_session(&root, cwd, "new", r#"{"compactionCount":0}"#, 2, 2);
        assert_eq!(read_compaction_count_in(&Machine::Local, &root, cwd, None), Some(2));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rollup_waiting_beats_working() {
        assert_eq!(
            AgentStatus::rollup([AgentStatus::Idle, AgentStatus::Done, AgentStatus::Working]),
            AgentStatus::Working
        );
        assert_eq!(
            AgentStatus::rollup([AgentStatus::Working, AgentStatus::Waiting]),
            AgentStatus::Waiting
        );
        assert_eq!(
            AgentStatus::rollup([AgentStatus::Done, AgentStatus::Waiting]),
            AgentStatus::Waiting
        );
        assert_eq!(
            AgentStatus::rollup([AgentStatus::Idle, AgentStatus::Done]),
            AgentStatus::Done
        );
        assert_eq!(AgentStatus::rollup([]), AgentStatus::Idle);
    }

    #[test]
    fn only_working_and_waiting_mark_the_bar() {
        assert_eq!(AgentStatus::Waiting.activity(), Activity::Waiting);
        assert_eq!(AgentStatus::Working.activity(), Activity::Working);
        assert_eq!(AgentStatus::Done.activity(), Activity::Idle);
        assert_eq!(AgentStatus::Idle.activity(), Activity::Idle);
    }

    #[test]
    fn rollup_grok_ignores_shell() {
        let grok = PaneStatus {
            status: AgentStatus::Done,
            agent: Some("grok".into()),
            ..PaneStatus::default()
        };
        let shell = PaneStatus {
            status: AgentStatus::Working,
            ..PaneStatus::default()
        };
        let other = PaneStatus {
            status: AgentStatus::Waiting,
            agent: Some("opencode".into()),
            ..PaneStatus::default()
        };
        assert_eq!(rollup_tracked([&grok, &shell, &other]), AgentStatus::Done);
        let claude = PaneStatus {
            status: AgentStatus::Waiting,
            agent: Some("claude".into()),
            ..PaneStatus::default()
        };
        assert_eq!(
            rollup_tracked([&grok, &shell, &claude]),
            AgentStatus::Waiting
        );
        let codex = PaneStatus {
            status: AgentStatus::Waiting,
            agent: Some("codex".into()),
            ..PaneStatus::default()
        };
        assert_eq!(
            rollup_tracked([&grok, &shell, &codex]),
            AgentStatus::Waiting
        );
        assert_eq!(loudest_agent([&grok, &codex]).as_deref(), Some("codex"));
        let working = PaneStatus {
            status: AgentStatus::Working,
            agent: Some("grok".into()),
            ..PaneStatus::default()
        };
        let waiting = PaneStatus {
            status: AgentStatus::Waiting,
            agent: Some("grok".into()),
            ..PaneStatus::default()
        };
        assert_eq!(
            rollup_tracked([&working, &waiting, &shell]),
            AgentStatus::Waiting
        );
        assert_eq!(rollup_tracked([&shell]), AgentStatus::Idle);
    }

    #[test]
    fn claude_project_encoding_and_compaction_count() {
        assert_eq!(
            encode_claude_project(Path::new("/home/joshua/Workspace/Exit")),
            "-home-joshua-Workspace-Exit"
        );
        assert_eq!(
            encode_claude_project(Path::new(
                "/home/joshua/Workspace/Sola/.worktrees/sola-workspaces"
            )),
            "-home-joshua-Workspace-Sola--worktrees-sola-workspaces"
        );
        let root = std::env::temp_dir().join(format!(
            "sola-ws-claude-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cwd = Path::new("/tmp/proj");
        let dir = root.join("projects").join(encode_claude_project(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        let sid = "11111111-2222-3333-4444-555555555555";
        let path = dir.join(format!("{sid}.jsonl"));
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"user","message":{"content":"hi"}}"#,
                "\n",
                r#"{"type":"system","subtype":"compact_boundary","compactMetadata":{"trigger":"auto"}}"#,
                "\n",
                r#"{"type":"user","isCompactSummary":true,"message":{"content":"summary"}}"#,
                "\n",
                r#"{"parentUuid":"abc","type":"system","subtype":"compact_boundary"}"#,
                "\n",
            ),
        )
        .unwrap();
        assert!(find_claude_transcript(&Machine::Local, &root, sid).is_some());
        assert_eq!(count_claude_compactions(&Machine::Local, &path), 2);
        let (n, len) = read_claude_compaction_in(&Machine::Local, &root, cwd, Some(sid), 0, 0).unwrap();
        assert_eq!(n, 2);
        assert!(len > 0);
        let (cached, _) = read_claude_compaction_in(&Machine::Local, &root, cwd, Some(sid), len, n).unwrap();
        assert_eq!(cached, n);
        let (newest, _) = read_claude_compaction_in(&Machine::Local, &root, cwd, None, 0, 0).unwrap();
        assert_eq!(newest, 2);
        assert!(read_claude_compaction_in(&Machine::Local, &root, cwd, Some("missing-session"), 0, 0).is_none());
        let old = dir.join("older-session.jsonl");
        std::fs::write(&old, "{\"type\":\"user\",\"isCompactSummary\":true}\n").unwrap();
        assert_eq!(count_claude_compactions(&Machine::Local, &old), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn child_session_cannot_done_parent() {
        let mut pane = PaneStatus::default();
        pane.apply_hook(&hook("owner", AgentStatus::Working));
        pane.apply_hook(&hook("child", AgentStatus::Done));
        assert_eq!(pane.status, AgentStatus::Working);
        pane.apply_hook(&hook("owner", AgentStatus::Done));
        assert_eq!(pane.status, AgentStatus::Done);
    }

    #[test]
    fn session_start_reclaims_after_rotation() {
        let mut pane = PaneStatus::default();
        pane.apply_hook(&hook("old", AgentStatus::Working));
        pane.apply_hook(&session_start("new"));
        assert_eq!(pane.owner_session.as_deref(), Some("new"));
        assert_eq!(pane.agent.as_deref(), Some("grok"));
        assert_eq!(pane.status, AgentStatus::Idle);
        pane.apply_hook(&hook("new", AgentStatus::Waiting));
        assert_eq!(pane.status, AgentStatus::Waiting);
        pane.apply_hook(&hook("old", AgentStatus::Done));
        assert_eq!(pane.status, AgentStatus::Waiting);
    }

    #[test]
    fn user_prompt_reclaims_after_rotation() {
        let mut pane = PaneStatus::default();
        pane.apply_hook(&hook("old", AgentStatus::Done));
        pane.apply_hook(&prompt_submit("new"));
        assert_eq!(pane.owner_session.as_deref(), Some("new"));
        assert_eq!(pane.status, AgentStatus::Working);
    }

    #[test]
    fn session_end_releases_owner() {
        let mut pane = PaneStatus::default();
        pane.apply_hook(&hook("old", AgentStatus::Working));
        pane.apply_hook(&session_end("old"));
        assert_eq!(pane.status, AgentStatus::Done);
        assert_eq!(pane.owner_session, None);
        pane.apply_hook(&hook("new", AgentStatus::Working));
        assert_eq!(pane.owner_session.as_deref(), Some("new"));
        assert_eq!(pane.status, AgentStatus::Working);
    }

    #[test]
    fn presence_does_not_set_working() {
        let mut pane = PaneStatus::default();
        pane.apply_presence(Presence::Agent("grok"));
        assert_eq!(pane.status, AgentStatus::Idle);
        assert_eq!(pane.agent.as_deref(), Some("grok"));
    }

    #[test]
    fn presence_tracks_who_is_live() {
        let mut pane = PaneStatus::default();
        pane.apply_presence(Presence::Agent("grok"));
        assert_eq!(pane.agent.as_deref(), Some("grok"));
        pane.apply_presence(Presence::Agent("claude"));
        assert_eq!(pane.agent.as_deref(), Some("claude"));
        pane.apply_presence(Presence::Shell);
        assert_eq!(pane.agent, None);
        assert_eq!(pane.status, AgentStatus::Idle);
    }

    #[test]
    fn presence_shell_idles_after_done() {
        let mut pane = PaneStatus::default();
        pane.apply_hook(&hook("owner", AgentStatus::Working));
        pane.apply_hook(&session_end("owner"));
        assert_eq!(pane.status, AgentStatus::Done);
        pane.apply_presence(Presence::Shell);
        assert_eq!(pane.status, AgentStatus::Idle);
        assert_eq!(pane.agent, None);
        assert_eq!(pane.tool, None);
    }

    #[test]
    fn presence_unknown_does_not_idle() {
        let mut pane = PaneStatus::default();
        pane.apply_hook(&hook("owner", AgentStatus::Working));
        pane.apply_presence(Presence::Unknown);
        assert_eq!(pane.status, AgentStatus::Working);
        assert_eq!(pane.agent.as_deref(), Some("grok"));
    }

    #[test]
    fn shell_pane_does_not_inherit_compaction() {
        let mut pane = PaneStatus::default();
        pane.compaction_count = 4;
        pane.refresh_compaction(&Machine::Local, Path::new("/tmp/not-a-session"));
        assert_eq!(pane.compaction_count, 0);
    }

    #[test]
    fn codex_pane_keeps_count_when_rollout_missing() {
        let mut pane = PaneStatus {
            agent: Some("codex".into()),
            owner_session: Some("01a0missing-session".into()),
            compaction_count: 4,
            ..PaneStatus::default()
        };
        pane.refresh_compaction(&Machine::Local, Path::new("/tmp/not-a-session"));
        assert_eq!(pane.compaction_count, 4);
    }

    fn write_codex_rollout(home: &Path, sid: &str, compacted: u32, decoy: bool) {
        let dir = home.join("sessions").join("2026").join("09").join("21");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("rollout-2026-09-21T12-00-00-{sid}.jsonl"));
        let mut text = format!(
            "{{\"timestamp\":\"t\",\"ordinal\":0,\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{sid}\"}}}}\n"
        );
        for i in 0..compacted {
            text.push_str(&format!(
                "{{\"timestamp\":\"t\",\"ordinal\":{},\"type\":\"compacted\",\"payload\":{{\"message\":\"\",\"replacement_history\":[]}}}}\n",
                i + 1
            ));
        }
        if decoy {
            text.push_str(
                "{\"timestamp\":\"t\",\"ordinal\":99,\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"talk about compacted context\"}}\n",
            );
        }
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn codex_compaction_counts_compacted_records() {
        let root = std::env::temp_dir().join(format!(
            "sola-ws-codex-compact-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sid = "01a0codex-count-sid";
        write_codex_rollout(&root, sid, 3, true);
        let path = find_codex_rollout(&Machine::Local, &root, sid).unwrap();
        let len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(
            read_codex_compaction_in(&Machine::Local, &root, Some(sid), 0, 0),
            Some((3, len))
        );
        // Unchanged length keeps the previous count (no rescan).
        assert_eq!(
            read_codex_compaction_in(&Machine::Local, &root, Some(sid), len, 99),
            Some((99, len))
        );
        assert_eq!(count_codex_compactions(&Machine::Local, &path), 3);
        // A megabyte-class compacted line still counts once (type is in
        // the first bytes; later `"type":"compacted"` in the payload is
        // the same record).
        let long = root.join("sessions/2026/09/21").join(format!(
            "rollout-2026-09-21T12-00-01-01a0codex-long-sid.jsonl"
        ));
        let mut long_line = String::from(
            r#"{"timestamp":"t","ordinal":1,"type":"compacted","payload":{"message":""#,
        );
        long_line.push_str(&"x".repeat(800));
        long_line.push_str(r#"","note":"\"type\":\"compacted\" decoy"}}"#);
        long_line.push('\n');
        std::fs::write(&long, long_line).unwrap();
        assert_eq!(count_codex_compactions(&Machine::Local, &long), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_rollout_turn_markers_drive_status() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("sola-codex-turn-{n}"));
        let cwd = PathBuf::from("/src/rimworld");
        let sid = "01a0f83d-53ec-74f0-a240-ff826d500d8e";
        let day = root.join("sessions/2026/10/01");
        std::fs::create_dir_all(&day).unwrap();
        let path = day.join(format!("rollout-2026-10-01T10-12-43-{sid}.jsonl"));
        let started = r#"{"timestamp":"t","ordinal":1,"type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#;
        let done = r#"{"timestamp":"t","ordinal":2,"type":"event_msg","payload":{"type":"task_complete","turn_id":"t1"}}"#;
        std::fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n{started}\n",
                cwd.display()
            ),
        )
        .unwrap();

        let mut pane = PaneStatus {
            agent: Some("codex".into()),
            ..PaneStatus::default()
        };
        let taken = HashSet::new();
        assert!(pane.observe_codex_turn_in(&Machine::Local, &root, &cwd, &taken));
        assert_eq!(pane.status, AgentStatus::Working);
        assert_eq!(pane.owner_session.as_deref(), Some(sid));
        assert!(!pane.observe_codex_turn_in(&Machine::Local, &root, &cwd, &taken));

        let mut waiting = pane.clone();
        waiting.status = AgentStatus::Waiting;
        waiting.codex_phase_len = 0;
        waiting.codex_turn_open = None;
        assert!(!waiting.observe_codex_turn_in(&Machine::Local, &root, &cwd, &taken));
        assert_eq!(waiting.status, AgentStatus::Waiting);

        std::fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n{started}\n{done}\n",
                cwd.display()
            ),
        )
        .unwrap();
        // File was rewritten shorter than the previous offset; rescan.
        assert!(pane.observe_codex_turn_in(&Machine::Local, &root, &cwd, &taken));
        assert_eq!(pane.status, AgentStatus::Done);

        let mut other = PaneStatus {
            agent: Some("codex".into()),
            ..PaneStatus::default()
        };
        let mut claimed = HashSet::new();
        claimed.insert(sid.to_string());
        assert!(!other.observe_codex_turn_in(&Machine::Local, &root, &cwd, &claimed));
        assert!(other.owner_session.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_hook_session_start_names_codex() {
        let mut pane = PaneStatus::default();
        let incoming = Incoming {
            host: None,
            pane_id: "p".into(),
            cwd: None,
            ask: None,
            transcript: None,
            agent: "codex".into(),
            mapped: MappedHook {
                status: None,
                clear_turn: true,
                claim: true,
                session_end: false,
                compacted: false,
                prompt: None,
                tool: None,
                session_id: Some("c-sid".into()),
            },
        };
        pane.apply_hook(&incoming);
        assert_eq!(pane.agent.as_deref(), Some("codex"));
        assert_eq!(pane.owner_session.as_deref(), Some("c-sid"));
        assert_eq!(pane.status, AgentStatus::Idle);
    }

    #[test]
    fn resume_session_id_needs_grok_and_id() {
        let mut pane = PaneStatus::default();
        assert_eq!(pane.resume_session_id(), None);
        pane.owner_session = Some("sid-a".into());
        assert_eq!(pane.resume_session_id(), None);
        pane.agent = Some("grok".into());
        assert_eq!(pane.resume_session_id(), Some("sid-a"));
        pane.agent = Some("claude".into());
        assert_eq!(pane.resume_session_id(), Some("sid-a"));
        pane.agent = Some("codex".into());
        assert_eq!(pane.resume_session_id(), None);
        pane.agent = None;
        pane.apply_presence(Presence::Shell);
        assert_eq!(pane.resume_session_id(), None);
        pane.agent = Some("grok".into());
        pane.owner_session = Some("../nope".into());
        assert_eq!(pane.resume_session_id(), None);
        pane.owner_session = Some("sid-a".into());
        assert_eq!(pane.resume_session_id(), Some("sid-a"));
    }

    #[test]
    fn persist_hydrate_keeps_session_id() {
        let root = std::env::temp_dir().join(format!(
            "sola-ws-last-status-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("last-status.json");
        let mut panes = std::collections::HashMap::new();
        panes.insert(
            "ws-a".into(),
            PaneStatus {
                status: AgentStatus::Done,
                agent: Some("grok".into()),
                owner_session: Some("sid-live".into()),
                ..PaneStatus::default()
            },
        );
        panes.insert(
            "ws-b".into(),
            PaneStatus {
                status: AgentStatus::Idle,
                ..PaneStatus::default()
            },
        );
        persist_all_to(&path, &panes);
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["panes"]["ws-a"]["session_id"], "sid-live");
        assert!(v["panes"]["ws-b"].get("session_id").is_none());
        let a = hydrate_from(&path, "ws-a").unwrap();
        assert_eq!(a.owner_session.as_deref(), Some("sid-live"));
        assert_eq!(a.agent.as_deref(), Some("grok"));
        assert_eq!(a.status, AgentStatus::Done);
        assert!(a.restored_unconfirmed);
        let b = hydrate_from(&path, "ws-b").unwrap();
        assert_eq!(b.owner_session, None);
        assert_eq!(b.agent, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hydrate_legacy_snapshot_without_session_id() {
        let root = std::env::temp_dir().join(format!(
            "sola-ws-last-status-legacy-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("last-status.json");
        std::fs::write(
            &path,
            r#"{"panes":{"p":{"status":"working","agent":"grok"}}}"#,
        )
        .unwrap();
        let p = hydrate_from(&path, "p").unwrap();
        assert_eq!(p.agent.as_deref(), Some("grok"));
        assert_eq!(p.owner_session, None);
        assert_eq!(p.status, AgentStatus::Working);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn grok_session_exists_walks_cwd_groups() {
        let root = std::env::temp_dir().join(format!(
            "sola-ws-sess-exists-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cwd = Path::new("/tmp/proj");
        write_session(&root, cwd, "sid-live", r#"{"compactionCount":0}"#, 0, 0);
        assert!(grok_session_exists_in(&Machine::Local, &root, "sid-live"));
        assert!(!grok_session_exists_in(&Machine::Local, &root, "missing"));
        assert!(!grok_session_exists_in(&Machine::Local, &root, "../nope"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_hook_names_codex() {
        let mut pane = PaneStatus::default();
        let mut incoming = hook("c1", AgentStatus::Working);
        incoming.agent = "codex".into();
        pane.apply_hook(&incoming);
        assert_eq!(pane.agent.as_deref(), Some("codex"));
        assert_eq!(pane.status, AgentStatus::Working);
        assert!(pane.is_tracked());
        assert!(!pane.is_grok());
    }
}
