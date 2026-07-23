//! sola-agent — kit-native desktop ACP client (default backend: Grok Build).
//!
//! Resume-only in v1: quitting stops the child process; sessions live under
//! `~/.grok/sessions` and can be resumed from Sola or the Grok TUI.
//! Leader-daemon multi-client attach is a future connection mode.

use std::path::PathBuf;
use std::sync::Arc;

use iced::{Element, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings,
};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

mod acp;
mod backend;
mod bridge;
mod overlay;
mod protocol;
mod sessions;
mod view;
mod worker;

use backend::ConnectionMode;
use protocol::{
    AgentCmd, AgentEvent, ConnectionModeLabel, PermissionChoice, SessionSummary, ToolTurn, Turn,
};

const APP_ID: &str = "sola-agent";

#[derive(Debug, Clone)]
struct PendingApproval {
    request_id: u64,
    tool: String,
    preview: String,
    options: Vec<PermissionChoice>,
}

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(&[TopicKind::Theme, TopicKind::MenuAction, TopicKind::CloseApp])
        .app_menu("Agent", [("quit", "Quit Agent", KeyCode::Q.meta())])
        .install();

    bridge::init_channels();
    worker::start(ConnectionMode::v1_default());

    // Kick connection + session list after iced is up.
    bridge::agent_send(AgentCmd::EnsureConnected);
    let cwd = project_cwd();
    bridge::agent_send(AgentCmd::RefreshSessions { cwd: cwd.clone() });

    let app = iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings(APP_ID));
    app.run()
}

fn project_cwd() -> String {
    overlay::load()
        .last_cwd
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| ".".into())
}

struct App {
    theme: Theme,
    turns: Vec<Turn>,
    draft: String,
    streaming: bool,
    pending: Option<PendingApproval>,
    sessions: Vec<SessionSummary>,
    session_id: Option<String>,
    session_title: Option<String>,
    project_root: PathBuf,
    connected: bool,
    backend_label: String,
    connection_mode: ConnectionModeLabel,
    usage_used: Option<u64>,
    usage_size: Option<u64>,
    need_setup: Option<String>,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Acp(AgentEvent),
    DraftChanged(String),
    Send,
    Cancel,
    NewSession,
    SelectSession(String),
    TogglePin(String),
    PermissionPick(String),
    PermissionAllowFirst,
    PermissionDeny,
    Restart,
}

impl App {
    fn new() -> Self {
        let project_root = PathBuf::from(project_cwd());
        Self {
            theme: default_theme(),
            turns: Vec::new(),
            draft: String::new(),
            streaming: false,
            pending: None,
            sessions: sessions::list_for_cwd(&project_root.to_string_lossy()),
            session_id: None,
            session_title: None,
            project_root,
            connected: false,
            backend_label: "Grok".into(),
            connection_mode: ConnectionModeLabel::Local,
            usage_used: None,
            usage_size: None,
            need_setup: None,
        }
    }

