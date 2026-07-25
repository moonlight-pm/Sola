//! sola-agent — kit-native desktop ACP client (default backend: Grok Build).
//!
//! Resume-only in v1: quitting stops the child process; sessions live under
//! `~/.grok/sessions` and can be resumed from Sola or the Grok TUI.
//! Leader-daemon multi-client attach is a future connection mode.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::operation;
use iced::widget::scrollable::RelativeOffset;
use iced::widget::Id as ScrollId;
use iced::{event, mouse, Element, Event, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings,
};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

const SIDEBAR_W_DEFAULT: f32 = 248.0;
const SIDEBAR_W_MIN: f32 = 200.0;
const SIDEBAR_W_MAX: f32 = 420.0;

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
/// Default Grok context window when ACP omits `size`.
const DEFAULT_CONTEXT_SIZE: u64 = 500_000;

#[derive(Debug, Clone)]
struct PendingApproval {
    request_id: u64,
    tool: String,
    preview: String,
    options: Vec<PermissionChoice>,
}

/// New-session project directory picker.
#[derive(Debug, Clone)]
pub(crate) struct ProjectPicker {
    pub(crate) draft: String,
    pub(crate) recent: Vec<String>,
}

/// Inline rename of a session title.
#[derive(Debug, Clone)]
pub(crate) struct RenameState {
    pub(crate) id: String,
    pub(crate) draft: String,
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

pub(crate) fn transcript_scroll_id() -> ScrollId {
    ScrollId::new("agent-transcript")
}

pub(crate) struct App {
    pub(crate) theme: Theme,
    pub(crate) turns: Vec<Turn>,
    pub(crate) draft: String,
    pub(crate) streaming: bool,
    pub(crate) pending: Option<PendingApproval>,
    pub(crate) sessions: Vec<SessionSummary>,
    pub(crate) session_id: Option<String>,
    pub(crate) session_title: Option<String>,
    pub(crate) project_root: PathBuf,
    pub(crate) connected: bool,
    pub(crate) backend_label: String,
    pub(crate) connection_mode: ConnectionModeLabel,
    pub(crate) usage_used: Option<u64>,
    pub(crate) usage_size: Option<u64>,
    pub(crate) need_setup: Option<String>,
    /// Lazy history cursor — absolute byte into updates.jsonl.
    pub(crate) history_start_byte: u64,
    pub(crate) has_older_history: bool,
    pub(crate) loading_older: bool,
    /// Chunks auto-prepended after session open (stops at target item count).
    pub(crate) history_auto_chunks: u32,
    /// Last known relative scroll Y (0 = top). `None` until first scroll notify.
    /// When content does not overflow, iced never fires `on_scroll`.
    pub(crate) scroll_rel_y: Option<f32>,
    /// Auto-scroll transcript to bottom unless user scrolled away.
    pub(crate) stick_to_bottom: bool,
    pub(crate) project_picker: Option<ProjectPicker>,
    pub(crate) rename: Option<RenameState>,
    /// Request snap-to-bottom on next update return.
    pub(crate) scroll_bottom_pending: bool,
    /// Resizable left session column width.
    pub(crate) sidebar_w: f32,
    /// Sidebar session filter (title / path substring).
    pub(crate) session_filter: String,
    pub(crate) dragging_divider: bool,
    pub(crate) last_cursor_x: Option<f32>,
    /// `(cursor_x_at_press, sidebar_w_at_press)`.
    pub(crate) drag_anchor: Option<(f32, f32)>,
    /// Double-click rename: last session row click (id, instant).
    pub(crate) last_session_click: Option<(String, Instant)>,
    /// Sessions section scroll viewport (overflow chips: ↑ N … / ↓ N …).
    pub(crate) session_section_scroll: sola_kit::components::SectionScroll,
}

#[derive(Debug, Clone)]
pub(crate) enum Msg {
    Bus(Arc<Message>),
    Acp(AgentEvent),
    DraftChanged(String),
    Send,
    Cancel,
    NewSession,
    SelectSession(String),
    PermissionPick(String),
    PermissionAllowFirst,
    PermissionDeny,
    Restart,
    /// Scrollable viewport changed (relative Y: 0 top … 1 bottom).
    TranscriptScrolled(f32),
    /// Mouse wheel up over the app — used when content doesn't overflow.
    TranscriptWheelUp,
    /// Explicit "load earlier" control (always works without a scrollbar).
    LoadOlderHistory,
    SessionFilter(String),
    // Project picker
    PickerDraft(String),
    PickerUse,
    PickerPick(String),
    PickerCancel,
    // Rename
    StartRename(String),
    RenameDraft(String),
    RenameCommit,
    RenameCancel,
    // Sidebar resize
    DividerPress,
    CursorMoved(f32),
    CursorReleased,
    /// Periodic list refresh (live TUI dots, ages).
    RefreshSessionsTick,
    /// Sessions fill-section scroll viewport (for overflow chips).
    SessionSectionScroll(sola_kit::components::SectionScroll),
}

impl App {
    fn new() -> Self {
        let project_root = PathBuf::from(project_cwd());
        let sidebar_w = overlay::load()
            .sidebar_w
            .unwrap_or(SIDEBAR_W_DEFAULT)
            .clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
        Self {
            theme: default_theme(),
            turns: Vec::new(),
            draft: String::new(),
            streaming: false,
            pending: None,
            sessions: sessions::list_all(),
            session_id: None,
            session_title: None,
            project_root,
            connected: false,
            backend_label: "Grok".into(),
            connection_mode: ConnectionModeLabel::Local,
            usage_used: None,
            usage_size: None,
            need_setup: None,
            history_auto_chunks: 0,
            scroll_rel_y: None,
            history_start_byte: 0,
            has_older_history: false,
            loading_older: false,
            stick_to_bottom: true,
            project_picker: None,
            rename: None,
            scroll_bottom_pending: false,
            sidebar_w,
            session_filter: String::new(),
            dragging_divider: false,
            last_cursor_x: None,
            drag_anchor: None,
            last_session_click: None,
            session_section_scroll: sola_kit::components::SectionScroll::default(),
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
        let mut subs = vec![
            bus_subscription().map(Msg::Bus),
            bridge::agent_subscription().map(Msg::Acp),
            iced::time::every(Duration::from_secs(8)).map(|_| Msg::RefreshSessionsTick),
        ];
        // Cursor tracking for divider drag; wheel-up for history when no scrollbar.
        subs.push(event::listen_with(|event, _status, _id| match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Msg::CursorMoved(position.x))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(Msg::CursorReleased)
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let up = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y > 0.0,
                    mouse::ScrollDelta::Pixels { y, .. } => y > 0.0,
                };
                if up {
                    Some(Msg::TranscriptWheelUp)
                } else {
                    None
                }
            }
            _ => None,
        }));
        Subscription::batch(subs)
    }

    fn maybe_scroll_bottom(&mut self) -> Task<Msg> {
        if self.scroll_bottom_pending && self.stick_to_bottom {
            self.scroll_bottom_pending = false;
            return operation::snap_to(transcript_scroll_id(), RelativeOffset::END);
        }
        self.scroll_bottom_pending = false;
        Task::none()
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
            Msg::Acp(ev) => {
                let need_scroll = matches!(
                    ev,
                    AgentEvent::Transcript { .. }
                        | AgentEvent::UserEcho { .. }
                        | AgentEvent::AgentDelta { .. }
                        | AgentEvent::ThoughtDelta { .. }
                        | AgentEvent::ToolStart { .. }
                        | AgentEvent::Plan { .. }
                        | AgentEvent::TurnEnded { .. }
                );
                let fill_history = matches!(
                    ev,
                    AgentEvent::Transcript { .. } | AgentEvent::HistoryOlder { .. }
                );
                self.on_event(ev);
                let mut tasks = Vec::new();
                if fill_history {
                    tasks.push(self.maybe_auto_fill_history());
                }
                if need_scroll && self.stick_to_bottom {
                    self.scroll_bottom_pending = true;
                    tasks.push(self.maybe_scroll_bottom());
                }
                return Task::batch(tasks);
            }
            Msg::DraftChanged(s) => self.draft = s,
            Msg::Send => {
                let text = self.draft.trim().to_string();
                if text.is_empty() || self.pending.is_some() || self.streaming {
                    return Task::none();
                }
                self.draft.clear();
                self.streaming = true;
                self.stick_to_bottom = true;
                if self.session_id.is_none() {
                    bridge::agent_send(AgentCmd::NewSession {
                        cwd: self.project_root.to_string_lossy().into_owned(),
                    });
                }
                bridge::agent_send(AgentCmd::Send { text });
                self.scroll_bottom_pending = true;
                return self.maybe_scroll_bottom();
            }
            Msg::Cancel => {
                bridge::agent_send(AgentCmd::Cancel);
                self.streaming = false;
            }
            Msg::NewSession => {
                if self.streaming || self.pending.is_some() {
                    return Task::none();
                }
                let default = self.project_root.to_string_lossy().into_owned();
                self.project_picker = Some(ProjectPicker {
                    draft: default,
                    recent: sessions::recent_project_cwds(),
                });
            }
            Msg::PickerDraft(s) => {
                if let Some(p) = &mut self.project_picker {
                    p.draft = s;
                }
            }
            Msg::PickerPick(cwd) => {
                self.project_picker = None;
                self.start_session_in(cwd);
            }
            Msg::PickerUse => {
                let cwd = self
                    .project_picker
                    .as_ref()
                    .map(|p| p.draft.trim().to_string())
                    .unwrap_or_default();
                self.project_picker = None;
                if !cwd.is_empty() {
                    self.start_session_in(cwd);
                }
            }
            Msg::PickerCancel => {
                self.project_picker = None;
            }
            Msg::SelectSession(id) => {
                // Double-click same row → rename (no pin/edit chrome).
                let now = Instant::now();
                if let Some((ref last_id, t)) = self.last_session_click {
                    if last_id == &id && now.duration_since(t) < Duration::from_millis(450) {
                        self.last_session_click = None;
                        let draft = self
                            .sessions
                            .iter()
                            .find(|s| s.id == id)
                            .map(|s| s.title.clone())
                            .unwrap_or_default();
                        self.rename = Some(RenameState { id, draft });
                        return Task::none();
                    }
                }
                self.last_session_click = Some((id.clone(), now));

                if self.streaming || self.pending.is_some() {
                    return Task::none();
                }
                // Already open — don't reload on the first click of a double.
                if self.session_id.as_deref() == Some(id.as_str()) {
                    return Task::none();
                }
                let cwd = self
                    .sessions
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.cwd.clone())
                    .filter(|c| !c.is_empty())
                    .unwrap_or_else(|| self.project_root.to_string_lossy().into_owned());
                self.project_root = PathBuf::from(&cwd);
                overlay::note_cwd(&cwd);
                self.stick_to_bottom = true;
                self.loading_older = false;
                bridge::agent_send(AgentCmd::LoadSession { id, cwd });
            }
            Msg::StartRename(id) => {
                let draft = self
                    .sessions
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.title.clone())
                    .unwrap_or_default();
                self.rename = Some(RenameState { id, draft });
            }
            Msg::RenameDraft(s) => {
                if let Some(r) = &mut self.rename {
                    r.draft = s;
                }
            }
            Msg::RenameCommit => {
                if let Some(r) = self.rename.take() {
                    let title = r.draft.trim().to_string();
                    overlay::set_title_override(&r.id, &title);
                    if self.session_id.as_deref() == Some(r.id.as_str()) {
                        self.session_title = if title.is_empty() {
                            sessions::title_for(
                                &self.project_root.to_string_lossy(),
                                &r.id,
                            )
                        } else {
                            Some(title.clone())
                        };
                    }
                    self.sessions = sessions::list_all();
                }
            }
            Msg::RenameCancel => {
                self.rename = None;
            }
            Msg::DividerPress => {
                self.dragging_divider = true;
                if let Some(x) = self.last_cursor_x {
                    self.drag_anchor = Some((x, self.sidebar_w));
                }
            }
            Msg::CursorMoved(x) => {
                self.last_cursor_x = Some(x);
                if self.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.drag_anchor {
                        // Left sidebar grows when cursor moves right.
                        let desired = anchor_w + (x - anchor_x);
                        self.sidebar_w = desired.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
                    }
                }
            }
            Msg::CursorReleased => {
                if self.dragging_divider {
                    self.dragging_divider = false;
                    self.drag_anchor = None;
                    overlay::set_sidebar_w(self.sidebar_w);
                }
            }
            Msg::RefreshSessionsTick => {
                // Keep ages + TUI-live dots fresh without requiring a click.
                self.sessions = sessions::list_all();
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
            Msg::TranscriptScrolled(relative_y) => {
                // relative_y: 0.0 = top, 1.0 = bottom (NaN when content fits viewport)
                let y = if relative_y.is_finite() {
                    relative_y
                } else {
                    0.0
                };
                self.scroll_rel_y = Some(y);
                if y < 0.92 {
                    self.stick_to_bottom = false;
                } else {
                    self.stick_to_bottom = true;
                }
                if y < 0.08 {
                    return self.request_older_history();
                }
            }
            Msg::TranscriptWheelUp => {
                // When content is shorter than the pane, iced never scrolls and
                // never fires on_scroll — wheel-up still means "go older".
                let near_top = self.scroll_rel_y.map(|y| y < 0.12).unwrap_or(true);
                if near_top {
                    return self.request_older_history();
                }
            }
            Msg::LoadOlderHistory => {
                return self.request_older_history();
            }
            Msg::SessionFilter(s) => {
                self.session_filter = s;
            }
            Msg::SessionSectionScroll(scroll) => {
                self.session_section_scroll = scroll;
            }
        }
        Task::none()
    }

    fn start_session_in(&mut self, cwd: String) {
        self.project_root = PathBuf::from(&cwd);
        overlay::note_cwd(&cwd);
        self.turns.clear();
        self.session_id = None;
        self.session_title = None;
        self.history_start_byte = 0;
        self.has_older_history = false;
        self.history_auto_chunks = 0;
        self.scroll_rel_y = None;
        self.stick_to_bottom = true;
        self.sessions = sessions::list_all();
        bridge::agent_send(AgentCmd::NewSession { cwd: cwd.clone() });
        bridge::agent_send(AgentCmd::RefreshSessions { cwd });
    }

    fn refresh_title_from_turns(&mut self) {
        let Some(id) = self.session_id.clone() else {
            return;
        };
        sessions::maybe_update_auto_title(&id, &self.turns);
        if overlay::title_override(&id).is_none() {
            if let Some(t) = sessions::derive_title_from_turns(&self.turns) {
                self.session_title = Some(t);
            }
        }
        // Keep sidebar label in sync.
        if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
            if let Some(t) = self.session_title.clone() {
                s.title = t;
            }
        }
    }

    fn request_older_history(&mut self) -> Task<Msg> {
        let Some(id) = self.session_id.clone() else {
            return Task::none();
        };
        if !self.has_older_history || self.loading_older {
            return Task::none();
        }
        self.loading_older = true;
        let cwd = self.project_root.to_string_lossy().into_owned();
        bridge::agent_send(AgentCmd::LoadOlderHistory {
            id,
            cwd,
            before_byte: self.history_start_byte,
        });
        Task::none()
    }

    /// After open / prepend, keep fetching older chunks until the pane has
    /// enough display items (or we hit the auto-chunk cap). Does not depend on
    /// a scrollbar existing.
    fn maybe_auto_fill_history(&mut self) -> Task<Msg> {
        if !self.has_older_history || self.loading_older || self.session_id.is_none() {
            return Task::none();
        }
        if self.history_auto_chunks >= sessions::HISTORY_AUTO_CHUNKS_MAX {
            return Task::none();
        }
        if sessions::display_item_count(&self.turns) >= sessions::HISTORY_INITIAL_ITEMS {
            return Task::none();
        }
        self.history_auto_chunks += 1;
        self.request_older_history()
    }

    fn finalize_open_tools(&mut self) {
        sessions::finalize_tool_statuses(&mut self.turns);
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
                self.session_id = Some(id.clone());
                if let Some(t) = overlay::title_override(&id) {
                    self.session_title = Some(t);
                } else if title.is_some() {
                    self.session_title = title;
                }
            }
            AgentEvent::Transcript {
                turns,
                history_start_byte,
                has_older,
            } => {
                self.turns = turns;
                self.history_start_byte = history_start_byte;
                self.has_older_history = has_older;
                self.loading_older = false;
                self.history_auto_chunks = 0;
                self.scroll_rel_y = None;
                self.streaming = false;
                self.pending = None;
                self.stick_to_bottom = true;
                self.scroll_bottom_pending = true;
                self.refresh_title_from_turns();
                // Do not re-sort ages from summary — list_all uses updates mtime.
                self.sessions = sessions::list_all();
            }
            AgentEvent::HistoryOlder {
                turns,
                history_start_byte,
                has_older,
            } => {
                if !turns.is_empty() {
                    let mut merged = turns;
                    merged.append(&mut self.turns);
                    self.turns = merged;
                }
                self.history_start_byte = history_start_byte;
                self.has_older_history = has_older;
                self.loading_older = false;
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
            AgentEvent::ToolStart { call_id, tool } => {
                self.streaming = true;
                // Metadata only — UI collapses contiguous tools to "N tool uses".
                self.turns.push(Turn::Tool(ToolTurn {
                    call_id,
                    tool,
                    status: "running".into(),
                }));
            }
            AgentEvent::ToolUpdate {
                call_id,
                status,
                title,
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
                }
            }
            AgentEvent::ToolEnd { call_id, status } => {
                if let Some(Turn::Tool(t)) = self
                    .turns
                    .iter_mut()
                    .rev()
                    .find(|t| matches!(t, Turn::Tool(tt) if tt.call_id == call_id))
                {
                    t.status = status;
                }
            }
            AgentEvent::Plan { entries } => {
                self.turns.push(Turn::Plan(entries));
            }
            AgentEvent::Usage { used, size } => {
                self.usage_used = Some(used);
                self.usage_size = size.or(Some(DEFAULT_CONTEXT_SIZE));
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
                self.finalize_open_tools();
                self.refresh_title_from_turns();
                self.sessions = sessions::list_all();
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
