//! Pane / workspace status vocabulary.
//!
//! Hooks (Grok and Codex) and OSC 9999 write this. Process-tree only names
//! *who*. Never infer from OSC 0/2 titles.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sola_kit::components::SidebarIndicator;

use crate::hooks::Incoming;
use crate::presence::Presence;
use sola_terminal::osc9999::{OscState, OscStatus};

/// What a pane (or the workspace roll-up) is doing.
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
    pub fn indicator(self) -> SidebarIndicator {
        match self {
            Self::Working => SidebarIndicator::Working,
            Self::Waiting => SidebarIndicator::Waiting,
            Self::Done => SidebarIndicator::Done,
            Self::Idle => SidebarIndicator::Idle,
        }
    }

    /// Workspace row: waiting (needs attention) beats working beats
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

/// Mark for a workspace tab: Grok and Codex panes. A sibling shell stays
/// off the disc even if it still holds a leftover status.
pub fn rollup_tracked<'a>(panes: impl IntoIterator<Item = &'a PaneStatus>) -> AgentStatus {
    AgentStatus::rollup(
        panes
            .into_iter()
            .filter(|p| p.is_tracked())
            .map(|p| p.status),
    )
}

/// Agent name for desk cards / `workspace.agent`: the tracked pane whose
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

/// Quiet `×N` on the workspace row: loudest Grok or Codex session in the tab.
pub fn rail_compaction<'a>(panes: impl IntoIterator<Item = &'a PaneStatus>) -> u32 {
    panes
        .into_iter()
        .filter(|p| p.shows_compaction())
        .map(|p| p.compaction_count)
        .max()
        .unwrap_or(0)
}

#[derive(Clone, Debug, Default)]
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
                self.owner_session = Some(sid);
            }
            // SessionStart names this pane even before presence ticks.
            self.agent = Some(who.clone());
        } else if self.is_foreign(sid) {
            return;
        } else if let Some(sid) = incoming.mapped.session_id.clone() {
            if self.owner_session.is_none() {
                self.owner_session = Some(sid);
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
    /// Only a Grok or Codex pane gets a count — a sibling shell must
    /// not inherit the newest session under this cwd. Grok: session dir
    /// segments/checkpoints, then `signals.json` `compactionCount`
    /// (that field often stays 0). Codex: `type: compacted` records in
    /// `~/.codex/sessions/**/rollout-*-{session_id}.jsonl`. No Grok
    /// owner session yet → newest session under this cwd. Codex needs
    /// `owner_session`; a missed file keeps the hook-incremented count.
    pub fn refresh_compaction(&mut self, cwd: &Path) {
        if !self.shows_compaction() {
            self.compaction_count = 0;
            self.codex_scan_len = 0;
            return;
        }
        if self.is_grok() {
            if let Some(n) = read_compaction_count(cwd, self.owner_session.as_deref()) {
                self.compaction_count = n;
            }
            return;
        }
        if self.is_codex() {
            if let Some((n, len)) = read_codex_compaction_in(
                &codex_home(),
                self.owner_session.as_deref(),
                self.codex_scan_len,
                self.compaction_count,
            ) {
                self.compaction_count = n;
                self.codex_scan_len = len;
            }
        }
    }

    pub fn is_grok(&self) -> bool {
        self.agent.as_deref() == Some("grok")
    }

    pub fn is_codex(&self) -> bool {
        self.agent.as_deref() == Some("codex")
    }

    /// Session to `grok -r` after a lost tmux. Needs Grok still named on
    /// the pane (exited-to-shell is not a resume) and a non-empty id.
    pub fn resume_session_id(&self) -> Option<&str> {
        if !self.is_grok() {
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
        self.is_grok() || self.is_codex()
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

fn grok_home() -> PathBuf {
    std::env::var_os("GROK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".grok")
        })
}

fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codex")
        })
}

/// True when `~/.grok/sessions/<cwd-encode>/<id>/` (any cwd group) exists.
pub fn grok_session_exists(session_id: &str) -> bool {
    grok_session_exists_in(&grok_home(), session_id)
}

fn grok_session_exists_in(home: &Path, session_id: &str) -> bool {
    if !is_session_id(session_id) {
        return false;
    }
    let sessions = home.join("sessions");
    let Ok(rd) = std::fs::read_dir(&sessions) else {
        return false;
    };
    rd.filter_map(|e| e.ok()).any(|group| {
        group.file_type().map(|t| t.is_dir()).unwrap_or(false)
            && group.path().join(session_id).is_dir()
    })
}

