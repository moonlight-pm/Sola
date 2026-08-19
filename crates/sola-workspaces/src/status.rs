//! Pane / workspace status vocabulary.
//!
//! Hooks (Grok first) and OSC 9999 write this. Process-tree only names
//! *who*. Never infer from OSC 0/2 titles.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sola_kit::components::SidebarIndicator;

use crate::hooks::Incoming;
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

    /// Workspace row: working beats waiting beats done beats idle.
    pub fn rollup(statuses: impl IntoIterator<Item = Self>) -> Self {
        let mut best = Self::Idle;
        for s in statuses {
            best = match (best, s) {
                (Self::Working, _) | (_, Self::Working) => Self::Working,
                (Self::Waiting, _) | (_, Self::Waiting) => Self::Waiting,
                (Self::Done, _) | (_, Self::Done) => Self::Done,
                _ => Self::Idle,
            };
        }
        best
    }
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
    /// Grok `signals.json` `compactionCount` for `owner_session`.
    pub compaction_count: u32,
}

impl PaneStatus {
    pub fn apply_hook(&mut self, incoming: &Incoming) {
        let sid = incoming.mapped.session_id.as_deref();
        // SessionStart / UserPromptSubmit are lead events for the grok
        // in this pane. They must reclaim after `/new`, `grok -r`, or a
        // child CLI that inherited SOLA_PANE_ID — otherwise the mark
        // freezes on the previous session. Grok does not fire those
        // events for a subagent's own session.
        if incoming.mapped.claim || incoming.mapped.clear_turn {
            if let Some(sid) = incoming.mapped.session_id.clone() {
                self.owner_session = Some(sid);
            }
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
                return;
            }
        }
        if incoming.mapped.compacted {
            self.agent = Some("grok".into());
        }
        if incoming.mapped.compacted && incoming.mapped.status.is_none() {
            return;
        }
        let Some(status) = incoming.mapped.status else {
            return;
        };
        self.status = status;
        self.restored_unconfirmed = false;
        self.agent = Some("grok".into());
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

    /// Refresh `compaction_count` from the pane's Grok session dir.
    ///
    /// Only a pane whose presence is Grok gets a count — a sibling
    /// shell must not inherit the newest session under this cwd.
    /// `signals.json` `compactionCount` is preferred when it is ahead,
    /// but Grok often leaves that field at 0 after a compact; segment
    /// files and checkpoints are the durable record. No owner session
    /// yet → newest session under this cwd.
    pub fn refresh_compaction(&mut self, cwd: &Path) {
        if !self.shows_compaction() {
            self.compaction_count = 0;
            return;
        }
        if let Some(n) = read_compaction_count(cwd, self.owner_session.as_deref()) {
            self.compaction_count = n;
        }
    }

    fn shows_compaction(&self) -> bool {
        self.agent.as_deref() == Some("grok")
    }

    /// Presence names who is here. It never sets working/waiting/done.
    /// The sidebar leaf label follows this every tick.
    pub fn apply_presence(&mut self, who: Option<&str>) {
        match who {
            Some(name) => self.agent = Some(name.to_string()),
            None => {
                self.agent = None;
                if self.restored_unconfirmed
                    && matches!(self.status, AgentStatus::Working | AgentStatus::Waiting)
                {
                    self.status = AgentStatus::Idle;
                    self.restored_unconfirmed = false;
                }
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

fn last_status_path() -> PathBuf {
    crate::paths::config_dir().join("last-status.json")
}

#[derive(Serialize, Deserialize)]
struct DiskPane {
    status: AgentStatus,
    agent: Option<String>,
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

fn write_snapshot(snap: &DiskSnapshot) {
    if let Ok(text) = serde_json::to_string_pretty(snap) {
        let path = last_status_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

fn read_snapshot() -> Option<DiskSnapshot> {
    let text = std::fs::read_to_string(last_status_path()).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn persist_all(panes: &std::collections::HashMap<String, PaneStatus>) {
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
            },
        );
    }
    write_snapshot(&snap);
}

/// Hydrate last hook status. Caller must mark unconfirmed and not toast.
pub fn hydrate(pane_id: &str) -> Option<PaneStatus> {
    let snap = read_snapshot()?;
    if let Some(p) = snap.panes.get(pane_id) {
        return Some(PaneStatus {
            status: p.status,
            agent: p.agent.clone(),
            restored_unconfirmed: p.status != AgentStatus::Idle,
            ..PaneStatus::default()
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::map::MappedHook;
    use crate::hooks::server::Incoming;

    fn hook(session: &str, status: AgentStatus) -> Incoming {
        Incoming {
            pane_id: "p".into(),
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
            std::fs::write(dir.join("compaction").join(format!("segment_{i:03}.md")), "x")
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
        assert_eq!(
            read_compaction_count_in(&root, cwd, Some("sid-a")),
            Some(1)
        );
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
        assert_eq!(
            read_compaction_count_in(&root, cwd, Some("sid-b")),
            Some(8)
        );
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
    fn rollup_prefers_working() {
        assert_eq!(
            AgentStatus::rollup([AgentStatus::Idle, AgentStatus::Done, AgentStatus::Working]),
            AgentStatus::Working
        );
        assert_eq!(
            AgentStatus::rollup([AgentStatus::Done, AgentStatus::Waiting]),
            AgentStatus::Waiting
        );
        assert_eq!(AgentStatus::rollup([]), AgentStatus::Idle);
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
        pane.apply_hook(&hook("new", AgentStatus::Waiting));
        assert_eq!(pane.owner_session.as_deref(), Some("new"));
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
        pane.apply_presence(Some("grok"));
        assert_eq!(pane.status, AgentStatus::Idle);
        assert_eq!(pane.agent.as_deref(), Some("grok"));
    }

    #[test]
    fn presence_tracks_who_is_live() {
        let mut pane = PaneStatus::default();
        pane.apply_presence(Some("grok"));
        assert_eq!(pane.agent.as_deref(), Some("grok"));
        pane.apply_presence(Some("claude"));
        assert_eq!(pane.agent.as_deref(), Some("claude"));
        pane.apply_presence(None);
        assert_eq!(pane.agent, None);
    }

    #[test]
    fn shell_pane_does_not_inherit_compaction() {
        let mut pane = PaneStatus::default();
        pane.compaction_count = 4;
        pane.refresh_compaction(Path::new("/tmp/not-a-session"));
        assert_eq!(pane.compaction_count, 0);
    }
}
