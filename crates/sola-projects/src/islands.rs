//! Agent islands (`docs/specs/2026-10-23-agent-islands-design.md`): a
//! session that asks (a permission, a question) or finishes its turn gets
//! an island in the shell's top row (sticky `Topic::AgentIsland`, keyed by
//! its pane), until it moves on, you look at its tab, or you dismiss it.
//! What it asks comes from its hooks ([`ask_from_hook`]).

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use sola_bus::topics::{AgentIsland, IslandState, Topic};

use crate::status::AgentStatus;

/// What a hook's payload says the session asks, for after its name: "wants
/// to run cargo make build" (Claude's `tool_name` / `tool_input`), "is
/// waiting" (a notification). `None` when it says nothing.
pub fn ask_from_hook(event: &Value) -> Option<String> {
    let tool = str_at(event, &["tool_name", "toolName"]);
    let input = event.get("tool_input").or_else(|| event.get("toolInput"));
    if let Some(tool) = tool {
        return Some(ask_for_tool(&tool, input.unwrap_or(&Value::Null)));
    }
    str_at(event, &["message"])?;
    Some("is waiting".into())
}

fn ask_for_tool(tool: &str, input: &Value) -> String {
    let field = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    match tool {
        "Bash" => {
            let command = field("command").unwrap_or_default();
            format!("wants to run {}", first_line(&command))
        }
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let path = field("file_path")
                .or_else(|| field("notebook_path"))
                .unwrap_or_default();
            let name = Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(path);
            format!("wants to edit {name}")
        }
        "WebFetch" => format!("wants to fetch {}", field("url").unwrap_or_default()),
        "AskUserQuestion" => question(input),
        _ => format!("wants to use {tool}"),
    }
}

/// `AskUserQuestion`: its first question.
fn question(input: &Value) -> String {
    let text = input
        .get("questions")
        .and_then(Value::as_array)
        .and_then(|q| q.first())
        .and_then(|q| q.get("question"))
        .and_then(Value::as_str)
        .unwrap_or("A question");
    format!("asks: {}", first_line(text))
}

fn str_at(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or_default().trim()
}

/// What a session needs from you.
#[derive(Debug, Clone)]
pub struct Need {
    pub state: IslandState,
    pub title: String,
    /// Since (unix ms).
    pub since: u64,
    /// Looked at or dismissed: no island until it needs you again.
    pub seen: bool,
    /// The title is the hook's (not "is waiting").
    pub asked: bool,
}

/// Every session's need, and the islands last sent.
#[derive(Default)]
pub struct Islands {
    needs: HashMap<String, Need>,
    asks: HashMap<String, String>,
    sent: HashMap<String, AgentIsland>,
    /// Each pane's status as last followed: a need comes from a change,
    /// not from a status first seen (restored from before a restart).
    statuses: HashMap<String, AgentStatus>,
    /// Stale islands of a previous run retracted.
    primed: bool,
}

impl Islands {
    /// A hook for pane `pane`: what it asks.
    pub fn note_hook(&mut self, pane: &str, ask: Option<String>) {
        let Some(ask) = ask else {
            return;
        };
        // A permission's notification follows its request: keep the
        // request's command over the notification's message.
        if let Some(need) = self.needs.get_mut(pane) {
            if need.state == IslandState::Waiting && !need.asked {
                need.title = ask.clone();
                need.asked = true;
            }
            if need.asked {
                return;
            }
        }
        self.asks.insert(pane.to_string(), ask);
    }

    /// Follow pane `pane`'s status (`None` while it is unconfirmed after a
    /// restart). Only a change makes a need: a status first seen is where
    /// the session already was.
    pub fn track(&mut self, pane: &str, status: Option<AgentStatus>, now: u64) {
        let Some(status) = status else {
            return;
        };
        let prev = self.statuses.insert(pane.to_string(), status);
        let state = match status {
            AgentStatus::Waiting => IslandState::Waiting,
            AgentStatus::Done => IslandState::Done,
            AgentStatus::Working | AgentStatus::Idle => {
                self.needs.remove(pane);
                self.asks.remove(pane);
                return;
            }
        };
        if prev.is_none() || prev == Some(status) {
            return;
        }
        let need = match state {
            IslandState::Waiting => match self.asks.remove(pane) {
                Some(ask) => Need::new(state, ask, now, true),
                None => Need::new(state, "is waiting".into(), now, false),
            },
            IslandState::Done => Need::new(state, "finished".into(), now, false),
        };
        tracing::info!(%pane, ?state, title = %need.title, "island need");
        self.needs.insert(pane.to_string(), need);
    }

    /// Looked at elsewhere, pulled up or dismissed: its island goes (the
    /// need stays until the session moves on, so it does not come back
    /// meanwhile).
    pub fn let_go(&mut self, pane: &str) {
        if let Some(need) = self.needs.get_mut(pane) {
            need.seen = true;
        }
    }

