//! sola-agent — kit-native desktop ACP client for Grok Build.
//!
//! Always attaches to a shared **Grok leader** (`grok agent --leader stdio`
//! bridge → `~/.grok/leader.sock`). The leader is owned outside Sola
//! (user systemd unit `grok-leader.service`); quitting this app does not
//! stop the agent. Sessions live under `~/.grok/sessions` and are shared
//! live with the Grok TUI when both use the same leader.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use std::sync::Arc as StdArc; // text_editor paste payload

use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::widget::operation;
use iced::widget::scrollable::RelativeOffset;
use iced::widget::text_editor;
use iced::widget::Id as ScrollId;
use iced::{event, mouse, Element, Event, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::{MenuDefinition, MenuItem, Topic, TopicKind};
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
mod transcript_cache;
mod version;
mod view;
mod worker;

use transcript_cache::{CachedTranscript, TranscriptCache};

use backend::ConnectionMode;
use protocol::{
    AgentCmd, AgentEvent, ConnectionModeLabel, EffortOption, PermissionChoice, PermissionMode,
    SessionSummary, ToolTurn, Turn,
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

/// Bulk-delete modal phase.
#[derive(Debug, Clone)]
pub(crate) enum BulkDeletePhase {
    Idle,
    Confirm,
    Deleting {
        done: u32,
        total: u32,
        last_id: String,
    },
    Done {
        deleted: u32,
        failed: u32,
        errors: Vec<String>,
    },
}

/// Bulk-delete panel state (Agent → Bulk Delete…).
#[derive(Debug, Clone)]
pub(crate) struct BulkDeletePanel {
    pub(crate) criteria: sessions::BulkDeleteCriteria,
    /// UI toggle for “keep open session” (criteria.keep_open_id mirrors it).
    pub(crate) keep_open: bool,
    pub(crate) preview: sessions::BulkDeletePreview,
    pub(crate) phase: BulkDeletePhase,
}

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(&[TopicKind::Theme, TopicKind::MenuAction, TopicKind::CloseApp])
        // Agent app menu (product actions).
        .app_menu_definition(MenuDefinition {
            label: "Agent".into(),
            items: vec![
                MenuItem::Action {
                    id: "bulk_delete".into(),
                    label: "Bulk Delete…".into(),
                    shortcut: None,
                    disabled: false,
                    checked: false,
                },
                MenuItem::Divider,
                MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Agent".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                },
            ],
        })
        // Edit menu — same contract as sola-terminal (shell routes chords here).
        .app_menu_definition(MenuDefinition {
            label: "Edit".into(),
            items: vec![
                MenuItem::Action {
                    id: "cut".into(),
                    label: "Cut".into(),
                    shortcut: Some(KeyCode::X.meta()),
                    disabled: false,
                    checked: false,
                },
                MenuItem::Action {
                    id: "copy".into(),
                    label: "Copy".into(),
                    shortcut: Some(KeyCode::C.meta()),
                    disabled: false,
                    checked: false,
                },
                MenuItem::Action {
                    id: "paste".into(),
                    label: "Paste".into(),
                    shortcut: Some(KeyCode::V.meta()),
                    disabled: false,
                    checked: false,
                },
                MenuItem::Divider,
                MenuItem::Action {
                    id: "select_all".into(),
                    label: "Select All".into(),
                    shortcut: Some(KeyCode::A.meta()),
                    disabled: false,
                    checked: false,
                },
            ],
        })
        .install();

    bridge::init_channels();
    worker::start(ConnectionMode::default_mode());
    version::start_update_watcher();

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
    /// Multi-line composer buffer (Enter sends, Shift+Enter newline).
    pub(crate) draft: text_editor::Content,
    pub(crate) streaming: bool,
    pub(crate) pending: Option<PendingApproval>,
    pub(crate) sessions: Vec<SessionSummary>,
    pub(crate) session_id: Option<String>,
    pub(crate) session_title: Option<String>,
    /// True once ACP `session/load` (or new) finished for `session_id`.
    /// Live stream events are ignored until then so a prior session cannot
    /// paint into an optimistically selected transcript.
    pub(crate) acp_attached: bool,
    /// True while disk history for the selected session is loading off-thread.
    /// Sidebar selection already flipped; transcript shows a light placeholder.
    pub(crate) content_loading: bool,
    /// Bumps on every session select so late async loads are dropped.
    content_load_gen: u64,
    /// In-memory bounce cache (TTL ~1 day, LRU capped).
    transcript_cache: TranscriptCache,
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
    pub(crate) bulk_delete: Option<BulkDeletePanel>,
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
    /// Hovered session row id (hover-only trash control).
    pub(crate) session_hover: Option<String>,
    /// Two-click delete: first trash click arms this id; second deletes.
    pub(crate) delete_armed: Option<String>,
    /// Permission mode (default always-approve).
    pub(crate) permission_mode: PermissionMode,
    /// Reasoning effort options from the active model.
    pub(crate) efforts: Vec<EffortOption>,
    pub(crate) effort_id: Option<String>,
    pub(crate) model_id: Option<String>,
    /// Leader / bridge agent version (`initialize` or update check).
    pub(crate) grok_version: Option<String>,
    pub(crate) grok_latest: Option<String>,
    pub(crate) grok_update_available: bool,
    /// Wayland often omits SHIFT on the Enter event itself (see sola-terminal).
    /// Track Shift key down/up so Shift+Enter = newline is reliable.
    pub(crate) shift_held: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum Msg {
    Bus(Arc<Message>),
    Acp(AgentEvent),
    DraftAction(text_editor::Action),
    Send,
    /// Clipboard paste result (Edit → Paste / ⌘V).
    ClipboardPasted(Option<String>),
    /// Shift key down/up (Wayland Enter often lacks SHIFT in modifiers).
    ShiftHeld(bool),
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
    /// Async first-paint history for a session (cache miss path).
    TranscriptLoaded {
        load_gen: u64,
        id: String,
        cwd: String,
        slice: sessions::HistorySlice,
        file_len: u64,
    },
    /// Next-frame apply of a cache hit (keeps SelectSession light so the
    /// sidebar selection paints before markdown layout runs).
    ApplyCachedTranscript {
        load_gen: u64,
        id: String,
        cached: CachedTranscript,
    },
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
    /// Periodic list refresh (activity dots, ages).
    RefreshSessionsTick,
    /// Sessions fill-section scroll viewport (for overflow chips).
    SessionSectionScroll(sola_kit::components::SectionScroll),
    // Bulk delete panel (opened via Agent menu → Bulk Delete…)
    BulkAge(sessions::BulkAge),
    BulkKeepPinned(bool),
    BulkKeepLive(bool),
    BulkKeepOpen(bool),
    BulkOnlyNoise(bool),
    BulkAskConfirm,
    BulkBack,
    BulkConfirmDelete,
    BulkCancel,
    /// Footer: cycle / pick permission mode.
    SetPermissionMode(PermissionMode),
    /// Footer: pick reasoning effort id.
    SetEffort(String),
    /// Sidebar row hover (for trash visibility).
    SessionHover(Option<String>),
    /// Trash control: arm on first click, delete on second (same id).
    SessionDeleteClick(String),
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
            draft: text_editor::Content::new(),
            streaming: false,
            pending: None,
            sessions: sessions::list_all(),
            session_id: None,
            session_title: None,
            acp_attached: false,
            content_loading: false,
            content_load_gen: 0,
            transcript_cache: TranscriptCache::default(),
            project_root,
            connected: false,
            backend_label: "Grok".into(),
            connection_mode: ConnectionModeLabel::Leader,
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
            bulk_delete: None,
            scroll_bottom_pending: false,
            sidebar_w,
            session_filter: String::new(),
            dragging_divider: false,
            last_cursor_x: None,
            drag_anchor: None,
            last_session_click: None,
            session_section_scroll: sola_kit::components::SectionScroll::default(),
            session_hover: None,
            delete_armed: None,
            permission_mode: PermissionMode::default_mode(),
            efforts: Vec::new(),
            effort_id: None,
            model_id: None,
            grok_version: None,
            grok_latest: None,
            grok_update_available: false,
            shift_held: false,
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
        let subs = [
            bus_subscription().map(Msg::Bus),
            bridge::agent_subscription().map(Msg::Acp),
            iced::time::every(Duration::from_secs(8)).map(|_| Msg::RefreshSessionsTick),
            // Cursor tracking for divider drag; wheel-up for history when no scrollbar.
            // Also track Shift held — Wayland Enter events often omit the SHIFT mask.
            event::listen_with(|event, _status, _id| match event {
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
                Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, .. }) => {
                    if is_shift_key(&key, physical_key) {
                        Some(Msg::ShiftHeld(true))
                    } else {
                        None
                    }
                }
                Event::Keyboard(keyboard::Event::KeyReleased { key, physical_key, .. }) => {
                    if is_shift_key(&key, physical_key) {
                        Some(Msg::ShiftHeld(false))
                    } else {
                        None
                    }
                }
                _ => None,
            }),
        ];
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

    /// Submit the composer (Enter / Send button / menu).
    fn submit_draft(&mut self) -> Task<Msg> {
        let text = self.draft.text();
        let text = text.trim().to_string();
        if text.is_empty() || self.pending.is_some() || self.streaming {
            return Task::none();
        }
        self.draft = text_editor::Content::new();
        self.streaming = true;
        self.stick_to_bottom = true;
        if self.session_id.is_none() {
            bridge::agent_send(AgentCmd::NewSession {
                cwd: self.project_root.to_string_lossy().into_owned(),
            });
        }
        bridge::agent_send(AgentCmd::Send { text });
        self.scroll_bottom_pending = true;
        self.maybe_scroll_bottom()
    }

    /// Menubar / shortcut actions (shell delivers MenuAction for chords).
    fn on_menu_action(&mut self, action: &str) -> Task<Msg> {
        match action {
            "bulk_delete" => {
                self.open_bulk_delete();
                Task::none()
            }
            "cut" => self.edit_cut(),
            "copy" => self.edit_copy(),
            "paste" => self.edit_paste(),
            "select_all" => {
                if self.pending.is_none() {
                    self.draft.perform(text_editor::Action::SelectAll);
                }
                Task::none()
            }
            "quit" => {
                // Shell also emits CloseApp; handle explicit menu quit.
                bridge::agent_send(AgentCmd::Shutdown);
                iced::exit()
            }
            _ => Task::none(),
        }
    }

    fn edit_copy(&self) -> Task<Msg> {
        if let Some(sel) = self.draft.selection() {
            if !sel.is_empty() {
                return iced::clipboard::write(sel);
            }
        }
        Task::none()
    }

    fn edit_cut(&mut self) -> Task<Msg> {
        if self.pending.is_some() {
            return Task::none();
        }
        let Some(sel) = self.draft.selection() else {
            return Task::none();
        };
        if sel.is_empty() {
            return Task::none();
        }
        self.draft
            .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
        iced::clipboard::write(sel)
    }

    fn edit_paste(&self) -> Task<Msg> {
        if self.pending.is_some() {
            return Task::none();
        }
        iced::clipboard::read().map(Msg::ClipboardPasted)
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => {
                if is_self_quit(&m, APP_ID) {
                    bridge::agent_send(AgentCmd::Shutdown);
                    return iced::exit();
                }
                let _ = apply_theme_update(&m, &mut self.theme);
                if let Some(Topic::MenuAction(p)) = Topic::parse(&m) {
                    if p.app_id == APP_ID {
                        return self.on_menu_action(&p.action_id);
                    }
                }
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
                    AgentEvent::Transcript {
                        from_watch: false,
                        ..
                    } | AgentEvent::HistoryOlder { .. }
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
            Msg::DraftAction(action) => {
                // Plain Enter submits; Shift+Enter inserts a newline.
                // Intercept here (not key_binding Custom) so submit rides the
                // same proven Action path that typing uses.
                if matches!(
                    action,
                    text_editor::Action::Edit(text_editor::Edit::Enter)
                ) && !self.shift_held
                {
                    return self.submit_draft();
                }
                self.draft.perform(action);
            }
            Msg::Send => {
                return self.submit_draft();
            }
            Msg::ClipboardPasted(text) => {
                let Some(text) = text else {
                    return Task::none();
                };
                if self.pending.is_some() {
                    return Task::none();
                }
                self.draft.perform(text_editor::Action::Edit(
                    text_editor::Edit::Paste(StdArc::new(text)),
                ));
            }
            Msg::ShiftHeld(held) => {
                self.shift_held = held;
            }
            Msg::Cancel => {
                bridge::agent_send(AgentCmd::Cancel);
                self.streaming = false;
            }
            Msg::NewSession => {
                // Always allow opening the picker — busy/console-watch
                // `streaming` must not trap chrome. In-flight work is
                // abandoned only when the user actually starts a session.
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

                // Already open — don't reload on the first click of a double.
                if self.session_id.as_deref() == Some(id.as_str()) {
                    return Task::none();
                }
                return self.switch_to_session(id);
            }
            Msg::TranscriptLoaded {
                load_gen,
                id,
                cwd: _,
                slice,
                file_len,
            } => {
                if load_gen != self.content_load_gen
                    || self.session_id.as_deref() != Some(id.as_str())
                {
                    return Task::none();
                }
                self.turns = slice.turns;
                self.history_start_byte = slice.start_byte;
                self.has_older_history = slice.has_older;
                self.history_auto_chunks = sessions::HISTORY_AUTO_CHUNKS_MAX;
                self.loading_older = false;
                self.content_loading = false;
                self.scroll_rel_y = None;
                self.stick_to_bottom = true;
                self.scroll_bottom_pending = true;
                self.refresh_title_from_turns();
                self.cache_current_session(file_len);
                return self.maybe_scroll_bottom();
            }
            Msg::ApplyCachedTranscript {
                load_gen,
                id,
                cached,
            } => {
                if load_gen != self.content_load_gen
                    || self.session_id.as_deref() != Some(id.as_str())
                {
                    return Task::none();
                }
                self.apply_cached(cached);
                if self.stick_to_bottom {
                    return self.maybe_scroll_bottom();
                }
                return Task::none();
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
                    self.sessions = sessions::merge_list(&self.sessions);
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
                // Metadata only — do not re-sort by activity (rows thrash mid-turn).
                self.sessions = sessions::merge_list(&self.sessions);
                // Keep busy dots consistent for the open ACP session.
                if let Some(id) = self.session_id.as_deref() {
                    if self.streaming {
                        if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
                            s.busy = true;
                        }
                    }
                }
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
                    if let Some(option_id) = pick_allow_option(&p.options) {
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
                // List length may change — keep offset in range.
                self.session_section_scroll = self.session_section_scroll.clamped();
            }
            Msg::SessionSectionScroll(scroll) => {
                // Skip no-op updates so chip math doesn't thrash redraws.
                let scroll = scroll.clamped();
                if self.session_section_scroll != scroll {
                    self.session_section_scroll = scroll;
                }
            }
            Msg::BulkAge(age) => {
                if let Some(p) = &mut self.bulk_delete {
                    if matches!(p.phase, BulkDeletePhase::Deleting { .. }) {
                        return Task::none();
                    }
                    p.criteria.age = age;
                    p.phase = BulkDeletePhase::Idle;
                    self.refresh_bulk_preview();
                }
            }
            Msg::BulkKeepPinned(v) => {
                if let Some(p) = &mut self.bulk_delete {
                    if matches!(p.phase, BulkDeletePhase::Deleting { .. }) {
                        return Task::none();
                    }
                    p.criteria.keep_pinned = v;
                    p.phase = BulkDeletePhase::Idle;
                    self.refresh_bulk_preview();
                }
            }
            Msg::BulkKeepLive(v) => {
                if let Some(p) = &mut self.bulk_delete {
                    if matches!(p.phase, BulkDeletePhase::Deleting { .. }) {
                        return Task::none();
                    }
                    p.criteria.keep_live = v;
                    p.phase = BulkDeletePhase::Idle;
                    self.refresh_bulk_preview();
                }
            }
            Msg::BulkKeepOpen(v) => {
                if let Some(p) = &mut self.bulk_delete {
                    if matches!(p.phase, BulkDeletePhase::Deleting { .. }) {
                        return Task::none();
                    }
                    p.keep_open = v;
                    p.criteria.keep_open_id = if v {
                        self.session_id.clone()
                    } else {
                        None
                    };
                    p.phase = BulkDeletePhase::Idle;
                    self.refresh_bulk_preview();
                }
            }
            Msg::BulkOnlyNoise(v) => {
                if let Some(p) = &mut self.bulk_delete {
                    if matches!(p.phase, BulkDeletePhase::Deleting { .. }) {
                        return Task::none();
                    }
                    p.criteria.only_noise_paths = v;
                    p.phase = BulkDeletePhase::Idle;
                    self.refresh_bulk_preview();
                }
            }
            Msg::BulkAskConfirm => {
                if let Some(p) = &mut self.bulk_delete {
                    if !p.preview.candidates.is_empty() {
                        p.phase = BulkDeletePhase::Confirm;
                    }
                }
            }
            Msg::BulkBack => {
                if let Some(p) = &mut self.bulk_delete {
                    if matches!(p.phase, BulkDeletePhase::Confirm) {
                        p.phase = BulkDeletePhase::Idle;
                    }
                }
            }
            Msg::BulkConfirmDelete => {
                let Some(p) = self.bulk_delete.as_mut() else {
                    return Task::none();
                };
                if !matches!(p.phase, BulkDeletePhase::Confirm) {
                    return Task::none();
                }
                let ids: Vec<String> = p.preview.candidates.iter().map(|c| c.id.clone()).collect();
                if ids.is_empty() {
                    return Task::none();
                }
                let total = ids.len() as u32;
                p.phase = BulkDeletePhase::Deleting {
                    done: 0,
                    total,
                    last_id: String::new(),
                };
                bridge::agent_send(AgentCmd::BulkDelete { ids });
            }
            Msg::BulkCancel => {
                // Allow close after done; block only while deleting.
                if let Some(p) = &self.bulk_delete {
                    if matches!(p.phase, BulkDeletePhase::Deleting { .. }) {
                        return Task::none();
                    }
                }
                self.bulk_delete = None;
            }
            Msg::SetPermissionMode(mode) => {
                self.permission_mode = mode;
                if self.session_id.is_some() {
                    bridge::agent_send(AgentCmd::SetPermissionMode {
                        mode_id: mode.as_mode_id().to_string(),
                    });
                }
            }
            Msg::SetEffort(id) => {
                self.effort_id = Some(id.clone());
                if self.session_id.is_some() {
                    // Effort also rides `session/set_mode` on Grok — re-apply
                    // permission mode after so effort does not clobber YOLO.
                    bridge::agent_send(AgentCmd::SetEffort { effort_id: id });
                    bridge::agent_send(AgentCmd::SetPermissionMode {
                        mode_id: self.permission_mode.as_mode_id().to_string(),
                    });
                }
            }
            Msg::SessionHover(id) => {
                self.session_hover = id.clone();
                // Leaving the row (or switching rows) clears arm — two
                // clicks must be on the same visible trash control.
                if let Some(armed) = self.delete_armed.as_ref() {
                    if id.as_ref() != Some(armed) {
                        self.delete_armed = None;
                    }
                }
            }
            Msg::SessionDeleteClick(id) => {
                if self.delete_armed.as_deref() == Some(id.as_str()) {
                    self.delete_armed = None;
                    let was_open = self.session_id.as_deref() == Some(id.as_str());
                    // Sync delete on worker; refresh list after.
                    bridge::agent_send(AgentCmd::BulkDelete { ids: vec![id.clone()] });
                    self.transcript_cache.remove(&id);
                    if was_open {
                        self.session_id = None;
                        self.session_title = None;
                        self.acp_attached = false;
                        self.content_loading = false;
                        self.turns.clear();
                        self.history_start_byte = 0;
                        self.has_older_history = false;
                        self.streaming = false;
                        self.pending = None;
                    }
                } else {
                    self.delete_armed = Some(id);
                }
            }
        }
        Task::none()
    }

    fn open_bulk_delete(&mut self) {
        // Don't stack over a mid-delete; allow reopening otherwise.
        if let Some(p) = &self.bulk_delete {
            if matches!(p.phase, BulkDeletePhase::Deleting { .. }) {
                return;
            }
        }
        let keep_open = true;
        let mut criteria = sessions::BulkDeleteCriteria::default();
        criteria.keep_open_id = if keep_open {
            self.session_id.clone()
        } else {
            None
        };
        let preview = sessions::bulk_delete_preview(&criteria);
        self.project_picker = None;
        self.rename = None;
        self.bulk_delete = Some(BulkDeletePanel {
            criteria,
            keep_open,
            preview,
            phase: BulkDeletePhase::Idle,
        });
    }

    fn refresh_bulk_preview(&mut self) {
        let Some(p) = self.bulk_delete.as_mut() else {
            return;
        };
        p.criteria.keep_open_id = if p.keep_open {
            self.session_id.clone()
        } else {
            None
        };
        p.preview = sessions::bulk_delete_preview(&p.criteria);
    }

    fn start_session_in(&mut self, cwd: String) {
        if let Some(prev) = self.session_id.clone() {
            let prev_cwd = self.project_root.to_string_lossy().into_owned();
            self.cache_current_session(sessions::updates_file_len(&prev_cwd, &prev));
        }
        self.abandon_turn_for_switch();
        self.project_root = PathBuf::from(&cwd);
        overlay::note_cwd(&cwd);
        self.turns.clear();
        self.session_id = None;
        self.session_title = None;
        self.acp_attached = false;
        self.content_loading = false;
        self.content_load_gen = self.content_load_gen.wrapping_add(1);
        self.history_start_byte = 0;
        self.has_older_history = false;
        self.history_auto_chunks = 0;
        self.scroll_rel_y = None;
        self.stick_to_bottom = true;
        self.sessions = sessions::list_all();
        bridge::agent_send(AgentCmd::NewSession { cwd: cwd.clone() });
        bridge::agent_send(AgentCmd::RefreshSessions { cwd });
    }

    /// Sidebar click path: selection + chrome this frame; content from cache
    /// or a background disk load (never block the UI thread on parse/fill).
    fn switch_to_session(&mut self, id: String) -> Task<Msg> {
        // Park the outgoing session so bounce-back is free.
        if let Some(prev) = self.session_id.clone() {
            let prev_cwd = self.project_root.to_string_lossy().into_owned();
            let file_len = sessions::updates_file_len(&prev_cwd, &prev);
            self.cache_current_session(file_len);
        }

        self.abandon_turn_for_switch();
        let summary = self.sessions.iter().find(|s| s.id == id);
        let title = summary.map(|s| s.title.clone());
        let cwd = summary
            .map(|s| s.cwd.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| self.project_root.to_string_lossy().into_owned());
        self.project_root = PathBuf::from(&cwd);
        overlay::note_cwd(&cwd);

        // Instant selection chrome — must not wait on I/O or markdown.
        self.session_id = Some(id.clone());
        self.acp_attached = false;
        self.session_title = overlay::title_override(&id).or(title);
        self.pending = None;
        self.streaming = false;
        self.loading_older = false;
        self.content_load_gen = self.content_load_gen.wrapping_add(1);
        let load_gen = self.content_load_gen;

        // Attach leader for prompt ownership in the background.
        bridge::agent_send(AgentCmd::LoadSession {
            id: id.clone(),
            cwd: cwd.clone(),
        });

        // Always blank the pane this frame so selection chrome is the only
        // work before paint. Cache restore / disk load land on a later msg.
        self.turns.clear();
        self.history_start_byte = 0;
        self.has_older_history = false;
        self.history_auto_chunks = 0;
        self.draft = text_editor::Content::new();
        self.scroll_rel_y = None;
        self.stick_to_bottom = true;
        self.content_loading = true;

        let file_len = sessions::updates_file_len(&cwd, &id);
        if let Some(cached) = self.transcript_cache.get_fresh(&id, file_len) {
            // Next frame only — never rebuild markdown in the same update as
            // the sidebar selection flip.
            return Task::done(Msg::ApplyCachedTranscript {
                load_gen,
                id,
                cached,
            });
        }

        let load_id = id.clone();
        let load_cwd = cwd;
        let (tx, rx) = iced::futures::channel::oneshot::channel();
        std::thread::Builder::new()
            .name("sola-agent-hist".into())
            .spawn(move || {
                let slice = sessions::load_for_display(&load_cwd, &load_id);
                let len = sessions::updates_file_len(&load_cwd, &load_id);
                let _ = tx.send((load_id, load_cwd, slice, len));
            })
            .expect("spawn history load");
        Task::perform(
            async move {
                rx.await.unwrap_or_else(|_| {
                    (
                        String::new(),
                        String::new(),
                        sessions::HistorySlice {
                            turns: Vec::new(),
                            start_byte: 0,
                            has_older: false,
                        },
                        0,
                    )
                })
            },
            move |(id, cwd, slice, file_len)| Msg::TranscriptLoaded {
                load_gen,
                id,
                cwd,
                slice,
                file_len,
            },
        )
    }

    fn apply_cached(&mut self, cached: CachedTranscript) {
        self.turns = Arc::try_unwrap(cached.turns).unwrap_or_else(|a| (*a).clone());
        self.history_start_byte = cached.history_start_byte;
        self.has_older_history = cached.has_older_history;
        if cached.session_title.is_some() {
            self.session_title = cached.session_title;
        }
        self.draft = text_editor::Content::with_text(&cached.draft);
        self.scroll_rel_y = cached.scroll_rel_y;
        self.stick_to_bottom = cached.stick_to_bottom;
        self.history_auto_chunks = sessions::HISTORY_AUTO_CHUNKS_MAX;
        self.content_loading = false;
        self.scroll_bottom_pending = cached.stick_to_bottom;
        self.loading_older = false;
    }

    fn cache_current_session(&mut self, file_len: u64) {
        let Some(id) = self.session_id.clone() else {
            return;
        };
        if id.is_empty() || self.content_loading {
            // Don't cache an empty "still loading" placeholder over real data.
            return;
        }
        let draft = self.draft.text();
        self.transcript_cache.insert(
            id,
            CachedTranscript::new(
                self.turns.clone(),
                self.history_start_byte,
                self.has_older_history,
                self.session_title.clone(),
                draft,
                self.scroll_rel_y,
                self.stick_to_bottom,
                file_len,
            ),
        );
    }

    /// Drop local turn/approval chrome so the user can switch sessions or
    /// start a new one.
    ///
    /// **Do not** send `session/cancel` here. sola-agent shares the Grok
    /// leader with the TUI (and other attaches); cancel is global to the
    /// session. Leaving a tab must not kill a turn still running in another
    /// client — only the explicit Cancel control may do that.
    ///
    /// Pending permission strips are ours to resolve (we received the RPC).
    /// Cancel the request so the leader is not stuck if no other client will
    /// answer; that is narrower than aborting the whole turn.
    fn abandon_turn_for_switch(&mut self) {
        if let Some(p) = self.pending.take() {
            bridge::agent_send(AgentCmd::PermissionCancel {
                request_id: p.request_id,
            });
        }
        self.streaming = false;
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
        // Disk-only — do not wait on the ACP worker (which may be blocked on
        // session/load). Same parse path as the worker's LoadOlderHistory.
        let cwd = self.project_root.to_string_lossy().into_owned();
        let before = self.history_start_byte;
        let slice = sessions::history_before(&cwd, &id, before);
        if !slice.turns.is_empty() {
            let mut merged = slice.turns;
            merged.append(&mut self.turns);
            self.turns = merged;
        }
        self.history_start_byte = slice.start_byte;
        self.has_older_history = slice.has_older;
        self.loading_older = false;
        Task::none()
    }

    /// After open / prepend, keep fetching older chunks until the pane has
    /// enough display items (or we hit the auto-chunk cap). Disk-only and
    /// synchronous so it is not stalled behind ACP `session/load`.
    fn maybe_auto_fill_history(&mut self) -> Task<Msg> {
        if self.session_id.is_none() {
            return Task::none();
        }
        while self.has_older_history
            && self.history_auto_chunks < sessions::HISTORY_AUTO_CHUNKS_MAX
            && sessions::display_item_count(&self.turns) < sessions::HISTORY_INITIAL_ITEMS
        {
            self.history_auto_chunks += 1;
            let _ = self.request_older_history();
        }
        Task::none()
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
            AgentEvent::AgentInfo {
                agent_version,
                model_id,
                efforts,
                current_effort,
            } => {
                if let Some(v) = agent_version {
                    self.grok_version = Some(v);
                }
                if model_id.is_some() {
                    self.model_id = model_id;
                }
                if !efforts.is_empty() {
                    self.efforts = efforts;
                }
                if current_effort.is_some() {
                    self.effort_id = current_effort;
                }
            }
            AgentEvent::GrokVersion {
                current,
                latest,
                update_available,
                channel: _,
            } => {
                if let Some(v) = current {
                    self.grok_version = Some(v);
                }
                self.grok_latest = latest;
                self.grok_update_available = update_available;
            }
            AgentEvent::SessionConfig {
                efforts,
                current_effort,
                model_id,
            } => {
                if !efforts.is_empty() {
                    self.efforts = efforts;
                }
                if current_effort.is_some() {
                    self.effort_id = current_effort;
                }
                if model_id.is_some() {
                    self.model_id = model_id;
                }
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
                // Fast sidebar switches paint optimistically; ignore attach
                // completions for a session the user already left.
                if self.session_id.as_deref().is_some_and(|s| s != id.as_str()) {
                    return;
                }
                self.session_id = Some(id.clone());
                self.acp_attached = true;
                if let Some(t) = overlay::title_override(&id) {
                    self.session_title = Some(t);
                } else if title.is_some() {
                    self.session_title = title;
                }
                // Apply preferred permission mode on every session attach.
                // Order: effort first (also uses set_mode on Grok), then
                // permission so YOLO is the last modeId applied.
                if let Some(effort) = self.effort_id.clone() {
                    bridge::agent_send(AgentCmd::SetEffort {
                        effort_id: effort,
                    });
                }
                bridge::agent_send(AgentCmd::SetPermissionMode {
                    mode_id: self.permission_mode.as_mode_id().to_string(),
                });
            }
            AgentEvent::Transcript {
                session_id,
                turns,
                history_start_byte,
                has_older,
                from_watch,
            } => {
                if self.session_id.as_deref().is_some_and(|s| s != session_id.as_str()) {
                    return;
                }
                // Prefer the async disk paint / cache restore when still loading.
                // Soft post-attach re-sync may replace once content is shown.
                if self.content_loading && from_watch {
                    return;
                }
                // Detect real change so watch ticks that re-read the same
                // tail do not thrash scroll / auto-title.
                let changed = self.turns != turns
                    || self.history_start_byte != history_start_byte
                    || self.has_older_history != has_older;
                // Keep a richer auto-filled window: only replace when the new
                // slice is at least as deep (lower start_byte) or first paint.
                if from_watch
                    && !self.turns.is_empty()
                    && history_start_byte > self.history_start_byte
                {
                    // Tail-only re-sync would drop older chunks we already have.
                    return;
                }
                self.turns = turns;
                self.history_start_byte = history_start_byte;
                self.has_older_history = has_older;
                self.loading_older = false;
                if !from_watch {
                    self.content_loading = false;
                    self.history_auto_chunks = 0;
                    self.scroll_rel_y = None;
                    self.streaming = false;
                    self.pending = None;
                    self.stick_to_bottom = true;
                    self.scroll_bottom_pending = true;
                } else if changed && self.stick_to_bottom {
                    self.scroll_bottom_pending = true;
                }
                if changed {
                    self.refresh_title_from_turns();
                    let cwd = self.project_root.to_string_lossy().into_owned();
                    let len = sessions::updates_file_len(&cwd, &session_id);
                    self.cache_current_session(len);
                }
            }
            AgentEvent::HistoryOlder {
                session_id,
                turns,
                history_start_byte,
                has_older,
            } => {
                if self.session_id.as_deref().is_some_and(|s| s != session_id.as_str()) {
                    return;
                }
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
                if !self.acp_attached {
                    return;
                }
                self.turns.push(Turn::User(text));
            }
            AgentEvent::AgentDelta { text } => {
                if !self.acp_attached {
                    return;
                }
                self.streaming = true;
                match self.turns.last_mut() {
                    Some(Turn::Assistant(s)) => s.push_str(&text),
                    _ => self.turns.push(Turn::Assistant(text)),
                }
            }
            AgentEvent::ThoughtDelta { text } => {
                if !self.acp_attached {
                    return;
                }
                self.streaming = true;
                match self.turns.last_mut() {
                    Some(Turn::Thought(s)) => s.push_str(&text),
                    _ => self.turns.push(Turn::Thought(text)),
                }
            }
            AgentEvent::ToolStart { call_id, tool } => {
                if !self.acp_attached {
                    return;
                }
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
                if !self.acp_attached {
                    return;
                }
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
                if !self.acp_attached {
                    return;
                }
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
                if !self.acp_attached {
                    return;
                }
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
                if !self.acp_attached {
                    // Still answer always-approve so the leader does not hang
                    // on a permission for a session we already left.
                    if self.permission_mode.auto_answers_permissions() {
                        if let Some(option_id) = pick_allow_option(&options) {
                            bridge::agent_send(AgentCmd::Permission {
                                request_id,
                                option_id,
                            });
                        }
                    }
                    return;
                }
                // always-approve: answer in-process so the strip never flashes
                // even if the leader still emits request_permission (hooks,
                // shell ask rules, mode not yet applied, effort clobber race).
                if self.permission_mode.auto_answers_permissions() {
                    if let Some(option_id) = pick_allow_option(&options) {
                        bridge::agent_send(AgentCmd::Permission {
                            request_id,
                            option_id,
                        });
                        return;
                    }
                    tracing::warn!(
                        %tool,
                        "always-approve but no allow option; showing strip"
                    );
                }
                self.pending = Some(PendingApproval {
                    request_id,
                    tool,
                    preview,
                    options,
                });
            }
            AgentEvent::TurnEnded { stop_reason } => {
                if !self.acp_attached {
                    return;
                }
                self.streaming = false;
                self.pending = None;
                self.finalize_open_tools();
                self.refresh_title_from_turns();
                if let Some(id) = self.session_id.clone() {
                    let cwd = self.project_root.to_string_lossy().into_owned();
                    let len = sessions::updates_file_len(&cwd, &id);
                    self.cache_current_session(len);
                }
                if stop_reason != "end_turn" && stop_reason != "EndTurn" {
                    tracing::info!(%stop_reason, "turn ended");
                }
            }
            AgentEvent::Error { message } => {
                // Errors can be attach failures for the selected session —
                // always surface them (tagged paths use session_id elsewhere).
                self.streaming = false;
                self.turns.push(Turn::Error(message));
            }
            AgentEvent::SessionsListed { entries } => {
                // Worker refresh: merge so a mid-turn list poll doesn't reshuffle.
                self.sessions = sessions::merge_with(&self.sessions, entries);
            }
            AgentEvent::BulkDeleteProgress {
                done,
                total,
                last_id,
            } => {
                if let Some(p) = &mut self.bulk_delete {
                    p.phase = BulkDeletePhase::Deleting {
                        done,
                        total,
                        last_id,
                    };
                }
            }
            AgentEvent::BulkDeleteFinished {
                deleted,
                failed,
                errors,
            } => {
                // If we deleted the open session (keep_open off), clear transcript.
                if let Some(open) = self.session_id.clone() {
                    let still_there = sessions::find_session_dir(&open).is_some();
                    if !still_there {
                        self.session_id = None;
                        self.session_title = None;
                        self.acp_attached = false;
                        self.turns.clear();
                        self.history_start_byte = 0;
                        self.has_older_history = false;
                    }
                }
                self.sessions = sessions::list_all();
                if let Some(p) = &mut self.bulk_delete {
                    p.phase = BulkDeletePhase::Done {
                        deleted,
                        failed,
                        errors,
                    };
                    // Refresh preview so remaining sessions show correctly.
                    p.preview = sessions::bulk_delete_preview(&p.criteria);
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        view::screen(self)
    }
}

/// Pick the best allow option for auto-approve / default Approve.
/// Prefers `allow_always`, then `allow_once`, then any non-reject option.
fn pick_allow_option(options: &[PermissionChoice]) -> Option<String> {
    let score = |o: &PermissionChoice| -> i32 {
        let k = o.kind.to_lowercase();
        if k.contains("reject") || k.contains("deny") {
            return -1;
        }
        if k.contains("allow_always") || (k.contains("always") && k.contains("allow")) {
            return 3;
        }
        if k.contains("allow_once") || k.contains("allow") {
            return 2;
        }
        1
    };
    options
        .iter()
        .filter(|o| score(o) > 0)
        .max_by_key(|o| score(o))
        .map(|o| o.option_id.clone())
}

fn is_shift_key(key: &keyboard::Key, physical: keyboard::key::Physical) -> bool {
    matches!(key, keyboard::Key::Named(NamedKey::Shift))
        || matches!(
            physical,
            keyboard::key::Physical::Code(keyboard::key::Code::ShiftLeft)
                | keyboard::key::Physical::Code(keyboard::key::Code::ShiftRight)
        )
}
