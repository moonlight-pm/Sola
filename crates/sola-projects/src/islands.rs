//! Agent islands (`docs/specs/2026-10-23-agent-islands-design.md`): a
//! session that asks (a permission, a question) or finishes its turn gets
//! an island in the shell's top row (sticky `Topic::AgentIsland`, keyed by
//! its pane), until it moves on, is answered, or you look at its tab. What
//! it asks comes from its hooks ([`ask_from_hook`]); the answers are the
//! prompt's numbered lines on its screen ([`choices_on_screen`]); a
//! finished Claude turn's last message comes from its transcript
//! ([`last_assistant_text`]).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;
use sola_bus::topics::{AgentIsland, IslandState, Topic};

use crate::status::AgentStatus;

/// What a hook says a session asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// After the session's name: "wants to run cargo make build".
    pub title: String,
    pub body: String,
}

/// An answer the prompt offers: its label and the key that picks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub key: char,
}

/// The longest body sent on the bus.
const BODY_MAX: usize = 4000;
/// How much of a transcript's end is read for its last message.
const TRANSCRIPT_TAIL: u64 = 512 * 1024;

/// What a hook's payload says the session asks (Claude's `tool_name` /
/// `tool_input`, a notification's message). `None` when it says nothing
/// (the island then reads "is waiting" and shows the screen).
pub fn ask_from_hook(event: &Value) -> Option<Ask> {
    let tool = str_at(event, &["tool_name", "toolName"]);
    let input = event.get("tool_input").or_else(|| event.get("toolInput"));
    if let Some(tool) = tool {
        return Some(ask_for_tool(&tool, input.unwrap_or(&Value::Null)));
    }
    let message = str_at(event, &["message"])?;
    Some(Ask {
        title: "is waiting".into(),
        body: message,
    })
}

fn ask_for_tool(tool: &str, input: &Value) -> Ask {
    let field = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    match tool {
        "Bash" => {
            let command = field("command").unwrap_or_default();
            let mut body = command.clone();
            if let Some(d) = field("description") {
                body = format!("{d}\n\n{command}");
            }
            Ask {
                title: format!("wants to run {}", first_line(&command)),
                body,
            }
        }
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let path = field("file_path")
                .or_else(|| field("notebook_path"))
                .unwrap_or_default();
            let name = Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            Ask {
                title: format!("wants to edit {name}"),
                body: path,
            }
        }
        "WebFetch" => {
            let url = field("url").unwrap_or_default();
            Ask {
                title: format!("wants to fetch {url}"),
                body: url,
            }
        }
        "AskUserQuestion" => question(input),
        _ => Ask {
            title: format!("wants to use {tool}"),
            body: serde_json::to_string_pretty(input).unwrap_or_default(),
        },
    }
}