    fn title(&self) -> String {
        match (&self.session_title, &self.session_id) {
            (Some(t), _) => format!("Agent — {t}"),
            (None, Some(id)) => format!("Agent — {id}"),
            _ => "Agent".into(),
        }
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            bridge::agent_subscription().map(Msg::Acp),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => {
                if is_self_quit(&m, APP_ID) {
                    bridge::agent_send(AgentCmd::Shutdown);
                    return iced::exit();
                }
                let _ = apply_theme_update(&m, &mut self.theme);
            }
            Msg::Acp(ev) => self.on_event(ev),
            Msg::DraftChanged(s) => self.draft = s,
            Msg::Send => {
                let text = self.draft.trim().to_string();
                if text.is_empty() || self.pending.is_some() {
                    return Task::none();
                }
                self.draft.clear();
                self.streaming = true;
                // Ensure we have a session
                if self.session_id.is_none() {
                    bridge::agent_send(AgentCmd::NewSession {
                        cwd: self.project_root.to_string_lossy().into_owned(),
                    });
                }
                bridge::agent_send(AgentCmd::Send { text });
            }
            Msg::Cancel => {
                bridge::agent_send(AgentCmd::Cancel);
                self.streaming = false;
            }
            Msg::NewSession => {
                if self.streaming || self.pending.is_some() {
                    return Task::none();
                }
                self.turns.clear();
                self.session_id = None;
                self.session_title = None;
                bridge::agent_send(AgentCmd::NewSession {
                    cwd: self.project_root.to_string_lossy().into_owned(),
                });
            }
            Msg::SelectSession(id) => {
                if self.streaming || self.pending.is_some() {
                    return Task::none();
                }
                bridge::agent_send(AgentCmd::LoadSession {
                    id,
                    cwd: self.project_root.to_string_lossy().into_owned(),
                });
            }
            Msg::TogglePin(id) => {
                overlay::toggle_pin(&id);
                self.sessions = sessions::list_for_cwd(&self.project_root.to_string_lossy());
            }
            Msg::PermissionPick(option_id) => {
                if let Some(p) = self.pending.take() {
                    bridge::agent_send(AgentCmd::Permission {
                        request_id: p.request_id,
                        option_id,
                    });
                }
            }
            Msg::PermissionAllowFirst => {
                if let Some(p) = self.pending.take() {
                    let option_id = p
                        .options
                        .iter()
                        .find(|o| {
                            let k = o.kind.to_lowercase();
                            k.contains("allow") && !k.contains("always")
                        })
                        .or_else(|| p.options.first())
                        .map(|o| o.option_id.clone())
                        .unwrap_or_default();
                    if option_id.is_empty() {
                        bridge::agent_send(AgentCmd::PermissionCancel {
                            request_id: p.request_id,
                        });
                    } else {
                        bridge::agent_send(AgentCmd::Permission {
                            request_id: p.request_id,
                            option_id,
                        });
                    }
                }
            }
            Msg::PermissionDeny => {
                if let Some(p) = self.pending.take() {
                    let option_id = p
                        .options
                        .iter()
                        .find(|o| {
                            let k = o.kind.to_lowercase();
                            k.contains("reject") || k.contains("deny")
                        })
                        .map(|o| o.option_id.clone());
                    if let Some(option_id) = option_id {
                        bridge::agent_send(AgentCmd::Permission {
                            request_id: p.request_id,
                            option_id,
                        });
                    } else {
                        bridge::agent_send(AgentCmd::PermissionCancel {
                            request_id: p.request_id,
                        });
                    }
                }
            }
            Msg::Restart => {
                self.need_setup = None;
                bridge::agent_send(AgentCmd::Restart);
                bridge::agent_send(AgentCmd::RefreshSessions {
                    cwd: self.project_root.to_string_lossy().into_owned(),
                });
            }
        }
        Task::none()
    }

    fn on_event(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Connected { backend, mode } => {
                self.connected = true;
                self.backend_label = backend;
                self.connection_mode = mode;
                self.need_setup = None;
            }
            AgentEvent::Disconnected { reason } => {
                self.connected = false;
                self.streaming = false;
                self.turns
                    .push(Turn::Error(format!("Disconnected: {reason}")));
            }
            AgentEvent::NeedSetup { message } => {
                self.connected = false;
                self.need_setup = Some(message);
            }
            AgentEvent::SessionReady { id, title } => {
                self.session_id = Some(id);
                if title.is_some() {
                    self.session_title = title;
                }
            }
            AgentEvent::Transcript { turns } => {
                self.turns = turns;
                self.streaming = false;
                self.pending = None;
            }
            AgentEvent::UserEcho { text } => {
                self.turns.push(Turn::User(text));
            }
            AgentEvent::AgentDelta { text } => {
                self.streaming = true;
                match self.turns.last_mut() {
                    Some(Turn::Assistant(s)) => s.push_str(&text),
                    _ => self.turns.push(Turn::Assistant(text)),
                }
            }
            AgentEvent::ThoughtDelta { text } => {
                self.streaming = true;
                match self.turns.last_mut() {
                    Some(Turn::Thought(s)) => s.push_str(&text),
                    _ => self.turns.push(Turn::Thought(text)),
                }
            }
            AgentEvent::ToolStart {
                call_id,
                tool,
                args,
            } => {
                self.streaming = true;
                self.turns.push(Turn::Tool(ToolTurn {
                    call_id,
                    tool,
                    args,
                    status: "running".into(),
                    output: String::new(),
                }));
            }
            AgentEvent::ToolUpdate {
                call_id,
                status,
                title,
                output,
            } => {
                if let Some(Turn::Tool(t)) = self
                    .turns
                    .iter_mut()
                    .rev()
                    .find(|t| matches!(t, Turn::Tool(tt) if tt.call_id == call_id))
                {
                    if let Some(s) = status {
                        t.status = s;
                    }
                    if let Some(title) = title {
                        t.tool = title;
                    }
                    if let Some(out) = output {
                        t.output = out;
                    }
                }
            }
            AgentEvent::ToolEnd {
                call_id,
                status,
                output,
            } => {
                if let Some(Turn::Tool(t)) = self
                    .turns
                    .iter_mut()
                    .rev()
                    .find(|t| matches!(t, Turn::Tool(tt) if tt.call_id == call_id))
                {
                    t.status = status;
                    if let Some(out) = output {
                        t.output = out;
                    }
                }
            }
            AgentEvent::Plan { entries } => {
                self.turns.push(Turn::Plan(entries));
            }
            AgentEvent::Usage { used, size } => {
                self.usage_used = Some(used);
                if size.is_some() {
                    self.usage_size = size;
                }
            }
            AgentEvent::PermissionRequired {
                request_id,
                tool,
                preview,
                options,
            } => {
                self.pending = Some(PendingApproval {
                    request_id,
                    tool,
                    preview,
                    options,
                });
            }
            AgentEvent::TurnEnded { stop_reason } => {
                self.streaming = false;
                self.pending = None;
                if stop_reason != "end_turn" && stop_reason != "EndTurn" {
                    tracing::info!(%stop_reason, "turn ended");
                }
            }
            AgentEvent::Error { message } => {
                self.streaming = false;
                self.turns.push(Turn::Error(message));
            }
            AgentEvent::SessionsListed { entries } => {
                self.sessions = entries;
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        view::screen(self)
    }
}
