//! sola-agent — iced GUI for a focused Sakana Fugu coding agent.
//!
//! Grows the original kit stub into a real client: bus + theme (kit helpers),
//! a background engine worker (event.rs bridge), and a transcript UI. Follows
//! the `sola-terminal` App::new/update/view shape.
#![allow(dead_code)] // interim: Turn/tool fields are wired up in later tasks.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use iced::{Element, Length, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings,
};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

mod engine;
mod event;
mod permit;
mod provider;
mod session;
mod tools;

use event::{AgentCmd, AgentEvent, NodeId};
use session::{Content, Role, Session, Usage};
use tools::ToolDetail;

const APP_ID: &str = "sola-agent";
const DEFAULT_MODEL: &str = "fugu";
const DEFAULT_EFFORT: &str = "high";

/// One display row in the transcript. Driven by `AgentEvent`s, not the persisted
/// session tree (that stays the engine's single-writer store).
#[derive(Debug, Clone)]
enum Turn {
    User(String),
    Assistant { id: NodeId, text: String },
    Reasoning(String),
    Tool(ToolTurn),
    Error(String),
}

#[derive(Debug, Clone)]
struct ToolTurn {
    call_id: String,
    tool: String,
    args: serde_json::Value,
    output: String,
    detail: Option<ToolDetail>,
}

#[derive(Debug, Clone)]
struct PendingApproval {
    call_id: String,
    tool: String,
    preview: String,
}

#[derive(Debug, Clone)]
struct SessionSummary {
    id: String,
    title: String,
    path: PathBuf,
}

/// On-disk credential wrapper. `Encrypted<String>` ciphers the key on
/// human-readable serializers (serde_json here), so the file holds an
/// `age1enc:` blob, not the raw key.
#[derive(serde::Serialize, serde::Deserialize)]
struct Credentials {
    sakana_api_key: sola_core::Encrypted<String>,
}

/// Boot payload assembled in `main` and handed to `App::new` (iced's constructor
/// takes no args, so a static is the fit).
struct Boot {
    session: Arc<Mutex<Session>>,
    model: String,
    effort: String,
    project_root: PathBuf,
    first_run: bool,
}
static BOOT: OnceLock<Boot> = OnceLock::new();

fn credentials_path() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("agent")
        .join("credentials")
}

fn sessions_dir() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("agent")
        .join("sessions")
}

/// Encrypted credentials file first, then the `SAKANA_API_KEY` env var. `None`
/// means first-run: the UI prompts instead of the app crashing.
fn load_api_key() -> Option<String> {
    let path = credentials_path();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<Credentials>(&raw) {
            Ok(creds) => return Some(creds.sakana_api_key.0),
            Err(e) => tracing::warn!(?path, "failed to read agent credentials: {e}"),
        }
    }
    match std::env::var("SAKANA_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

/// Persist the key encrypted at `credentials_path()`.
fn save_api_key(key: &str) -> std::io::Result<()> {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let creds = Credentials {
        sakana_api_key: sola_core::Encrypted(key.to_string()),
    };
    let json = serde_json::to_string_pretty(&creds)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)
}

/// Build the real provider + config and hand the turn loop to a worker thread.
/// Called at boot when a key exists, and again on first-run submit.
fn spawn_engine(
    api_key: String,
    model: String,
    effort: String,
    project_root: PathBuf,
    session: Arc<Mutex<Session>>,
) {
    let provider: Arc<dyn provider::LlmStream + Send + Sync> =
        Arc::new(provider::SakanaProvider {
            base_url: "https://api.sakana.ai/v1".to_string(),
            api_key: api_key.clone(),
        });
    let config = engine::EngineConfig {
        api_key,
        model,
        effort,
        project_root,
        classifier: false,
    };
    engine::start(config, provider, session);
}

/// Scan the sessions dir for `<id>.jsonl` transcripts, loading each for its
/// title. Called off the render path (boot + New/Select), cached in
/// `App::sessions`.
fn list_sessions() -> Vec<SessionSummary> {
    let dir = sessions_dir();
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        match Session::load(&path) {
            Ok(s) => out.push(SessionSummary {
                id: s.id.clone(),
                title: s.title.clone(),
                path,
            }),
            Err(e) => tracing::debug!(?path, "skipping unreadable session: {e}"),
        }
    }
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