/// `AskUserQuestion`: the first question and its options.
fn question(input: &Value) -> Ask {
    let first = input
        .get("questions")
        .and_then(Value::as_array)
        .and_then(|q| q.first());
    let text = first
        .and_then(|q| q.get("question"))
        .and_then(Value::as_str)
        .unwrap_or("A question")
        .to_string();
    let mut body = text.clone();
    let options = first
        .and_then(|q| q.get("options"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for o in options {
        let label = o.get("label").and_then(Value::as_str).unwrap_or_default();
        match o.get("description").and_then(Value::as_str) {
            Some(d) if !d.is_empty() => body.push_str(&format!("\n• {label} — {d}")),
            _ => body.push_str(&format!("\n• {label}")),
        }
    }
    Ask {
        title: format!("asks: {}", first_line(&text)),
        body,
    }
}

/// The transcript a Claude hook names (`transcript_path`).
pub fn transcript_of(event: &Value) -> Option<String> {
    str_at(event, &["transcript_path", "transcriptPath"])
}

/// The prompt's numbered answers on a screen: the last run of lines
/// `1. …`, `2. …` (a cursor mark or a box edge in front allowed; lines
/// between, an option's description, skipped). Fewer than two is none.
pub fn choices_on_screen(screen: &str) -> Vec<Choice> {
    let mut best: Vec<Choice> = Vec::new();
    let mut run: Vec<Choice> = Vec::new();
    for line in screen.lines() {
        let Some((n, label)) = numbered(line) else {
            continue;
        };
        if n == 1 {
            if run.len() >= 2 {
                best = std::mem::take(&mut run);
            }
            run.clear();
        }
        if usize::from(n) == run.len() + 1 {
            run.push(Choice {
                label,
                key: char::from(b'0' + n),
            });
        }
    }
    if run.len() >= 2 { run } else { best }
}

/// `│ ❯ 2. Yes, and don't ask again │` → `(2, "Yes, and don't ask again")`.
fn numbered(line: &str) -> Option<(u8, String)> {
    let t = line
        .trim()
        .trim_matches(|c: char| c == '│' || c == '┃' || c == '|')
        .trim();
    let t = t
        .trim_start_matches(|c: char| matches!(c, '❯' | '›' | '>' | '▶' | '→'))
        .trim_start();
    let mut chars = t.chars();
    let d = chars.next()?.to_digit(10)?;
    if d == 0 {
        return None;
    }
    let rest = chars.as_str();
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    let label = rest.trim();
    (!label.is_empty()).then(|| (d as u8, label.to_string()))
}

/// The answer that lets you type your own (Claude's "Type something").
pub fn is_free_text(label: &str) -> bool {
    let l = label.to_ascii_lowercase();
    l.starts_with("type something") || l.starts_with("other")
}

/// The end of a screen, trimmed: a body when a hook says nothing. The
/// agent's input box at the bottom (a rule, the prompt, a rule, a status
/// line) is not part of it.
pub fn screen_tail(screen: &str, lines: usize) -> String {
    let mut all: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let rule = |l: &str| {
        let t = l.trim();
        t.chars().count() >= 8 && t.chars().all(|c| matches!(c, '─' | '━' | '═' | '-'))
    };
    // The input box: the last two rules at most a few lines apart.
    let rules: Vec<usize> = (0..all.len()).filter(|&i| rule(all[i])).collect();
    if let [.., upper, lower] = rules[..] {
        if lower - upper <= 4 && all.len() - lower <= 4 {
            all.truncate(upper);
        }
    }
    let mut tail: Vec<&str> = all
        .into_iter()
        .rev()
        .skip_while(|l| l.trim().is_empty())
        .take(lines)
        .collect();
    tail.reverse();
    tail.join("\n")
}

/// The text of the last turn's last assistant message in a Claude
/// transcript (JSONL,
/// `{"type":"assistant","message":{"content":[{"type":"text","text":…}]}}`),
/// read from its end. `None` while the turn's answer is not written yet
/// (Claude flushes it a moment after its Stop hook): a prompt comes first.
pub fn last_assistant_text(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TRANSCRIPT_TAIL)))
        .ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    for line in text.lines().rev() {
        match turn_line(line) {
            Some(Line::Text(t)) => return Some(t),
            Some(Line::Prompt) => return None,
            None => {}
        }
    }
    None
}

enum Line {
    /// An assistant message with text.
    Text(String),
    /// A prompt you typed (not a tool's result).
    Prompt,
}

fn turn_line(line: &str) -> Option<Line> {
    let v: Value = serde_json::from_str(line).ok()?;
    let content = v.get("message")?.get("content")?;
    match v.get("type").and_then(Value::as_str)? {
        "assistant" => {
            let parts: Vec<&str> = content
                .as_array()?
                .iter()
                .filter(|c| c.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|c| c.get("text").and_then(Value::as_str))
                .collect();
            let text = parts.join("\n\n");
            (!text.trim().is_empty()).then(|| Line::Text(text.trim().to_string()))
        }
        "user" if v.get("isMeta").and_then(Value::as_bool) != Some(true) => {
            let typed = content.is_string()
                || content.as_array().is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|c| c.get("type").and_then(Value::as_str) == Some("text"))
                });
            typed.then_some(Line::Prompt)
        }
        _ => None,
    }
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