fn is_session_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains('\0')
}

pub fn read_compaction_count(cwd: &Path, session_id: Option<&str>) -> Option<u32> {
    read_compaction_count_in(&grok_home(), cwd, session_id)
}

fn read_compaction_count_in(home: &Path, cwd: &Path, session_id: Option<&str>) -> Option<u32> {
    let group = home.join("sessions").join(encode_session_cwd(cwd));
    let sid = session_id
        .filter(|s| !s.is_empty() && group.join(s).is_dir())
        .map(|s| s.to_string())
        .or_else(|| newest_session_id(&group))?;
    Some(count_session_compactions(&group.join(sid)))
}

fn newest_session_id(group: &Path) -> Option<String> {
    let mut best: Option<(std::time::SystemTime, String)> = None;
    let rd = std::fs::read_dir(group).ok()?;
    for ent in rd.filter_map(|e| e.ok()) {
        if !ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let t = std::fs::metadata(ent.path().join("signals.json"))
            .and_then(|m| m.modified())
            .or_else(|_| ent.metadata().and_then(|m| m.modified()))
            .ok()?;
        if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
            best = Some((t, name));
        }
    }
    best.map(|(_, n)| n)
}

/// Max of `signals.json` `compactionCount`, `compaction/segment_*.md`,
/// and `compaction_checkpoints/` files. Signals can lag; artifacts do not.
fn count_session_compactions(dir: &Path) -> u32 {
    let signals = std::fs::read_to_string(dir.join("signals.json"))
        .ok()
        .and_then(|t| parse_compaction_count(&t))
        .unwrap_or(0);
    let segments = count_dir_files(&dir.join("compaction"), |n| {
        n.starts_with("segment_") && n.ends_with(".md")
    });
    let checkpoints = count_dir_files(&dir.join("compaction_checkpoints"), |_| true);
    signals.max(segments).max(checkpoints)
}

fn count_dir_files(dir: &Path, pred: impl Fn(&str) -> bool) -> u32 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| pred(&e.file_name().to_string_lossy()))
        .count() as u32
}

fn parse_compaction_count(text: &str) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    v.get("compactionCount")
        .or_else(|| v.get("compaction_count"))
        .and_then(|n| n.as_u64())
        .map(|n| n as u32)
}

fn read_codex_compaction_in(
    home: &Path,
    session_id: Option<&str>,
    last_len: u64,
    last_count: u32,
) -> Option<(u32, u64)> {
    let sid = session_id.filter(|s| is_session_id(s))?;
    let path = find_codex_rollout(home, sid)?;
    let len = std::fs::metadata(&path).ok()?.len();
    if len == last_len && last_count > 0 {
        return Some((last_count, len));
    }
    Some((count_codex_compactions(&path), len))
}

/// `~/.codex/sessions/YYYY/MM/DD/rollout-{ts}-{session_id}.jsonl`
fn find_codex_rollout(home: &Path, session_id: &str) -> Option<PathBuf> {
    if !is_session_id(session_id) {
        return None;
    }
    let needle = format!("-{session_id}.jsonl");
    find_rollout_under(&home.join("sessions"), &needle, 4)
}

fn find_rollout_under(dir: &Path, suffix: &str, depth: u8) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for ent in rd.filter_map(|e| e.ok()) {
        let path = ent.path();
        let Ok(ft) = ent.file_type() else {
            continue;
        };
        if ft.is_file() {
            let name = path.file_name()?.to_string_lossy();
            if name.starts_with("rollout-") && name.ends_with(suffix) {
                return Some(path);
            }
        } else if ft.is_dir() && depth > 0 {
            dirs.push(path);
        }
    }
    for d in dirs {
        if let Some(found) = find_rollout_under(&d, suffix, depth.saturating_sub(1)) {
            return Some(found);
        }
    }
    None
}