    /// Forget panes that are gone.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.needs.retain(|p, _| keep(p));
        self.asks.retain(|p, _| keep(p));
        self.statuses.retain(|p, _| keep(p));
    }

    /// The panes that need you, for the caller to say which you are
    /// looking at.
    pub fn panes(&self) -> Vec<String> {
        self.needs.keys().cloned().collect()
    }

    /// A need you look at is seen: its window has the keyboard and the
    /// desk is not idle-fading (`looked_at`, from the caller). It stays
    /// seen until the session moves on. Returns the panes it just saw.
    pub fn mark_seen(&mut self, looked_at: &HashMap<String, bool>) -> Vec<String> {
        let mut now_seen = Vec::new();
        for (pane, need) in &mut self.needs {
            if !need.seen && looked_at.get(pane).copied().unwrap_or(false) {
                need.seen = true;
                now_seen.push(pane.clone());
            }
        }
        now_seen
    }

    /// Send the islands: one per need not seen ([`Self::mark_seen`] first);
    /// `place` names a pane `(project, session)`. `known` are every pane's
    /// ids: on the first sync, their islands of a previous run go. Returns
    /// the panes looked at just now (for other computers to hear).
    pub fn sync(
        &mut self,
        looked_at: &HashMap<String, bool>,
        place: &HashMap<String, (String, String)>,
        known: &[String],
    ) -> Vec<String> {
        let now_seen = self.mark_seen(looked_at);
        let mut want: HashMap<String, AgentIsland> = HashMap::new();
        for (pane, need) in &self.needs {
            if need.seen {
                continue;
            }
            let (project, name) = place.get(pane).cloned().unwrap_or_default();
            want.insert(
                pane.clone(),
                AgentIsland {
                    key: pane.clone(),
                    name,
                    project,
                    state: need.state,
                    title: need.title.clone(),
                    since: need.since,
                },
            );
        }
        let Ok(mut bus) = sola_kit::app::bus().lock() else {
            return now_seen;
        };
        if !self.primed {
            self.primed = true;
            for pane in known {
                if !want.contains_key(pane) {
                    let _ = bus.retract(Topic::AgentIsland(empty(pane)));
                }
            }
        }
        let gone: Vec<String> = self
            .sent
            .keys()
            .filter(|p| !want.contains_key(*p))
            .cloned()
            .collect();
        for pane in gone {
            if let Some(island) = self.sent.remove(&pane) {
                let _ = bus.retract(Topic::AgentIsland(island));
            }
        }
        for (pane, island) in want {
            if self.sent.get(&pane) == Some(&island) {
                continue;
            }
            if bus.emit(Topic::AgentIsland(island.clone())).is_ok() {
                self.sent.insert(pane, island);
            }
        }
        now_seen
    }
}

impl Need {
    fn new(state: IslandState, title: String, since: u64, asked: bool) -> Self {
        Self {
            state,
            title,
            since,
            seen: false,
            asked,
        }
    }
}

/// An island with only its key (to retract one).
fn empty(pane: &str) -> AgentIsland {
    AgentIsland {
        key: pane.to_string(),
        name: String::new(),
        project: String::new(),
        state: IslandState::Waiting,
        title: String::new(),
        since: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn need<'a>(islands: &'a Islands, pane: &str) -> Option<&'a Need> {
        islands.needs.get(pane)
    }

    #[test]
    fn a_permission_names_its_command() {
        let ask = ask_from_hook(&json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {"command": "cargo make build sola-kit", "description": "Build the kit"}
        }));
        assert_eq!(ask.as_deref(), Some("wants to run cargo make build sola-kit"));
    }

    #[test]
    fn a_question_says_its_first_question() {
        let ask = ask_from_hook(&json!({
            "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [{"question": "Which layout?", "options": [
                {"label": "Grid", "description": "Two by two"}, {"label": "Stack"}
            ]}]}
        }));
        assert_eq!(ask.as_deref(), Some("asks: Which layout?"));
    }

    #[test]
    fn a_need_follows_its_session() {
        let mut islands = Islands::default();
        // Where a session already is makes no island.
        islands.track("p", Some(AgentStatus::Done), 0);
        assert!(need(&islands, "p").is_none());
        islands.track("p", None, 0);
        islands.track("p", Some(AgentStatus::Working), 0);
        assert!(need(&islands, "p").is_none());
        islands.note_hook("p", Some("wants to run ls".into()));
        islands.track("p", Some(AgentStatus::Waiting), 1);
        assert_eq!(need(&islands, "p").unwrap().title, "wants to run ls");
        assert_eq!(need(&islands, "p").unwrap().since, 1);
        // Still waiting is not a new need.
        islands.track("p", Some(AgentStatus::Waiting), 2);
        assert_eq!(need(&islands, "p").unwrap().since, 1);
        islands.track("p", Some(AgentStatus::Working), 3);
        assert!(need(&islands, "p").is_none());
        islands.track("p", Some(AgentStatus::Done), 4);
        assert_eq!(need(&islands, "p").unwrap().state, IslandState::Done);
    }

    #[test]
    fn a_need_dismissed_stays_gone_until_the_session_needs_you_again() {
        let mut islands = Islands::default();
        islands.track("p", Some(AgentStatus::Working), 0);
        islands.track("p", Some(AgentStatus::Done), 1);
        islands.let_go("p");
        assert!(need(&islands, "p").unwrap().seen);
        // Still done: no island back.
        islands.track("p", Some(AgentStatus::Done), 2);
        assert!(need(&islands, "p").unwrap().seen);
        // It works and finishes again: a new island.
        islands.track("p", Some(AgentStatus::Working), 3);
        islands.track("p", Some(AgentStatus::Done), 4);
        assert!(!need(&islands, "p").unwrap().seen);
    }

    #[test]
    fn a_need_looked_at_is_seen_until_it_moves_on() {
        let mut islands = Islands::default();
        islands.track("p", Some(AgentStatus::Working), 0);
        islands.track("p", Some(AgentStatus::Done), 100);
        // Not looked at (another tab, or the desk fading): its island shows.
        islands.mark_seen(&HashMap::new());
        assert!(!need(&islands, "p").unwrap().seen);
        let looked: HashMap<String, bool> = [("p".to_string(), true)].into();
        islands.mark_seen(&looked);
        assert!(need(&islands, "p").unwrap().seen);
        // Working again clears it.
        islands.track("p", Some(AgentStatus::Working), 200);
        assert!(need(&islands, "p").is_none());
    }
}