fn clip(s: &str) -> String {
    if s.len() <= BODY_MAX {
        return s.to_string();
    }
    let mut end = BODY_MAX;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// What a session needs from you.
#[derive(Debug, Clone)]
pub struct Need {
    pub state: IslandState,
    pub title: String,
    pub body: String,
    pub choices: Vec<Choice>,
    /// Since (unix ms).
    pub since: u64,
    /// Looked at or answered: no island until it needs you again.
    pub seen: bool,
    /// Screen reads so far (the prompt draws a moment after the hook).
    pub scans: u8,
    /// The title and body are the hook's (not "is waiting" and the screen).
    pub asked: bool,
}

impl Need {
    /// A free-text answer: a finished session's reply, or a question's own.
    pub fn reply(&self) -> bool {
        self.state == IslandState::Done || self.choices.iter().any(|c| is_free_text(&c.label))
    }

    /// The choices the island offers as buttons: all but "Type something"
    /// (the island's reply field is that). `AgentIslandAction::Choose`
    /// indexes these.
    pub fn offered(&self) -> Vec<&Choice> {
        self.choices.iter().filter(|c| !is_free_text(&c.label)).collect()
    }
}

/// Every session's need, and the islands last sent.
#[derive(Default)]
pub struct Islands {
    needs: HashMap<String, Need>,
    asks: HashMap<String, Ask>,
    transcripts: HashMap<String, String>,
    sent: HashMap<String, AgentIsland>,
    /// Each pane's status as last followed: a need comes from a change,
    /// not from a status first seen (restored from before a restart).
    statuses: HashMap<String, AgentStatus>,
    /// Stale islands of a previous run retracted.
    primed: bool,
}

impl Islands {
    /// A hook for pane `pane`: what it asks, and its transcript.
    pub fn note_hook(&mut self, pane: &str, ask: Option<Ask>, transcript: Option<String>) {
        if let Some(t) = transcript {
            self.transcripts.insert(pane.to_string(), t);
        }
        let Some(ask) = ask else {
            return;
        };
        // A permission's notification follows its request: keep the
        // request's command over the notification's message.
        if let Some(need) = self.needs.get_mut(pane) {
            if need.state == IslandState::Waiting && !need.asked {
                need.title = ask.title.clone();
                need.body = clip(&ask.body);
                need.asked = true;
            }
            if need.asked {
                return;
            }
        }
        self.asks.insert(pane.to_string(), ask);
    }

    /// Follow pane `pane`'s status (`None` while it is unconfirmed after a
    /// restart). True when it newly needs you (read its screen in a moment
    /// for the choices or a body). Only a change makes a need: a status
    /// first seen is where the session already was.
    pub fn track(&mut self, pane: &str, status: Option<AgentStatus>, now: u64) -> bool {
        let Some(status) = status else {
            return false;
        };
        let prev = self.statuses.insert(pane.to_string(), status);
        let state = match status {
            AgentStatus::Waiting => IslandState::Waiting,
            AgentStatus::Done => IslandState::Done,
            AgentStatus::Working | AgentStatus::Idle => {
                self.needs.remove(pane);
                self.asks.remove(pane);
                return false;
            }
        };
        if prev.is_none() || prev == Some(status) {
            return false;
        }
        let need = match state {
            IslandState::Waiting => match self.asks.remove(pane) {
                Some(ask) => Need::new(state, ask.title, clip(&ask.body), now, true),
                None => Need::new(state, "is waiting".into(), String::new(), now, false),
            },
            IslandState::Done => {
                let body = self.last_message(pane).unwrap_or_default();
                let asked = !body.is_empty();
                Need::new(state, "finished".into(), body, now, asked)
            }
        };
        tracing::info!(
            %pane,
            ?state,
            title = %need.title,
            transcript = self.transcripts.contains_key(pane),
            body = need.body.len(),
            "island need"
        );
        self.needs.insert(pane.to_string(), need);
        true
    }

    /// The last message of pane `pane`'s transcript, if it has one.
    pub fn last_message(&self, pane: &str) -> Option<String> {
        let path = self.transcripts.get(pane)?;
        last_assistant_text(Path::new(path)).map(|b| clip(&b))
    }

    pub fn need_mut(&mut self, pane: &str) -> Option<&mut Need> {
        self.needs.get_mut(pane)
    }

    /// Answered or pulled up: its island goes (the need stays until the
    /// session moves on, so it does not come back meanwhile).
    pub fn answer(&mut self, pane: &str) -> Option<Need> {
        let need = self.needs.get_mut(pane)?;
        need.seen = true;
        Some(need.clone())
    }

    /// Forget panes that are gone.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.needs.retain(|p, _| keep(p));
        self.asks.retain(|p, _| keep(p));
        self.transcripts.retain(|p, _| keep(p));
        self.statuses.retain(|p, _| keep(p));
    }

    /// The panes that need you, for the caller to say which you are
    /// looking at.
    pub fn panes(&self) -> Vec<String> {
        self.needs.keys().cloned().collect()
    }

    /// A need you look at is seen: its tab shows in the focused window and
    /// the desk is not idle-fading (`looked_at`, from the caller). It stays
    /// seen until the session moves on.
    pub fn mark_seen(&mut self, looked_at: &HashMap<String, bool>) {
        for (pane, need) in &mut self.needs {
            if looked_at.get(pane).copied().unwrap_or(false) {
                need.seen = true;
            }
        }
    }

    /// Send the islands: one per need not seen ([`Self::mark_seen`] first);
    /// `place` names a pane `(project, session)`. `known` are every pane's
    /// ids: on the first sync, their islands of a previous run go.
    pub fn sync(
        &mut self,
        looked_at: &HashMap<String, bool>,
        place: &HashMap<String, (String, String)>,
        known: &[String],
    ) {
        self.mark_seen(looked_at);
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
                    body: need.body.clone(),
                    choices: need.offered().iter().map(|c| c.label.clone()).collect(),
                    reply: need.reply(),
                    since: need.since,
                },
            );
        }
        let Ok(mut bus) = sola_kit::app::bus().lock() else {
            return;
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
    }
}