/// Count jsonl records whose top-level `type` is `compacted`.
/// Compact payloads are huge; only the first bytes of each line are
/// inspected, then the rest of the line is skipped without buffering it.
fn count_codex_compactions(path: &Path) -> u32 {
    let Ok(file) = File::open(path) else {
        return 0;
    };
    let mut reader = BufReader::new(file);
    let mut n = 0u32;
    loop {
        let peek = match reader.fill_buf() {
            Ok(buf) if !buf.is_empty() => buf,
            _ => break,
        };
        let nl = peek.iter().position(|&b| b == b'\n');
        let head_end = nl.unwrap_or(peek.len()).min(192);
        let compacted = line_is_codex_compacted(&peek[..head_end]);
        match nl {
            Some(i) => reader.consume(i + 1),
            None => {
                let used = peek.len();
                reader.consume(used);
                skip_until_newline(&mut reader);
            }
        }
        if compacted {
            n += 1;
        }
    }
    n
}

fn skip_until_newline(reader: &mut impl BufRead) {
    loop {
        let peek = match reader.fill_buf() {
            Ok(buf) if !buf.is_empty() => buf,
            _ => return,
        };
        if let Some(i) = peek.iter().position(|&b| b == b'\n') {
            reader.consume(i + 1);
            return;
        }
        let n = peek.len();
        reader.consume(n);
    }
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

fn last_status_path() -> PathBuf {
    crate::paths::config_dir().join("last-status.json")
}

#[derive(Serialize, Deserialize)]
struct DiskPane {
    status: AgentStatus,
    agent: Option<String>,
    /// Grok `owner_session`. Resume `grok -r` after a lost tmux.
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
    use crate::hooks::map::MappedHook;
    use crate::hooks::server::Incoming;

    fn hook(session: &str, status: AgentStatus) -> Incoming {
        Incoming {
            pane_id: "p".into(),
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
            pane_id: "p".into(),
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
            pane_id: "p".into(),
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
            pane_id: "p".into(),
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
    fn vocab_maps_one_to_one_onto_kit_marks() {
        assert_eq!(AgentStatus::Working.indicator(), SidebarIndicator::Working);
        assert_eq!(AgentStatus::Waiting.indicator(), SidebarIndicator::Waiting);
        assert_eq!(AgentStatus::Done.indicator(), SidebarIndicator::Done);
        assert_eq!(AgentStatus::Idle.indicator(), SidebarIndicator::Idle);
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
        assert_eq!(read_compaction_count_in(&root, cwd, Some("sid-a")), Some(1));
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
        assert_eq!(read_compaction_count_in(&root, cwd, Some("sid-b")), Some(8));
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
        assert_eq!(read_compaction_count_in(&root, cwd, None), Some(2));
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
            agent: Some("claude".into()),
            ..PaneStatus::default()
        };
        assert_eq!(rollup_tracked([&grok, &shell, &other]), AgentStatus::Done);
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
    fn rail_compaction_takes_max_grok_or_codex() {
        let a = PaneStatus {
            agent: Some("grok".into()),
            compaction_count: 2,
            ..PaneStatus::default()
        };
        let b = PaneStatus {
            agent: Some("grok".into()),
            compaction_count: 5,
            ..PaneStatus::default()
        };
        let shell = PaneStatus {
            compaction_count: 9,
            ..PaneStatus::default()
        };
        let codex = PaneStatus {
            agent: Some("codex".into()),
            compaction_count: 7,
            ..PaneStatus::default()
        };
        assert_eq!(rail_compaction([&a, &b, &shell]), 5);
        assert_eq!(rail_compaction([&shell]), 0);
        assert_eq!(rail_compaction([&a, &codex, &shell]), 7);
        assert_eq!(rail_compaction([&codex]), 7);
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
        pane.refresh_compaction(Path::new("/tmp/not-a-session"));
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
        pane.refresh_compaction(Path::new("/tmp/not-a-session"));
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
        let path = find_codex_rollout(&root, sid).unwrap();
        let len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(read_codex_compaction_in(&root, Some(sid), 0, 0), Some((3, len)));
        // Unchanged length keeps the previous count (no rescan).
        assert_eq!(
            read_codex_compaction_in(&root, Some(sid), len, 99),
            Some((99, len))
        );
        assert_eq!(count_codex_compactions(&path), 3);
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
        assert_eq!(count_codex_compactions(&long), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_hook_session_start_names_codex() {
        let mut pane = PaneStatus::default();
        let incoming = Incoming {
            pane_id: "p".into(),
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
        assert!(grok_session_exists_in(&root, "sid-live"));
        assert!(!grok_session_exists_in(&root, "missing"));
        assert!(!grok_session_exists_in(&root, "../nope"));
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