/// Rebuild display turns from a persisted session's root..=leaf path.
/// FunctionCall/Output nodes pair back into a single tool row by call_id.
fn turns_from_session(session: &Session) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for node in session.path_to_leaf() {
        match node.content {
            Content::Text(t) => match node.role {
                Role::User => turns.push(Turn::User(t)),
                Role::Assistant => turns.push(Turn::Assistant {
                    id: node.id.clone(),
                    text: t,
                }),
                Role::Tool => turns.push(Turn::Error(t)),
            },
            Content::FunctionCall { call_id, name, arguments } => {
                let args = serde_json::from_str(&arguments).unwrap_or(serde_json::Value::Null);
                turns.push(Turn::Tool(ToolTurn {
                    call_id,
                    tool: name,
                    args,
                    output: String::new(),
                    detail: None,
                }));
            }
            Content::FunctionCallOutput { call_id, output } => {
                let existing = turns.iter_mut().rev().find_map(|t| match t {
                    Turn::Tool(tt) if tt.call_id == call_id => Some(tt),
                    _ => None,
                });
                if let Some(tt) = existing {
                    tt.detail = Some(ToolDetail::Text(output.clone()));
                    tt.output = output;
                } else {
                    turns.push(Turn::Error(format!("orphan tool output {call_id}")));
                }
            }
        }
    }
    turns
}

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(&[TopicKind::Theme, TopicKind::MenuAction, TopicKind::CloseApp])
        .app_menu("Agent", [("quit", "Quit Agent", KeyCode::Q.meta())])
        .install();

    // Wire the UI<->worker channels before anything subscribes or the engine
    // takes the command receiver.
    event::init_channels();

    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let session = Arc::new(Mutex::new(Session::new(project_root.clone())));
    let model = DEFAULT_MODEL.to_string();
    let effort = DEFAULT_EFFORT.to_string();

    let first_run = match load_api_key() {
        Some(api_key) => {
            spawn_engine(
                api_key,
                model.clone(),
                effort.clone(),
                project_root.clone(),
                session.clone(),
            );
            false
        }
        None => {
            tracing::warn!("no Sakana API key found; entering first-run key prompt");
            true
        }
    };

    let _ = BOOT.set(Boot {
        session,
        model,
        effort,
        project_root,
        first_run,
    });

    let app = iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings(APP_ID));
    app.run()
}

struct App {
    theme: Theme,
    session: Arc<Mutex<Session>>,
    turns: Vec<Turn>,
    streaming: Option<NodeId>,
    pending: Option<PendingApproval>,
    model: String,
    effort: String,
    usage: Usage,
    draft: String,
    project_root: PathBuf,
    first_run: bool,
    key_draft: String,
    sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Agent(AgentEvent),
    DraftChanged(String),
    Send,
    Approve,
    Always,
    Deny,
    Abort,
    NewSession,
    SelectSession(PathBuf),
    KeyDraftChanged(String),
    KeySubmit,
}

impl App {
    /// Construct from raw parts. Side-effect-free (no disk scan) so unit tests
    /// can build an `App` without touching `~/.config`.
    fn blank(
        session: Arc<Mutex<Session>>,
        model: String,
        effort: String,
        project_root: PathBuf,
        first_run: bool,
    ) -> Self {
        Self {
            theme: default_theme(),
            session,
            turns: Vec::new(),
            streaming: None,
            pending: None,
            model,
            effort,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
            draft: String::new(),
            project_root,
            first_run,
            key_draft: String::new(),
            sessions: Vec::new(),
        }
    }

    fn new() -> (Self, Task<Msg>) {
        let boot = BOOT
            .get()
            .expect("BOOT must be initialised in main before App::new");
        let mut app = App::blank(
            boot.session.clone(),
            boot.model.clone(),
            boot.effort.clone(),
            boot.project_root.clone(),
            boot.first_run,
        );
        app.sessions = list_sessions();
        (app, Task::none())
    }