impl Need {
    fn new(state: IslandState, title: String, body: String, since: u64, asked: bool) -> Self {
        Self {
            state,
            title,
            body,
            choices: Vec::new(),
            since,
            seen: false,
            scans: 0,
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
        body: String::new(),
        choices: Vec::new(),
        reply: false,
        since: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_permission_names_its_command() {
        let ask = ask_from_hook(&json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {"command": "cargo make build sola-kit", "description": "Build the kit"}
        }))
        .unwrap();
        assert_eq!(ask.title, "wants to run cargo make build sola-kit");
        assert!(ask.body.starts_with("Build the kit"));
    }

    #[test]
    fn a_question_shows_its_options() {
        let ask = ask_from_hook(&json!({
            "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [{"question": "Which layout?", "options": [
                {"label": "Grid", "description": "Two by two"}, {"label": "Stack"}
            ]}]}
        }))
        .unwrap();
        assert_eq!(ask.title, "asks: Which layout?");
        assert_eq!(ask.body, "Which layout?\n• Grid — Two by two\n• Stack");
    }

    #[test]
    fn the_last_numbered_run_is_the_choices() {
        let screen = "\
 1. an old list
 2. long gone
╭──────────────────────────────╮
│ Bash command                 │
│   cargo make build           │
│ Do you want to proceed?      │
│ ❯ 1. Yes                     │
│   2. Yes, and don't ask again│
│   3. No, and tell Claude what to do differently (esc) │
╰──────────────────────────────╯";
        let c = choices_on_screen(screen);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0], Choice { label: "Yes".into(), key: '1' });
        assert_eq!(c[1].label, "Yes, and don't ask again");
        assert_eq!(c[2].key, '3');
    }

    #[test]
    fn descriptions_between_options_are_skipped() {
        let screen = "Which layout?\n❯ 1. Grid\n     Two by two\n  2. Stack\n  3. Type something.";
        let c = choices_on_screen(screen);
        assert_eq!(c.iter().map(|c| c.key).collect::<String>(), "123");
        assert!(is_free_text(&c[2].label));
    }

    #[test]
    fn a_lone_number_is_not_a_prompt() {
        assert!(choices_on_screen("1. just one line\nand prose").is_empty());
    }

    #[test]
    fn the_last_assistant_message_is_read_from_the_end() {
        let dir = std::env::temp_dir().join(format!("sola-islands-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"old"}]}}"#,
                "\n",
                r#"{"type":"user","message":{"content":"next"}}"#,
                "\n",
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done: built it."},{"type":"tool_use","name":"Bash"}]}}"#,
                "\n",
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}"#,
                "\n"
            ),
        )
        .unwrap();
        assert_eq!(last_assistant_text(&path).as_deref(), Some("Done: built it."));
        // A new prompt with its answer not written yet: nothing (not the
        // turn before's).
        let mut more = std::fs::read_to_string(&path).unwrap();
        more.push_str(r#"{"type":"user","message":{"content":"next please"}}"#);
        more.push('\n');
        std::fs::write(&path, more).unwrap();
        assert_eq!(last_assistant_text(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_need_follows_its_session() {
        let mut islands = Islands::default();
        // Where a session already is makes no island.
        assert!(!islands.track("p", Some(AgentStatus::Done), 0));
        assert!(!islands.track("p", None, 0));
        assert!(!islands.track("p", Some(AgentStatus::Working), 0));
        islands.note_hook(
            "p",
            Some(Ask {
                title: "wants to run ls".into(),
                body: "ls".into(),
            }),
            None,
        );
        assert!(islands.track("p", Some(AgentStatus::Waiting), 1));
        assert!(!islands.track("p", Some(AgentStatus::Waiting), 2));
        assert_eq!(islands.need_mut("p").unwrap().title, "wants to run ls");
        islands.answer("p");
        assert!(islands.need_mut("p").unwrap().seen);
        assert!(!islands.track("p", Some(AgentStatus::Working), 3));
        assert!(islands.need_mut("p").is_none());
        assert!(islands.track("p", Some(AgentStatus::Done), 4));
        assert!(islands.need_mut("p").unwrap().reply());
    }

    #[test]
    fn a_need_looked_at_is_seen_until_it_moves_on() {
        let mut islands = Islands::default();
        islands.track("p", Some(AgentStatus::Working), 0);
        assert!(islands.track("p", Some(AgentStatus::Done), 100));
        // Not looked at (another tab, or the desk fading): its island shows.
        islands.mark_seen(&HashMap::new());
        assert!(!islands.need_mut("p").unwrap().seen);
        let looked: HashMap<String, bool> = [("p".to_string(), true)].into();
        islands.mark_seen(&looked);
        assert!(islands.need_mut("p").unwrap().seen);
        // Working again clears it.
        islands.track("p", Some(AgentStatus::Working), 200);
        assert!(islands.need_mut("p").is_none());
    }

    #[test]
    fn a_tail_skips_trailing_blank_lines() {
        assert_eq!(screen_tail("a\nb\nc\n\n  \n", 2), "b\nc");
    }

    #[test]
    fn a_tail_leaves_out_the_input_box() {
        let screen = "● Built it.\n\n──────────\n❯ \n──────────\n  ⏵⏵ auto mode on";
        assert_eq!(screen_tail(screen, 5), "● Built it.");
    }
}

