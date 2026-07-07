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

use event::{AgentEvent, NodeId};
use session::{Session, Usage};
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
            _ => Task::none(),
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
}