    fn title(&self) -> String {
        "Sola Agent".into()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            event::agent_subscription().map(Msg::Agent),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => {
                apply_theme_update(&m, &mut self.theme);
                if is_self_quit(&m, APP_ID) {
                    return iced::exit();
                }
                Task::none()
            }
            Msg::Agent(ev) => {
                self.on_agent(ev);
                Task::none()
            }
            Msg::DraftChanged(v) => {
                self.draft = v;
                Task::none()
            }
            Msg::Send => {
                let text = self.draft.trim().to_string();
                if text.is_empty() || self.first_run {
                    return Task::none();
                }
                self.turns.push(Turn::User(text.clone()));
                self.draft.clear();
                self.streaming = None;
                // Engine is the single writer of the session tree; it appends
                // the user node from this text and drives the turn.
                event::agent_send(AgentCmd::Send {
                    text,
                    branch_from: None,
                });
                Task::none()
            }
            Msg::Approve => {
                self.answer_approval(false, false);
                Task::none()
            }
            Msg::Always => {
                self.answer_approval(true, false);
                Task::none()
            }
            Msg::Deny => {
                self.answer_approval(false, true);
                Task::none()
            }
            Msg::Abort => {
                event::agent_send(AgentCmd::Abort);
                self.streaming = None;
                self.pending = None;
                Task::none()
            }
            Msg::NewSession => {
                let fresh = Session::new(self.project_root.clone());
                if let Ok(mut guard) = self.session.lock() {
                    *guard = fresh;
                }
                self.turns.clear();
                self.streaming = None;
                self.pending = None;
                self.usage = Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                };
                self.sessions = list_sessions();
                Task::none()
            }
            Msg::SelectSession(path) => {
                match Session::load(&path) {
                    Ok(loaded) => {
                        self.turns = turns_from_session(&loaded);
                        if let Ok(mut guard) = self.session.lock() {
                            *guard = loaded;
                        }
                        self.streaming = None;
                        self.pending = None;
                    }
                    Err(e) => tracing::warn!(?path, "failed to load session: {e}"),
                }
                Task::none()
            }
            Msg::KeyDraftChanged(v) => {
                self.key_draft = v;
                Task::none()
            }
            Msg::KeySubmit => {
                let key = self.key_draft.trim().to_string();
                if key.is_empty() {
                    return Task::none();
                }
                if let Err(e) = save_api_key(&key) {
                    tracing::error!("failed to persist Sakana key: {e}");
                    return Task::none();
                }
                spawn_engine(
                    key,
                    self.model.clone(),
                    self.effort.clone(),
                    self.project_root.clone(),
                    self.session.clone(),
                );
                self.first_run = false;
                self.key_draft.clear();
                Task::none()
            }
        }
    }

    /// Fold one streamed agent event into the display transcript.
    fn on_agent(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Delta { node_id, text } => self.apply_delta(&node_id, &text),
            AgentEvent::Reasoning { text } => self.apply_reasoning(&text),
            AgentEvent::ToolStart { call_id, tool, args } => {
                self.streaming = None;
                self.turns.push(Turn::Tool(ToolTurn {
                    call_id,
                    tool,
                    args,
                    output: String::new(),
                    detail: None,
                }));
            }
            AgentEvent::ToolOutput { call_id, chunk } => {
                if let Some(tt) = self.tool_turn_mut(&call_id) {
                    tt.output.push_str(&chunk);
                }
            }
            AgentEvent::ToolEnd { call_id, result } => {
                if let Some(tt) = self.tool_turn_mut(&call_id) {
                    if tt.output.is_empty() {
                        tt.output = result.model_text.clone();
                    }
                    tt.detail = Some(result.ui_detail);
                }
            }
            AgentEvent::ApprovalRequest { call_id, tool, preview } => {
                self.pending = Some(PendingApproval { call_id, tool, preview });
            }
            AgentEvent::TurnEnd { usage } => {
                self.usage.input_tokens += usage.input_tokens;
                self.usage.output_tokens += usage.output_tokens;
                self.streaming = None;
            }
            AgentEvent::Error { message } => {
                self.streaming = None;
                self.turns.push(Turn::Error(message));
            }
        }
    }

    /// Append a text delta to the still-open trailing assistant bubble (matched
    /// by id), or start a fresh streaming bubble. Mirrors `apply_reasoning`'s
    /// tail-only coalescing: only `self.turns.last_mut()` is eligible for the
    /// append, so a `Turn::Tool` (or anything else) sitting at the tail forces
    /// a new bubble instead of splicing text back through it into a stale one.
    fn apply_delta(&mut self, node_id: &str, chunk: &str) {
        if let Some(Turn::Assistant { id, text }) = self.turns.last_mut() {
            if id == node_id {
                text.push_str(chunk);
                self.streaming = Some(node_id.to_string());
                return;
            }
        }
        self.turns.push(Turn::Assistant {
            id: node_id.to_string(),
            text: chunk.to_string(),
        });
        self.streaming = Some(node_id.to_string());
    }

    /// Coalesce reasoning chunks into a single trailing reasoning row.
    fn apply_reasoning(&mut self, chunk: &str) {
        if let Some(Turn::Reasoning(buf)) = self.turns.last_mut() {
            buf.push_str(chunk);
            return;
        }
        self.turns.push(Turn::Reasoning(chunk.to_string()));
    }

    fn tool_turn_mut(&mut self, call_id: &str) -> Option<&mut ToolTurn> {
        self.turns.iter_mut().rev().find_map(|t| match t {
            Turn::Tool(tt) if tt.call_id == call_id => Some(tt),
            _ => None,
        })
    }

    /// Resolve the pending approval and forward the decision to the worker.
    fn answer_approval(&mut self, remember: bool, deny: bool) {
        let Some(p) = self.pending.take() else {
            return;
        };
        if deny {
            event::agent_send(AgentCmd::Deny {
                call_id: p.call_id,
                reason: None,
            });
        } else {
            event::agent_send(AgentCmd::Approve {
                call_id: p.call_id,
                remember,
            });
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        iced::widget::container(iced::widget::text("sola-agent"))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    pub(crate) fn blank_app(first_run: bool) -> App {
        let session = Arc::new(Mutex::new(session::Session::new(PathBuf::from(
            "/tmp/sola-agent-test",
        ))));
        App::blank(
            session,
            "fugu".into(),
            "high".into(),
            PathBuf::from("/tmp"),
            first_run,
        )
    }

    #[test]
    fn blank_starts_empty() {
        let app = blank_app(true);
        assert!(app.turns.is_empty());
        assert!(app.first_run);
        assert!(app.streaming.is_none());
        assert!(app.pending.is_none());
        assert_eq!(app.usage.input_tokens, 0);
        assert_eq!(app.usage.output_tokens, 0);
    }

    #[test]
    fn delta_appends_to_streaming_node() {
        let mut app = blank_app(false);
        let _ = app.update(Msg::Agent(AgentEvent::Delta {
            node_id: "n1".into(),
            text: "Hel".into(),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::Delta {
            node_id: "n1".into(),
            text: "lo".into(),
        }));
        assert_eq!(app.streaming.as_deref(), Some("n1"));
        match app.turns.as_slice() {
            [Turn::Assistant { id, text }] => {
                assert_eq!(id, "n1");
                assert_eq!(text, "Hello");
            }
            other => panic!("expected one assistant turn, got {other:?}"),
        }
    }

    #[test]
    fn tool_output_appends_and_end_sets_detail() {
        let mut app = blank_app(false);
        let _ = app.update(Msg::Agent(AgentEvent::ToolStart {
            call_id: "c1".into(),
            tool: "bash".into(),
            args: serde_json::json!({"cmd": "ls"}),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::ToolOutput {
            call_id: "c1".into(),
            chunk: "a\n".into(),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::ToolOutput {
            call_id: "c1".into(),
            chunk: "b\n".into(),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::ToolEnd {
            call_id: "c1".into(),
            result: tools::ToolResult {
                model_text: "a\nb\n".into(),
                ui_detail: tools::ToolDetail::Bash {
                    code: 0,
                    stdout: "a\nb\n".into(),
                    stderr: String::new(),
                },
            },
        }));
        match app.turns.as_slice() {
            [Turn::Tool(tt)] => {
                assert_eq!(tt.output, "a\nb\n");
                assert!(matches!(tt.detail, Some(tools::ToolDetail::Bash { code: 0, .. })));
            }
            other => panic!("expected one tool turn, got {other:?}"),
        }
    }


    /// Regression for the Task 27 latent-corruption finding: a delta after an
    /// interleaved tool call must start a NEW assistant bubble, not splice
    /// into the pre-tool bubble found by scanning back past the tool turn.
    #[test]
    fn delta_after_tool_starts_new_bubble() {
        let mut app = blank_app(false);
        let _ = app.update(Msg::Agent(AgentEvent::Delta {
            node_id: "n1".into(),
            text: "a".into(),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::ToolStart {
            call_id: "c1".into(),
            tool: "bash".into(),
            args: serde_json::json!({"cmd": "ls"}),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::ToolEnd {
            call_id: "c1".into(),
            result: tools::ToolResult {
                model_text: "ok".into(),
                ui_detail: tools::ToolDetail::Bash {
                    code: 0,
                    stdout: "ok".into(),
                    stderr: String::new(),
                },
            },
        }));
        // Same node_id as the pre-tool delta — a naive back-scan would find
        // and append into the "a" bubble straight through the tool turn.
        let _ = app.update(Msg::Agent(AgentEvent::Delta {
            node_id: "n1".into(),
            text: "b".into(),
        }));
        match app.turns.as_slice() {
            [Turn::Assistant { id: id1, text: t1 }, Turn::Tool(tt), Turn::Assistant { id: id2, text: t2 }] =>
            {
                assert_eq!(id1, "n1");
                assert_eq!(t1, "a");
                assert_eq!(tt.call_id, "c1");
                assert_eq!(id2, "n1");
                assert_eq!(t2, "b");
            }
            other => panic!("expected assistant, tool, assistant turns, got {other:?}"),
        }
    }
}
