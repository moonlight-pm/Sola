//! Pane / workspace status vocabulary.
//!
//! Hooks (Grok first) and OSC 9999 write this. Process-tree only names
//! *who*. Never infer from OSC 0/2 titles.

use std::path::PathBuf;

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
}

impl PaneStatus {
    pub fn apply_hook(&mut self, incoming: &Incoming) {
        if self.is_foreign(incoming.mapped.session_id.as_deref()) {
            return;
        }
        if incoming.mapped.clear_turn {
            self.tool = None;
            return;
        }
        let Some(status) = incoming.mapped.status else {
            return;
        };
        if let Some(sid) = &incoming.mapped.session_id {
            if self.owner_session.is_none() {
                self.owner_session = Some(sid.clone());
            }
        }
        if incoming.mapped.session_end {
            self.owner_session = None;
        }
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

    /// Presence names who is here. It never sets working/waiting/done.
    pub fn apply_presence(&mut self, who: Option<&str>) {
        match who {
            Some("grok") => self.agent = Some("grok".into()),
            Some(other) => {
                if self.agent.as_deref() != Some("grok") {
                    self.agent = Some(other.to_string());
                }
            }
            None => {
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

fn last_status_path() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("agent-terminal")
        .join("last-status.json")
}

#[derive(Serialize, Deserialize)]
struct DiskSnapshot {
    pane_id: String,
    status: AgentStatus,
    agent: Option<String>,
}

pub fn persist(pane_id: &str, pane: &PaneStatus) {
    let snap = DiskSnapshot {
        pane_id: pane_id.into(),
        status: pane.status,
        agent: pane.agent.clone(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&snap) {
        let path = last_status_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

/// Hydrate last hook status. Caller must mark unconfirmed and not toast.
pub fn hydrate(pane_id: &str) -> Option<PaneStatus> {
    let text = std::fs::read_to_string(last_status_path()).ok()?;
    let snap: DiskSnapshot = serde_json::from_str(&text).ok()?;
    if snap.pane_id != pane_id {
        return None;
    }
    Some(PaneStatus {
        status: snap.status,
        agent: snap.agent,
        restored_unconfirmed: snap.status != AgentStatus::Idle,
        ..PaneStatus::default()
    })
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
                session_end: false,
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
    fn child_session_cannot_done_parent() {
        let mut pane = PaneStatus::default();
        pane.apply_hook(&hook("owner", AgentStatus::Working));
        pane.apply_hook(&hook("child", AgentStatus::Done));
        assert_eq!(pane.status, AgentStatus::Working);
        pane.apply_hook(&hook("owner", AgentStatus::Done));
        assert_eq!(pane.status, AgentStatus::Done);
    }

    #[test]
    fn presence_does_not_set_working() {
        let mut pane = PaneStatus::default();
        pane.apply_presence(Some("grok"));
        assert_eq!(pane.status, AgentStatus::Idle);
        assert_eq!(pane.agent.as_deref(), Some("grok"));
    }

    #[test]
    fn grok_presence_wins_over_other() {
        let mut pane = PaneStatus::default();
        pane.apply_presence(Some("claude"));
        pane.apply_presence(Some("grok"));
        assert_eq!(pane.agent.as_deref(), Some("grok"));
        pane.apply_presence(Some("claude"));
        assert_eq!(pane.agent.as_deref(), Some("grok"));
    }
}
