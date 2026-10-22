//! sola-projects (Projects) — project / tab rail + agent-aware PTYs.
//!
//! Persist + spawn: catalog on disk, siblings under `.worktrees/`.
//! Grok + Codex hooks, OSC 9999, process-tree. Calls on sola-call owner `projects`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{canvas, column, container, mouse_area, stack};
use iced::{Element, Event, Length, Subscription, Task, Theme};
use iced::{event, keyboard, window};

use sola_bus::Message;
use sola_bus::topics::{AppNotification, SplitDir, Topic, TopicKind};
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup,
};
use sola_kit::fonts;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};
use sola_terminal::emulator::{self, Emulator, Listener};
use sola_terminal::input::{self, Mods};
use sola_terminal::pty::PtyBackend;
use sola_terminal::state::{self as term_state, PaneRuntime};
use sola_terminal::term_view::{self, CellMetrics, Palette};
use sola_terminal::{extkeys, links, tmux};

mod calls;
mod cards;
mod catalog;
mod cli;
mod hooks;
mod menu;
mod paths;
mod presence;
mod spawn;
mod startup;
mod status;
mod topbar;
mod windows;

const APP_ID: &str = "sola-projects";
const WINDOW_TITLE: &str = "Projects";

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

const TMUX_SOCKET: &str = "sola-ws";
const TMUX_UNIT: &str = "sola-ws-tmux.service";
const TMUX_PREFIX: &str = "sws-";

const DEFAULT_COLS: u16 = 80;
/// A window's size before it reports one.
const DEFAULT_WINDOW: iced::Size = iced::Size::new(1100.0, 720.0);
const DEFAULT_ROWS: u16 = 24;
const MIN_PANE_PX: f32 = 80.0;

fn main() -> iced::Result {
    startup(APP_ID);

    // Must precede any tmux helper — do not share sola-terminal's server.
    tmux::configure(TMUX_SOCKET, TMUX_UNIT, TMUX_PREFIX);
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::ensure_server_running();
    tmux::reload_config();

    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::Windows,
            TopicKind::WindowFloating,
            TopicKind::TabWindow,
            TopicKind::NotificationActivate,
        ])
        .calls(calls::OWNER, calls::methods())
        .install();

    // A window per project: each is a tab in the shell's tab bar.
    iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .run()
}

struct App {
    projects: Vec<catalog::Project>,
    tabs: Vec<catalog::Tab>,
    selected: String,
    focused: String,
    runtimes: HashMap<String, PaneRuntime>,
    dragging_split: Option<String>,
    /// Last applied grid per pane. Skip TIOCSWINSZ when a divider drag
    /// has not changed cols/rows (same as sola-terminal).
    pane_grids: HashMap<String, (u16, u16)>,
    theme: Theme,
    palette: Palette,
    metrics: CellMetrics,
    cursor_on: bool,
    keyboard_mods: keyboard::Modifiers,
    keys_held_mods: keyboard::Modifiers,
    float: sola_kit::FloatState,
    /// A window per project, each a tab in the shell's tab bar.
    windows: windows::ProjectWindows,
    /// Each project's last selected tab; the focused window's is `selected`.
    last_tab: HashMap<String, String>,
    /// `(tab id, draft name)` while a top-bar tab is being renamed.
    renaming: Option<(String, String)>,
    /// The top-bar tab under the pointer.
    hover_tab: Option<String>,
    /// The top-bar tab being dragged (from its press until the release).
    chip_drag: Option<String>,
    pane_status: HashMap<String, status::PaneStatus>,
    hook_sock: String,
    /// Previous pane id if we renamed a *path-matched* orphan onto `ws-main`.
    adopted_from: Option<String>,
    spawn: cards::SpawnDraft,
    add: cards::AddDraft,
    startup: cards::StartupDraft,
    /// One of the app's windows has keyboard focus (desk cards wait).
    window_focused: bool,
    pending_waits: Vec<PendingWait>,
}

struct PendingWait {
    pane: String,
    want: status::AgentStatus,
    fresh: bool,
    armed: bool,
    reply: sola_call::ReplyTx,
    deadline: Instant,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    PtyOutput(String),
    PtyExit(String),
    Title(String, String),
    Input(iced::Event),
    SelectionChanged,
    Scrolled(String),
    OpenUrl(String),
    WheelToPty(String, Vec<u8>),
    Pasted(Option<String>),
    BlinkTick,
    CursorMoved(f32, f32),
    CursorReleased,
    Window(window::Id, window::Event),
    TitleDrag(window::Id),
    TitleResize(window::Id, window::Direction),
    TitleClose(window::Id),
    /// The pointer came onto / left a top-bar tab.
    HoverIn(String),
    HoverOut(String),
    /// A press on a top-bar tab: select it, and start a drag.
    ChipPress(String),
    /// The button came up during a tab drag: drop it on the tab under it.
    ChipDrop,
    /// The wheel over a project's top bar.
    BarScroll(String, iced::mouse::ScrollDelta),
    BeginRename(String),
    RenameInput(String),
    RenameCommit,
    OpenSpawn(String),
    OpenAdd,
    StartupAction(iced::widget::text_editor::Action),
    SaveStartup,
    DismissDialog,
    SpawnName(String),
    /// New-tab card: open in the project root (no worktree).
    SpawnInRoot(bool),
    Spawn,
    AddPath(String),
    AddProject,
    CloseTab(String),
    /// Close the tab and `git worktree remove` the checkout (`--worktree`).
    RmCheckout {
        id: String,
        force: bool,
    },
    DropProject(String),
    /// `git worktree remove` failed after the tab was already closed.
    GitRmFailed {
        id: String,
        error: String,
    },
    RestartShell(String),
    PaneFocused(String),
    SplitDividerPress(String),
    Ignore,
    Hook(hooks::Incoming),
    Osc(String, sola_terminal::osc9999::OscStatus),
    PresenceTick,
    Call(sola_call::Incoming),
}

/// PTY taken off the iced state so `tmux kill-session` / `git worktree remove`
/// can run off the UI thread. `close()` matches [`App::teardown_pane`].
struct DetachedPane {
    backend: Option<PtyBackend>,
    session: String,
}

impl DetachedPane {
    fn close(self) {
        if let Some(backend) = self.backend {
            backend.close();
        } else {
            tmux::kill_session(&self.session);
        }
    }
}

/// Kill tmux (and optionally `git worktree remove`) after the rail has
/// already dropped the tab. Both wait on subprocesses; doing them in
/// `update` freezes every other pane.
fn close_io_task(
    panes: Vec<DetachedPane>,
    checkout: Option<(PathBuf, PathBuf, bool)>,
    tab_id: String,
) -> Task<Msg> {
    Task::perform(
        async move {
            match tokio::task::spawn_blocking(move || {
                for pane in panes {
                    pane.close();
                }
                match checkout {
                    Some((root, path, force)) => spawn::remove_worktree(&root, &path, force),
                    None => Ok(()),
                }
            })
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(%e, "tab teardown worker panicked");
                    Ok(())
                }
            }
        },
        move |result| match result {
            Ok(()) => Msg::Ignore,
            Err(error) => Msg::GitRmFailed {
                id: tab_id,
                error,
            },
        },
    )
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let hook_paths = hooks::start();
        let _ = sola_terminal::osc9999::sender();
        let mut catalog = catalog::load();
        if catalog.projects.is_empty() {
            if let Some((project, ws)) = catalog::seed_from_cwd() {
                catalog.selected = Some(ws.id.clone());
                catalog.projects.push(project);
                catalog.tabs.push(ws);
                catalog::save(&catalog);
            }
        }
        let selected = catalog
            .selected
            .clone()
            .filter(|id| catalog.tabs.iter().any(|w| w.id == *id))
            .or_else(|| catalog.tabs.first().map(|w| w.id.clone()))
            .unwrap_or_default();
        let adopted_from = catalog
            .tabs
            .iter()
            .find(|w| w.id == catalog::LIVE_ID)
            .and_then(|w| {
                let claimed: HashSet<String> =
                    catalog.tabs.iter().map(|x| x.id.clone()).collect();
                catalog::adopt_orphan_session(&w.path, &claimed)
            });
        let mut pane_status = HashMap::new();
        for ws in &catalog.tabs {
            for pane_id in ws.layout().leaves() {
                if let Some(st) = status::hydrate(&pane_id) {
                    pane_status.insert(pane_id, st);
                }
            }
        }
        let focused = catalog
            .tabs
            .iter()
            .find(|w| w.id == selected)
            .map(|w| w.active_pane_id())
            .unwrap_or_else(|| selected.clone());
        let last_tab = catalog
            .projects
            .iter()
            .filter_map(|p| p.selected.clone().map(|t| (p.id.clone(), t)))
            .collect();
        let mut app = Self {
            projects: catalog.projects,
            tabs: catalog.tabs,
            selected: selected.clone(),
            focused,
            runtimes: HashMap::new(),
            dragging_split: None,
            pane_grids: HashMap::new(),
            theme: default_theme(),
            palette: Palette::from_kit_theme(&Atoms::default()),
            metrics: CellMetrics::for_font(15.0, fonts::mono_metrics()),
            cursor_on: true,
            keyboard_mods: keyboard::Modifiers::empty(),
            keys_held_mods: keyboard::Modifiers::empty(),
            float: sola_kit::FloatState::new(APP_ID),
            windows: windows::ProjectWindows::new(APP_ID),
            last_tab,
            renaming: None,
            hover_tab: None,
            chip_drag: None,
            pane_status,
            hook_sock: hook_paths.socket_path.to_string_lossy().into_owned(),
            adopted_from,
            spawn: cards::SpawnDraft::default(),
            add: cards::AddDraft::default(),
            startup: cards::StartupDraft::default(),
            window_focused: true,
            pending_waits: Vec::new(),
        };
        app.sync_all_rows();
        let reap = app.reap_missing_worktrees();
        let windows = app.open_all_windows();
        (app, Task::batch([reap, windows]))
    }

    /// A project window's title is the project's name.
    fn title(&self, window: window::Id) -> String {
        self.windows
            .project_of(window)
            .and_then(|id| catalog::find_project(&self.projects, id))
            .map(|p| p.name.clone())
            .unwrap_or_else(|| WINDOW_TITLE.into())
    }

    fn theme(&self, window: window::Id) -> Theme {
        sola_kit::theme_for(self.is_floating(window), &self.theme)
    }

    fn is_floating(&self, window: window::Id) -> bool {
        self.windows
            .river_id(window)
            .is_some_and(|id| self.float.is_window_floating(id))
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(Msg::PtyOutput),
            emulator::exit_subscription().map(Msg::PtyExit),
            emulator::title_subscription().map(|(id, t)| Msg::Title(id, t)),
            hooks::subscription().map(Msg::Hook),
            sola_kit::call_subscription().map(Msg::Call),
            sola_terminal::osc9999::subscription().map(|(id, payload)| Msg::Osc(id, payload)),
            iced::time::every(Duration::from_secs(1)).map(|_| Msg::PresenceTick),
            window::events().map(|(id, e)| Msg::Window(id, e)),
            event::listen_with(|ev, status, _| match &ev {
                Event::Keyboard(_) => Some(Msg::Input(ev.clone())),
                // Mouse is handled by TermView / sidebar / the split-drag
                // sub below. Mapping ignored motion to Input presented the
                // whole window on every pixel (`on_input` ignores mouse).
                Event::Mouse(_) => None,
                _ if matches!(status, iced::event::Status::Ignored) => Some(Msg::Input(ev.clone())),
                _ => None,
            }),
            iced::time::every(Duration::from_millis(530)).map(|_| Msg::BlinkTick),
            if self.chip_drag.is_some() {
                event::listen_with(|ev, _, _| match ev {
                    Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                        Some(Msg::ChipDrop)
                    }
                    _ => None,
                })
            } else {
                Subscription::none()
            },
            if self.dragging_split.is_some() {
                event::listen_with(|ev, _, _| match ev {
                    Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Msg::CursorMoved(position.x, position.y))
                    }
                    Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                        Some(Msg::CursorReleased)
                    }
                    _ => None,
                })
            } else {
                Subscription::none()
            },
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        let task = self.update_msg(msg);
        self.note_selected();
        task
    }

    /// Remember the selected tab as its project's, so each project window
    /// keeps showing its own tab while another window has focus.
    fn note_selected(&mut self) {
        if let Some(tab) = self.tabs.iter().find(|w| w.id == self.selected) {
            self.last_tab
                .insert(tab.project_id.clone(), self.selected.clone());
        }
    }

    fn update_msg(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::Window(id, event) => self.on_window(id, event),
            Msg::TitleDrag(id) => sola_kit::drag(Some(id)),
            Msg::TitleResize(id, dir) => sola_kit::drag_resize(Some(id), dir),
            Msg::TitleClose(id) => self.close_window(id),
            Msg::Ignore => Task::none(),
            Msg::Call(incoming) => self.on_call(incoming),
            Msg::Hook(incoming) => {
                let Some(id) = self.resolve_hook_pane(&incoming) else {
                    tracing::debug!(
                        pane = %incoming.pane_id,
                        cwd = ?incoming.cwd,
                        session = ?incoming.mapped.session_id,
                        "hook for unknown pane"
                    );
                    return Task::none();
                };
                let prev = self
                    .pane_status
                    .get(&id)
                    .map(|s| s.status)
                    .unwrap_or_default();
                let cwd = self.tab_for_pane(&id).map(|w| w.path.clone());
                let st = self.pane_status.entry(id.clone()).or_default();
                st.apply_hook(&incoming);
                if incoming.mapped.compacted || incoming.mapped.session_id.is_some() {
                    if let Some(cwd) = cwd.as_deref() {
                        st.refresh_compaction(cwd);
                    }
                }
                if incoming.mapped.compacted && st.compaction_count == 0 {
                    st.compaction_count = 1;
                }
                let now = st.status;
                let unconfirmed = st.restored_unconfirmed;
                status::persist_all(&self.pane_status);
                self.sync_row(&id);
                self.maybe_toast_status(&id, prev, now, unconfirmed);
                self.flush_waits();
                Task::none()
            }
            Msg::Osc(id, payload) => {
                if let Some(id) = self.resolve_pane(&id) {
                    let prev = self
                        .pane_status
                        .get(&id)
                        .map(|s| s.status)
                        .unwrap_or_default();
                    let st = self.pane_status.entry(id.clone()).or_default();
                    st.apply_osc(&payload);
                    let now = st.status;
                    let unconfirmed = st.restored_unconfirmed;
                    status::persist_all(&self.pane_status);
                    self.sync_row(&id);
                    self.maybe_toast_status(&id, prev, now, unconfirmed);
                    self.flush_waits();
                }
                Task::none()
            }
            Msg::PresenceTick => {
                let reap = self.reap_missing_worktrees();
                let ids: Vec<String> = self
                    .tabs
                    .iter()
                    .flat_map(|w| w.layout().leaves())
                    .collect();
                let mut claimed: HashSet<String> = HashSet::new();
                for id in &ids {
                    let Some(st) = self.pane_status.get(id) else {
                        continue;
                    };
                    if !st.is_codex() {
                        continue;
                    }
                    if let Some(sid) = st.owner_session.clone() {
                        if status::codex_rollout_exists(&sid) {
                            claimed.insert(sid);
                        }
                    }
                }
                let mut dirty = false;
                for id in ids {
                    let tmux_session = tmux::session_name(&id);
                    let who = presence::scan_session(&tmux_session);
                    let cwd = self.tab_for_pane(&id).map(|w| w.path.clone());
                    let st = self.pane_status.entry(id).or_default();
                    let before = (st.status, st.agent.clone());
                    st.apply_presence(who);
                    if st.is_codex() {
                        if let Some(cwd) = cwd.as_deref() {
                            if st.observe_codex_turn(cwd, &claimed) {
                                dirty = true;
                            }
                            if let Some(sid) = st.owner_session.clone() {
                                claimed.insert(sid);
                            }
                        }
                    }
                    if let Some(cwd) = cwd.as_deref() {
                        st.refresh_compaction(cwd);
                    }
                    if (st.status, st.agent.clone()) != before {
                        dirty = true;
                    }
                }
                if dirty {
                    status::persist_all(&self.pane_status);
                }
                self.sync_all_rows();
                self.flush_waits();
                reap
            }
            Msg::HoverIn(id) => {
                self.hover_tab = Some(id);
                Task::none()
            }
            Msg::HoverOut(id) => {
                // Moving leftwards, the next tab's enter comes before this
                // exit (left to right order): only clear our own hover.
                if self.hover_tab.as_deref() == Some(id.as_str()) {
                    self.hover_tab = None;
                }
                Task::none()
            }
            Msg::ChipPress(id) => {
                self.chip_drag = Some(id.clone());
                self.select_tab(&id)
            }
            Msg::ChipDrop => {
                if let (Some(from), Some(onto)) = (self.chip_drag.take(), self.hover_tab.clone()) {
                    self.drop_tab_on(&from, &onto);
                }
                Task::none()
            }
            Msg::BarScroll(project_id, delta) => {
                let (dx, dy) = match delta {
                    iced::mouse::ScrollDelta::Lines { x, y } => (x * 40.0, y * 40.0),
                    iced::mouse::ScrollDelta::Pixels { x, y } => (x, y),
                };
                // Wheel down (or right) moves along to the later tabs.
                iced::widget::operation::scroll_by(
                    topbar::strip_id(&project_id),
                    iced::widget::scrollable::AbsoluteOffset { x: -(dx + dy), y: 0.0 },
                )
            }
            Msg::PaneFocused(pane_id) => {
                // Hover focuses for typing. It must not spawn a shell —
                // only the Start new shell button (or a sidebar click)
                // attaches a missing PTY.
                // Only in the window keys go to: hovering a pane in another
                // project's window must not take the keyboard.
                if let Some(ws) = self.tabs.iter().find(|w| w.owns_pane(&pane_id)) {
                    let in_focused = self.windows.focused.is_some()
                        && self.windows.window_of(&ws.project_id) == self.windows.focused;
                    if in_focused {
                        let ws_id = ws.id.clone();
                        self.set_focus(&ws_id, &pane_id);
                    }
                }
                Task::none()
            }
            Msg::SplitDividerPress(id) => {
                self.dragging_split = Some(id);
                Task::none()
            }
            Msg::OpenSpawn(project_id) => self.open_spawn(&project_id),
            Msg::OpenAdd => self.open_add(),
            Msg::StartupAction(action) => {
                self.startup.content.perform(action);
                Task::none()
            }
            Msg::SaveStartup => self.save_startup(),
            Msg::DismissDialog => {
                self.spawn = cards::SpawnDraft::default();
                self.add = cards::AddDraft::default();
                self.startup = cards::StartupDraft::default();
                self.renaming = None;
                Task::none()
            }
            Msg::SpawnName(s) => {
                self.spawn.name = s;
                self.spawn.error = None;
                Task::none()
            }
            Msg::SpawnInRoot(on) => {
                self.spawn.in_root = on;
                self.spawn.error = None;
                Task::none()
            }
            Msg::Spawn => self.spawn_sibling(),
            Msg::AddPath(s) => {
                self.add.path = s;
                self.add.error = None;
                Task::none()
            }
            Msg::AddProject => self.add_project(),
            Msg::CloseTab(id) => self.close_tab(&id, false, false),
            Msg::RmCheckout { id, force } => self.close_tab(&id, true, force),
            Msg::DropProject(id) => self.drop_project(&id),
            Msg::GitRmFailed { id, error } => {
                tracing::warn!(tab = %id, %error, "git worktree remove failed");
                self.emit_notice("Worktree leftover".into(), error, format!("git-rm-{id}"));
                Task::none()
            }
            Msg::RestartShell(id) => self.attach_pane(&id, &[]),
            Msg::BeginRename(id) => self.begin_rename(id),
            Msg::RenameInput(s) => {
                if let Some((_, draft)) = &mut self.renaming {
                    *draft = s;
                }
                Task::none()
            }
            Msg::RenameCommit => self.commit_rename(),
            Msg::PtyOutput(id) => {
                if let Some(rt) = self.runtimes.get(&id) {
                    rt.cache.clear();
                }
                Task::none()
            }
            Msg::PtyExit(id) => {
                tracing::info!(pane = %id, "pane PTY exited");
                // Drop the client without `close()` (plain Drop keeps tmux).
                self.runtimes.remove(&id);
                // Tab already dropped (CLI / hover). The session is being
                // killed off-thread — do not list-sessions or reattach.
                if self.tab_for_pane(&id).is_none() {
                    self.pane_status.remove(&id);
                    status::persist_all(&self.pane_status);
                    return Task::none();
                }
                let want = tmux::session_name(&id);
                let live = tmux::list_sessions().unwrap_or_default();
                if live.iter().any(|s| s == &want) {
                    tracing::info!(
                        pane = %id,
                        session = %want,
                        "tmux client exited; session lives, reattaching"
                    );
                    return self.attach_pane(&id, &[]);
                }
                if let Some(st) = self.pane_status.get_mut(&id) {
                    st.status = status::AgentStatus::Idle;
                    st.agent = None;
                    st.tool = None;
                    st.owner_session = None;
                }
                // A split leaf that dies retracts. Start new shell only
                // on the last remaining pane (`close_pane` handles that).
                if self.tab_for_pane(&id).is_some() {
                    return self.close_pane(&id);
                }
                self.sync_row(&id);
                status::persist_all(&self.pane_status);
                Task::none()
            }
            Msg::Title(id, title) => {
                tracing::debug!(pane = %id, %title, "pane title");
                Task::none()
            }
            Msg::BlinkTick => {
                self.cursor_on = !self.cursor_on;
                Task::none()
            }
            Msg::Input(event) => self.on_input(event),
            Msg::SelectionChanged => {
                if let Some(rt) = self.runtimes.get(&self.focused) {
                    rt.cache.clear();
                }
                Task::none()
            }
            Msg::Scrolled(id) => {
                if let Some(rt) = self.runtimes.get(&id) {
                    rt.cache.clear();
                }
                Task::none()
            }
            Msg::OpenUrl(uri) => {
                links::open_url(&uri);
                Task::none()
            }
            Msg::WheelToPty(id, bytes) => {
                if let Some(rt) = self.runtimes.get(&id) {
                    rt.backend.write(&bytes);
                }
                Task::none()
            }
            Msg::Pasted(text) => self.on_pasted(text),
            Msg::CursorMoved(x, y) => {
                if let Some(split_id) = self.dragging_split.clone() {
                    self.drag_split(&split_id, x, y);
                }
                Task::none()
            }
            Msg::CursorReleased => {
                if self.dragging_split.take().is_some() {
                    self.persist_catalog();
                }
                Task::none()
            }
        }
    }

    fn view(&self, window: window::Id) -> Element<'_, Msg> {
        let project_id = self.windows.project_of(window).unwrap_or(windows::EMPTY);
        let body: Element<'_, Msg> = if project_id.is_empty() {
            cards::no_project()
        } else {
            let selected = self.selected_of(project_id).unwrap_or("");
            let pane: Element<'_, Msg> = match self.tabs.iter().find(|w| w.id == selected) {
                Some(ws) => self.render_node(&ws.layout().to_node()),
                None => cards::empty_pane(),
            };
            let bar = topbar::view(topbar::Bar {
                project_id,
                tabs: &self.tabs,
                selected,
                hovered: self.hover_tab.as_deref(),
                renaming: self.renaming.as_ref(),
                dragging: self.chip_drag.as_deref(),
                pane_status: &self.pane_status,
            });
            // Clipped: a terminal canvas paints outside its bounds (rows
            // above the top while a grid catches up with its size), and
            // the bar sits right above it.
            let pane = container(pane)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true);
            column![bar, sola_kit::components::horizontal_divider(), pane].into()
        };

        let body: Element<'_, Msg> = match (self.card_window() == Some(window))
            .then(|| cards::overlay(&self.spawn, &self.add, &self.startup, &self.projects))
            .flatten()
        {
            Some(veil) => stack![body, veil].into(),
            None => body,
        };

        let bg = self.palette.bg;
        let framed = container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            });

        sola_kit::wrap_if_floating(
            self.is_floating(window),
            self.title(window),
            Msg::TitleDrag(window),
            Msg::TitleClose(window),
            move |dir| Msg::TitleResize(window, dir),
            framed.into(),
        )
    }

    /// The window an open card shows in: the new-tab and startup cards in
    /// their project's window, the add-project card in the focused one.
    fn card_window(&self) -> Option<window::Id> {
        let project = self
            .spawn
            .project_id
            .as_deref()
            .or(self.startup.project_id.as_deref());
        project
            .and_then(|p| self.windows.window_of(p))
            .or(self.windows.focused)
    }

    /// Project `project_id`'s selected tab: the focused window's selection
    /// if it is this project's, else the tab it last showed, else its root.
    fn selected_of(&self, project_id: &str) -> Option<&str> {
        let mine = |id: &str| {
            self.tabs
                .iter()
                .any(|w| w.id == id && w.project_id == project_id)
        };
        if mine(&self.selected) {
            return Some(self.selected.as_str());
        }
        if let Some(id) = self.last_tab.get(project_id).filter(|id| mine(id)) {
            return Some(id.as_str());
        }
        catalog::ordered_for_project(project_id, &self.tabs)
            .first()
            .map(|w| w.id.as_str())
    }

    /// Open a window for every project (the one with the saved selection
    /// in front), or the no-project window when there are none.
    fn open_all_windows(&mut self) -> Task<Msg> {
        let front = self
            .tabs
            .iter()
            .find(|w| w.id == self.selected)
            .map(|w| w.project_id.clone())
            .or_else(|| self.projects.first().map(|p| p.id.clone()));
        let ids: Vec<String> = self.projects.iter().map(|p| p.id.clone()).collect();
        if ids.is_empty() {
            return self.open_window(windows::EMPTY, false);
        }
        let mut tasks = Vec::new();
        for id in ids {
            let background = front.as_deref() != Some(id.as_str());
            tasks.push(self.open_window(&id, background));
        }
        Task::batch(tasks)
    }

    /// Open project `project_id`'s window and attach its selected tab. The
    /// no-project window goes once a project has one.
    fn open_window(&mut self, project_id: &str, background: bool) -> Task<Msg> {
        if self.windows.has(project_id) {
            return Task::none();
        }
        let mut tasks = Vec::new();
        if !project_id.is_empty() {
            if let Some(empty) = self.windows.window_of(windows::EMPTY) {
                self.windows.forget(empty);
                tasks.push(window::close(empty));
            }
        }
        tasks.push(self.windows.open(APP_ID, project_id, background).map(|_| Msg::Ignore));
        if let Some(tab) = self.selected_of(project_id).map(str::to_string) {
            tasks.push(self.attach_tab(&tab));
        }
        self.publish_menu();
        Task::batch(tasks)
    }

    /// Close a project window. The project stays, and its tmux sessions
    /// keep running; Project → Open brings it back. The last window
    /// closing quits.
    fn close_window(&mut self, window: window::Id) -> Task<Msg> {
        let project = self.windows.forget(window);
        tracing::info!(project = ?project, "project window closed");
        self.publish_menu();
        if self.windows.is_empty() {
            return Task::batch([window::close(window), iced::exit()]);
        }
        window::close(window)
    }

    /// Window lifecycle: sizes, focus, and the close button / request.
    fn on_window(&mut self, id: window::Id, event: window::Event) -> Task<Msg> {
        match event {
            window::Event::Opened { size, .. } | window::Event::Resized(size) => {
                self.windows.set_size(id, size);
                self.resize_all_panes();
                Task::none()
            }
            window::Event::Focused => {
                self.windows.focused = Some(id);
                self.window_focused = true;
                // Keys and menu actions go to this window's tab now.
                if let Some(tab) = self
                    .windows
                    .project_of(id)
                    .and_then(|p| self.selected_of(p))
                    .map(str::to_string)
                {
                    if tab != self.selected {
                        self.selected = tab.clone();
                        if let Some(ws) = self.tabs.iter().find(|w| w.id == tab) {
                            self.focused = ws.active_pane_id();
                        }
                        self.persist_catalog();
                    }
                }
                Task::none()
            }
            window::Event::Unfocused => {
                self.window_focused = false;
                // Super release is often eaten by River while we are
                // unfocused (switcher, another window). Drop the latch so
                // the next focus does not treat every key as ⌘.
                self.keyboard_mods = keyboard::Modifiers::empty();
                self.keys_held_mods = keyboard::Modifiers::empty();
                Task::none()
            }
            window::Event::CloseRequested => self.close_window(id),
            window::Event::Closed => {
                // Closed under us (not through close_window).
                if self.windows.forget(id).is_some() {
                    self.publish_menu();
                }
                if self.windows.is_empty() {
                    return iced::exit();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The app menu, with Project → Open for each project without a window.
    fn publish_menu(&self) {
        let closed: Vec<(String, String)> = self
            .projects
            .iter()
            .filter(|p| !self.windows.has(&p.id))
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        if let Ok(mut client) = bus().lock() {
            if let Err(e) = client.emit(Topic::SetAppMenu(menu::app_menu(&closed))) {
                tracing::warn!("app-menu publish failed: {e:?}");
            }
        }
    }

    fn on_bus(&mut self, m: &Message) -> Task<Msg> {
        self.float.update(m);
        self.windows.update(m);
        if let Some(Topic::NotificationActivate(a)) = Topic::parse(m) {
            if a.app_id == APP_ID {
                if let Some(tab) = cli::notice_pane(&a.id)
                    .and_then(|pane| self.tab_for_pane(pane))
                    .map(|w| w.id.clone())
                {
                    return self.show_tab(&tab);
                }
            }
            return Task::none();
        }
        if apply_theme_update(m, &mut self.theme) {
            if let Some(Topic::Theme(bus)) = Topic::parse(m) {
                self.palette = Palette::from_kit_theme(&atoms_from_bus_theme(&bus));
                self.metrics = CellMetrics::for_font(self.metrics.font_size, fonts::mono_metrics());
                self.resize_all_panes();
            }
            return Task::none();
        }
        if is_self_quit(m, APP_ID) {
            return iced::exit();
        }
        if let Some(Topic::MenuAction(p)) = Topic::parse(m) {
            if p.app_id == APP_ID {
                return match p.action_id.as_str() {
                    "spawn-sibling" => {
                        let pid = self
                            .tabs
                            .iter()
                            .find(|w| w.id == self.selected)
                            .map(|w| w.project_id.clone())
                            .or_else(|| self.projects.first().map(|p| p.id.clone()));
                        match pid {
                            Some(id) => self.open_spawn(&id),
                            None => self.open_add(),
                        }
                    }
                    "add-project" => self.open_add(),
                    "close-window" => match self.windows.focused {
                        Some(window) => self.close_window(window),
                        None => Task::none(),
                    },
                    open if open.starts_with(menu::OPEN_PREFIX) => {
                        let id = open[menu::OPEN_PREFIX.len()..].to_string();
                        self.open_window(&id, false)
                    }
                    "startup-script" => self.open_startup(),
                    "split-down" => self.split_focused(SplitDir::Horizontal),
                    "split-right" => self.split_focused(SplitDir::Vertical),
                    "close-pane" => {
                        let id = self.focused.clone();
                        if id.is_empty() {
                            Task::none()
                        } else {
                            self.close_pane(&id)
                        }
                    }
                    "drop-project" => {
                        let pid = self
                            .tabs
                            .iter()
                            .find(|w| w.id == self.selected)
                            .map(|w| w.project_id.clone());
                        match pid {
                            Some(id) => self.drop_project(&id),
                            None => Task::none(),
                        }
                    }
                    "copy" => self.copy_selection(),
                    "paste" => self.paste_clipboard(),
                    _ => Task::none(),
                };
            }
        }
        Task::none()
    }

    /// Grid (cols, rows) a pane should have in its project's window.
    fn pane_grid(&self, pane_id: &str) -> (u16, u16) {
        let fallback = (DEFAULT_COLS, DEFAULT_ROWS);
        let Some(tab) = self.tab_for_pane(pane_id) else {
            return fallback;
        };
        let Some(window) = self.windows.window_of(&tab.project_id) else {
            return fallback;
        };
        let content = self.content_rect(window);
        term_state::pane_rects(&tab.layout().to_node(), content)
            .into_iter()
            .find(|(id, _)| id == pane_id)
            .map(|(_, rect)| {
                let (c, r) =
                    term_view::cols_rows_for(iced::Size::new(rect.w, rect.h), self.metrics);
                (c.max(2), r.max(1))
            })
            .unwrap_or(fallback)
    }

    fn render_node(&self, node: &term_state::PaneNode) -> Element<'_, Msg> {
        match node {
            term_state::PaneNode::Leaf(pane_id) => self.render_leaf(pane_id),
            term_state::PaneNode::Split {
                id,
                dir,
                ratio,
                a,
                b,
            } => {
                let line = self.theme.extended_palette().background.stronger.color;
                let colors = sola_kit::components::DividerColors::uniform(self.palette.bg, line);
                sola_kit::components::split_with(
                    *dir,
                    self.render_node(a),
                    *ratio,
                    Msg::SplitDividerPress(id.clone()),
                    self.render_node(b),
                    colors,
                )
            }
        }
    }

    fn render_leaf(&self, pane_id: &str) -> Element<'_, Msg> {
        let inner: Element<'_, Msg> = match self.runtimes.get(pane_id) {
            Some(rt) => {
                let view = term_view::TermView {
                    term: rt.emulator.term(),
                    cursor_snap: rt.emulator.cursor_snap(),
                    selection_track: rt.emulator.selection_track(),
                    cache: &rt.cache,
                    palette: &self.palette,
                    metrics: self.metrics,
                    cursor_on: self.cursor_on,
                    active: pane_id == self.focused,
                    on_select: Msg::SelectionChanged,
                    on_scroll: Msg::Scrolled(pane_id.to_string()),
                    on_open_url: Box::new(Msg::OpenUrl),
                    on_wheel_pty: Box::new({
                        let pid = pane_id.to_string();
                        move |bytes| Msg::WheelToPty(pid.clone(), bytes)
                    }),
                };
                canvas(view).width(Length::Fill).height(Length::Fill).into()
            }
            None if self.is_sole_leaf(pane_id) => cards::exited_pane(pane_id),
            None => cards::empty_pane(),
        };
        mouse_area(inner)
            .on_enter(Msg::PaneFocused(pane_id.to_string()))
            .into()
    }

    fn attach_tab(&mut self, tab_id: &str) -> Task<Msg> {
        let leaves = self
            .tabs
            .iter()
            .find(|w| w.id == tab_id)
            .map(|w| w.layout().leaves())
            .unwrap_or_default();
        let mut tasks = Vec::new();
        for id in leaves {
            tasks.push(self.attach_pane(&id, &[]));
        }
        Task::batch(tasks)
    }

    fn is_sole_leaf(&self, pane_id: &str) -> bool {
        self.tab_for_pane(pane_id)
            .map(|w| {
                let leaves = w.layout().leaves();
                leaves.len() <= 1 && leaves.first().map(String::as_str) == Some(pane_id)
            })
            .unwrap_or(true)
    }

    /// Record focus. Does not attach a PTY (hover must not spawn).
    fn set_focus(&mut self, tab_id: &str, pane_id: &str) -> bool {
        let Some(ws) = self.tabs.iter().find(|w| w.id == tab_id) else {
            return false;
        };
        if !ws.owns_pane(pane_id) {
            return false;
        }
        let same = self.selected == tab_id && self.focused == pane_id;
        self.selected = tab_id.to_string();
        self.focused = pane_id.to_string();
        if let Some(ws) = self.tabs.iter_mut().find(|w| w.id == tab_id) {
            let node = ws.layout().to_node();
            ws.set_tree(node, pane_id.to_string());
        }
        if !same {
            self.persist_catalog();
        }
        true
    }

    fn attach_pane(&mut self, id: &str, exec: &[&str]) -> Task<Msg> {
        if self.runtimes.contains_key(id) {
            self.resize_all_panes();
            return Task::none();
        }
        let Some(ws) = self
            .tabs
            .iter()
            .find(|w| w.owns_pane(id) || w.id == id)
        else {
            return Task::none();
        };
        let (cols, rows) = self.pane_grid(id);
        let cols = if cols == 0 { DEFAULT_COLS } else { cols };
        let rows = if rows == 0 { DEFAULT_ROWS } else { rows };
        let tmux_session = catalog::bind_session(id, &ws.path);
        if tmux_session.is_empty() {
            tracing::error!(pane = %id, "refusing attach; leftover tmux is another checkout");
            return Task::none();
        }
        let cwd = ws.path.to_string_lossy().into_owned();
        // New tmux (reboot / first attach): if this pane had Grok running,
        // `grok -r <session>` restores the conversation. `-A` reattach
        // ignores exec, so a live tmux is left as-is.
        let resume = if exec.is_empty() && !tmux::has_session(&tmux_session) {
            self.resume_argv(id)
        } else {
            None
        };
        if let Some(args) = resume.as_ref() {
            if let (Some(bin), Some(sid)) = (args.first(), args.last()) {
                tracing::info!(pane = %id, agent = %bin, session = %sid, "resuming agent session");
            }
        }
        let exec_refs: Vec<&str> = match resume.as_ref() {
            Some(args) => args.iter().map(String::as_str).collect(),
            None => exec.to_vec(),
        };

        let listener = Listener::new(
            id.to_string(),
            sola_terminal::pty::pty_write_sender(),
            emulator::notify_sender(),
            emulator::title_sender(),
        );
        let em = Emulator::new(cols, rows, listener);
        let term = em.term();
        let cursor = em.cursor_snap();

        let hook_sock = self.hook_sock.clone();
        let env = [
            ("SOLA_PANE_ID", id),
            ("SOLA_WS_HOOKS_SOCK", hook_sock.as_str()),
            (catalog::SOLA_WS_PATH, cwd.as_str()),
        ];
        let backend = match PtyBackend::spawn_or_attach_with_env(
            id,
            &tmux_session,
            cols,
            rows,
            Some(&cwd),
            term,
            cursor,
            emulator::notify_sender(),
            emulator::exit_sender(),
            &env,
            &exec_refs,
        ) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("spawn_or_attach failed: {e}");
                return Task::none();
            }
        };
        backend.resize(cols, rows);
        backend.sigwinch();
        self.runtimes.insert(
            id.to_string(),
            PaneRuntime {
                emulator: em,
                backend,
                cache: canvas::Cache::default(),
            },
        );
        self.resize_all_panes();
        Task::none()
    }

    /// `grok -r` or `claude --resume` when last-status still names that
    /// agent and the session is on disk. A live tmux is left as-is.
    fn resume_argv(&self, pane_id: &str) -> Option<Vec<String>> {
        let st = self.pane_status.get(pane_id)?;
        let sid = st.resume_session_id()?;
        if st.is_grok() {
            if !status::grok_session_exists(sid) {
                tracing::warn!(
                    pane = %pane_id,
                    session = %sid,
                    "stored grok session not on disk; starting a shell"
                );
                return None;
            }
            return Some(cli::grok_resume_argv(sid));
        }
        if st.is_claude() {
            if !status::claude_session_exists(sid) {
                tracing::warn!(
                    pane = %pane_id,
                    session = %sid,
                    "stored claude session not on disk; starting a shell"
                );
                return None;
            }
            return Some(cli::claude_resume_argv(sid));
        }
        None
    }

    fn resolve_hook_pane(&self, incoming: &hooks::Incoming) -> Option<String> {
        let header = self.resolve_pane(&incoming.pane_id);
        let header_path = header
            .as_deref()
            .and_then(|id| self.tab_for_pane(id))
            .map(|ws| ws.path.clone());
        let located: Vec<(String, std::path::PathBuf)> = self
            .tabs
            .iter()
            .flat_map(|ws| {
                let path = ws.path.clone();
                ws.layout()
                    .leaves()
                    .into_iter()
                    .map(move |id| (id, path.clone()))
            })
            .collect();
        let owned: Vec<(String, std::path::PathBuf, Option<String>, Option<String>)> = located
            .into_iter()
            .map(|(id, path)| {
                let st = self.pane_status.get(&id);
                (
                    id,
                    path,
                    st.and_then(|s| s.agent.clone()),
                    st.and_then(|s| s.owner_session.clone()),
                )
            })
            .collect();
        let leaves: Vec<hooks::server::HookLeaf<'_>> = owned
            .iter()
            .map(|(id, path, agent, session)| hooks::server::HookLeaf {
                id,
                path,
                agent: agent.as_deref(),
                session_id: session.as_deref(),
            })
            .collect();
        hooks::server::resolve_hook_target(
            header.as_deref(),
            header_path.as_deref(),
            incoming.cwd.as_deref().map(std::path::Path::new),
            incoming.mapped.session_id.as_deref(),
            &incoming.agent,
            &leaves,
        )
        .and_then(|id| self.resolve_pane(&id))
    }

    fn resolve_pane(&self, id: &str) -> Option<String> {
        if self
            .tabs
            .iter()
            .any(|w| w.owns_pane(id) || w.id == id)
        {
            return Some(id.to_string());
        }
        if self.adopted_from.as_deref() == Some(id)
            && self.tabs.iter().any(|w| w.id == catalog::LIVE_ID)
        {
            return Some(catalog::LIVE_ID.into());
        }
        None
    }

    fn tab_for_pane(&self, pane_id: &str) -> Option<&catalog::Tab> {
        self.tabs
            .iter()
            .find(|w| w.owns_pane(pane_id) || w.id == pane_id)
    }

    fn sync_row(&mut self, pane_id: &str) {
        let Some(ws_id) = self.tab_for_pane(pane_id).map(|w| w.id.clone()) else {
            return;
        };
        let leaves = self
            .tabs
            .iter()
            .find(|w| w.id == ws_id)
            .map(|w| w.layout().leaves())
            .unwrap_or_default();
        let (rolled, agent) = {
            let panes: Vec<_> = leaves
                .iter()
                .filter_map(|id| self.pane_status.get(id))
                .collect();
            (
                status::rollup_tracked(panes.iter().copied()),
                status::loudest_agent(panes.iter().copied()),
            )
        };
        if let Some(ws) = catalog::find_tab_mut(&mut self.tabs, &ws_id) {
            ws.status = rolled;
            ws.agent = agent;
        }
    }

    fn sync_all_rows(&mut self) {
        let ids: Vec<String> = self.tabs.iter().map(|w| w.id.clone()).collect();
        for id in ids {
            if let Some(first) = self
                .tabs
                .iter()
                .find(|w| w.id == id)
                .and_then(|w| w.layout().leaves().into_iter().next())
            {
                self.sync_row(&first);
            }
        }
    }

    fn persist_catalog(&self) {
        // Each project keeps the tab its window last showed.
        let projects = self
            .projects
            .iter()
            .map(|p| catalog::Project {
                selected: self.selected_of(&p.id).map(str::to_string),
                ..p.clone()
            })
            .collect();
        catalog::save(&catalog::Catalog {
            version: 1,
            selected: if self.selected.is_empty() {
                None
            } else {
                Some(self.selected.clone())
            },
            projects,
            tabs: self.tabs.clone(),
        });
    }

    fn begin_rename(&mut self, id: String) -> Task<Msg> {
        let Some(ws) = self.tabs.iter().find(|w| w.id == id) else {
            return Task::none();
        };
        if !catalog::can_close(ws) {
            return Task::none();
        }
        self.renaming = Some((id, cli::rail_label(ws)));
        Task::batch([
            iced::widget::operation::focus::<Msg>(iced::widget::Id::new(topbar::RENAME_INPUT_ID)),
            iced::advanced::widget::operate(
                iced::advanced::widget::operation::text_input::select_all::<Msg>(
                    iced::widget::Id::new(topbar::RENAME_INPUT_ID),
                ),
            ),
        ])
    }

    fn commit_rename(&mut self) -> Task<Msg> {
        let Some((id, name)) = self.renaming.take() else {
            return Task::none();
        };
        if let Some(ws) = self.tabs.iter_mut().find(|w| w.id == id) {
            if !catalog::can_close(ws) {
                return Task::none();
            }
            cli::apply_rail_title(ws, &name);
            self.persist_catalog();
        }
        Task::none()
    }

    fn select_tab(&mut self, id: &str) -> Task<Msg> {
        if self.tabs.iter().all(|w| w.id != id) {
            return Task::none();
        }
        self.renaming = None;
        self.selected = id.to_string();
        if let Some(ws) = self.tabs.iter().find(|w| w.id == id) {
            self.focused = ws.active_pane_id();
        }
        self.persist_catalog();
        let attach = self.attach_tab(id);
        // A newly shown tab takes its window's size.
        self.note_selected();
        self.resize_all_panes();
        attach
    }

    /// A top-bar drag let go over `onto`: `from` takes its place (after it
    /// when dragged rightwards, before it when leftwards).
    fn drop_tab_on(&mut self, from: &str, onto: &str) {
        if from == onto {
            return;
        }
        let Some(project) = self.tabs.iter().find(|w| w.id == from).map(|w| w.project_id.clone())
        else {
            return;
        };
        let order: Vec<String> = catalog::ordered_for_project(&project, &self.tabs)
            .iter()
            .map(|w| w.id.clone())
            .collect();
        let Some(before) = catalog::drop_place(&order, from, onto) else {
            return;
        };
        match catalog::move_tab_before(&mut self.tabs, from, before.as_deref()) {
            Ok(()) => self.persist_catalog(),
            Err(e) => tracing::warn!(%from, %onto, "tab drop: {e}"),
        }
    }

    /// Select tab `id` and bring its project's window up (opening it if it
    /// was closed): `tab.select`, `--select`, a desk-card click.
    fn show_tab(&mut self, id: &str) -> Task<Msg> {
        let Some(project) = self.tabs.iter().find(|w| w.id == id).map(|w| w.project_id.clone())
        else {
            return Task::none();
        };
        let select = self.select_tab(id);
        Task::batch([select, self.show_project(&project), self.reveal_tab(id)])
    }

    /// Scroll a project window's top bar to tab `id` (by its place in the
    /// order) when it was selected from elsewhere: the card, the CLI, a
    /// desk card. A clicked tab is already in view.
    fn reveal_tab(&self, id: &str) -> Task<Msg> {
        let Some(project) = self.tabs.iter().find(|w| w.id == id).map(|w| w.project_id.clone())
        else {
            return Task::none();
        };
        let order = catalog::ordered_for_project(&project, &self.tabs);
        let Some(at) = order.iter().position(|w| w.id == id) else {
            return Task::none();
        };
        let x = if order.len() <= 1 {
            0.0
        } else {
            at as f32 / (order.len() - 1) as f32
        };
        iced::widget::operation::snap_to(
            topbar::strip_id(&project),
            iced::widget::scrollable::RelativeOffset { x: Some(x), y: None },
        )
    }

    /// Bring project `project_id`'s window up, opening it if it was closed.
    fn show_project(&mut self, project_id: &str) -> Task<Msg> {
        if self.windows.has(project_id) {
            self.windows.show(project_id);
            Task::none()
        } else {
            self.open_window(project_id, false)
        }
    }

    fn open_spawn(&mut self, project_id: &str) -> Task<Msg> {
        let Some(project) = self.projects.iter().find(|p| p.id == project_id) else {
            return Task::none();
        };
        let root_branch = spawn::current_branch(&project.root);
        self.add = cards::AddDraft::default();
        self.startup = cards::StartupDraft::default();
        self.renaming = None;
        self.spawn = cards::SpawnDraft::open(project_id, root_branch);
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(cards::SPAWN_INPUT_ID))
    }

    fn open_add(&mut self) -> Task<Msg> {
        self.spawn = cards::SpawnDraft::default();
        self.startup = cards::StartupDraft::default();
        self.renaming = None;
        self.add = cards::AddDraft {
            open: true,
            path: String::new(),
            error: None,
        };
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(cards::ADD_INPUT_ID))
    }

    fn selected_project_id(&self) -> Option<String> {
        self.tabs
            .iter()
            .find(|w| w.id == self.selected)
            .map(|w| w.project_id.clone())
            .or_else(|| self.projects.first().map(|p| p.id.clone()))
    }

    fn open_startup(&mut self) -> Task<Msg> {
        let Some(pid) = self.selected_project_id() else {
            return self.open_add();
        };
        let script = self
            .projects
            .iter()
            .find(|p| p.id == pid)
            .map(|p| p.startup.clone())
            .unwrap_or_default();
        self.spawn = cards::SpawnDraft::default();
        self.add = cards::AddDraft::default();
        self.startup = cards::StartupDraft::open(pid, &script);
        Task::none()
    }

    fn save_startup(&mut self) -> Task<Msg> {
        let Some(pid) = self.startup.project_id.clone() else {
            return Task::none();
        };
        let text = self.startup.content.text();
        if let Some(p) = self.projects.iter_mut().find(|p| p.id == pid) {
            p.startup = text;
        }
        self.persist_catalog();
        self.startup = cards::StartupDraft::default();
        Task::none()
    }

    fn spawn_sibling(&mut self) -> Task<Msg> {
        let Some(project_id) = self.spawn.project_id.clone() else {
            return Task::none();
        };
        let name = self.spawn.name.clone();
        let in_root = self.spawn.in_root;
        match self.spawn_tab(
            &project_id,
            &name,
            None,
            None,
            None,
            None,
            None,
            in_root,
            true,
        ) {
            Ok((id, startup_err)) => {
                self.spawn = cards::SpawnDraft::default();
                self.maybe_toast_startup(startup_err, &id);
                Task::batch([self.attach_pane(&id, &[]), self.reveal_tab(&id)])
            }
            Err(e) => {
                self.spawn.error = Some(e);
                Task::none()
            }
        }
    }

    /// Create a worktree (or a root tab when `in_root`) + catalog row.
    /// Does not attach a PTY. Second value is a startup-script error
    /// (tab still exists). `select` takes the rail (UI /
    /// `--select`). CLI default is false.
    fn spawn_tab(
        &mut self,
        project_q: &str,
        name: &str,
        parent: Option<String>,
        agent: Option<&str>,
        branch: Option<&str>,
        base: Option<&str>,
        title: Option<&str>,
        in_root: bool,
        select: bool,
    ) -> Result<(String, Option<String>), String> {
        let project = catalog::resolve_project(&self.projects, project_q)?.clone();
        let slug = spawn::slug(name);
        if slug.is_empty() {
            return Err("name needs a letter or number".into());
        }
        let agent = cli::first_class_agent(agent)?;
        let (dest, kind) = if in_root {
            let given = |v: Option<&str>| v.is_some_and(|s| !s.trim().is_empty());
            if given(branch) || given(base) {
                return Err("--branch / --base-branch need a worktree (drop --root)".into());
            }
            if catalog::worktree_name_taken(&self.tabs, &project.id, &slug, "") {
                return Err(format!("tab '{slug}' already exists"));
            }
            (project.root.clone(), catalog::Kind::Root)
        } else {
            let dest = spawn::add_worktree_at(&project.root, &slug, branch, base)?;
            (dest, catalog::Kind::Worktree)
        };
        let taken: HashSet<String> = self.tabs.iter().map(|w| w.id.clone()).collect();
        let id = catalog::unique_id("ws", &slug, &taken);
        let parent = match parent {
            Some(p) => Some(
                catalog::resolve_tab(&self.tabs, &p)?
                    .id
                    .clone(),
            ),
            None => self
                .tabs
                .iter()
                .find(|w| w.project_id == project.id && w.kind == catalog::Kind::Main)
                .map(|w| w.id.clone()),
        };
        let title = title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ws = catalog::Tab {
            id: id.clone(),
            project_id: project.id.clone(),
            name: name.trim().to_string(),
            title,
            path: dest,
            kind,
            parent,
            layout: None,
            active_pane: None,
            status: status::AgentStatus::Idle,
            agent: agent.map(str::to_string),
        };
        // Startup prepares a fresh worktree; a root tab has none.
        let startup_err = if in_root {
            None
        } else {
            startup::run(&project, &ws).err()
        };
        if let Some(e) = &startup_err {
            tracing::warn!(tab = %ws.id, "{e}");
        }
        self.tabs.push(ws);
        catalog::apply_spawn_focus(&mut self.selected, &mut self.focused, &id, select);
        self.persist_catalog();
        Ok((id, startup_err))
    }

    fn toast_place(&self, pane_id: &str) -> (Option<String>, String) {
        let Some(ws) = self.tab_for_pane(pane_id) else {
            return (None, pane_id.to_string());
        };
        let project =
            catalog::find_project(&self.projects, &ws.project_id).map(|p| p.name.clone());
        (project, cli::rail_label(ws))
    }

    fn emit_notice(&self, title: String, body: String, tag: String) {
        if let Ok(mut client) = bus().lock() {
            let _ = client.emit(Topic::AppNotification(AppNotification {
                id: format!("ws-{tag}-{}", now_millis()),
                app_id: APP_ID.into(),
                source: "Projects".into(),
                title,
                body,
                tag: Some(tag),
                tab_id: None,
                url: None,
            }));
        }
    }

    fn maybe_toast_startup(&self, err: Option<String>, ws_id: &str) {
        let Some(e) = err else {
            return;
        };
        let (project, tab) = self.toast_place(ws_id);
        self.emit_notice(
            cli::place_label(project.as_deref(), &tab),
            e,
            format!("startup-{ws_id}"),
        );
    }

    fn maybe_toast_status(
        &self,
        id: &str,
        prev: status::AgentStatus,
        now: status::AgentStatus,
        unconfirmed: bool,
    ) {
        if now == prev {
            return;
        }
        if unconfirmed || self.window_focused {
            return;
        }
        let (project, tab) = self.toast_place(id);
        let agent = self
            .pane_status
            .get(id)
            .and_then(|s| s.agent.as_deref())
            .unwrap_or("agent");
        let Some(notice) = cli::status_notice(project.as_deref(), &tab, agent, now) else {
            return;
        };
        let kind = match now {
            status::AgentStatus::Done => "done",
            status::AgentStatus::Waiting => "waiting",
            _ => return,
        };
        self.emit_notice(notice.title, notice.body, format!("{kind}-{id}"));
    }

    fn on_call(&mut self, inc: sola_call::Incoming) -> Task<Msg> {
        if inc.method == "pane.wait" {
            return self.cli_wait(inc);
        }
        let (result, task) = self.dispatch_call(&inc.method, &inc.params);
        match result {
            Ok(data) => inc.reply.ok(data),
            Err(e) => inc.reply.err(e),
        }
        task
    }

    fn dispatch_call(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> (Result<serde_json::Value, String>, Task<Msg>) {
        match method {
            "ps" => (Ok(self.ps_json()), Task::none()),
            "project.list" => {
                let projects: Vec<serde_json::Value> =
                    self.projects.iter().map(cli::project_json).collect();
                (
                    Ok(serde_json::json!({ "projects": projects })),
                    Task::none(),
                )
            }
            "project.add" => {
                let Some(path) = param_str(params, "path") else {
                    return (Err("missing path".into()), Task::none());
                };
                match self.cli_add_project(&path) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "project.startup" => (
                self.cli_startup(
                    param_str(params, "project").as_deref(),
                    params.get("script").and_then(|v| v.as_str()),
                    params.get("script").is_some(),
                ),
                Task::none(),
            ),
            "project.rm" => {
                let Some(q) = param_str(params, "project") else {
                    return (Err("missing project".into()), Task::none());
                };
                match self.cli_rm_project(&q) {
                    // Reply first; teardown on the next tick so solactl
                    // (maybe running in a pane we are about to kill)
                    // gets {ok:true} instead of a hang / timeout.
                    Ok(id) => (
                        Ok(serde_json::json!({ "ok": true })),
                        Task::done(Msg::DropProject(id)),
                    ),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "tab.list" => (
                self.cli_tab_list(param_str(params, "project").as_deref()),
                Task::none(),
            ),
            "tab.spawn" => {
                let Some(project) = param_str(params, "project") else {
                    return (Err("missing project".into()), Task::none());
                };
                let Some(name) = param_str(params, "name") else {
                    return (Err("missing name".into()), Task::none());
                };
                match self.cli_spawn(
                    &project,
                    &name,
                    param_str(params, "agent").as_deref(),
                    param_str(params, "prompt").as_deref(),
                    param_str(params, "prompt-file").as_deref(),
                    param_str(params, "parent"),
                    param_str(params, "branch").as_deref(),
                    param_str(params, "base-branch").as_deref(),
                    param_str(params, "title").as_deref(),
                    param_bool(params, "root"),
                    params
                        .get("select")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                ) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "tab.rm" => {
                let Some(ws) = param_str(params, "tab") else {
                    return (Err("missing tab".into()), Task::none());
                };
                let checkout = param_bool(params, "worktree");
                let force = param_bool(params, "force");
                if force && !checkout {
                    return (Err("--force needs --worktree".into()), Task::none());
                }
                match self.cli_rm(&ws, checkout) {
                    // Reply first; teardown on the next tick so solactl
                    // (maybe running in a pane we are about to kill)
                    // gets {ok:true} instead of a hang / timeout.
                    Ok(id) => (
                        Ok(serde_json::json!({ "ok": true })),
                        if checkout {
                            Task::done(Msg::RmCheckout { id, force })
                        } else {
                            Task::done(Msg::CloseTab(id))
                        },
                    ),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "tab.select" => {
                let Some(q) = param_str(params, "tab") else {
                    return (Err("missing tab".into()), Task::none());
                };
                match self.cli_select(&q) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "tab.set" => {
                let Some(q) = param_str(params, "tab") else {
                    return (Err("missing tab".into()), Task::none());
                };
                (
                    self.cli_set(
                        &q,
                        params.get("name").and_then(|v| v.as_str()),
                        params.get("title").and_then(|v| v.as_str()),
                        params.get("branch").and_then(|v| v.as_str()),
                        param_before(params, "before"),
                    ),
                    Task::none(),
                )
            }
            "project.reorder" => {
                let Some(q) = param_str(params, "project") else {
                    return (Err("missing project".into()), Task::none());
                };
                (
                    self.cli_reorder_project(&q, param_before(params, "before")),
                    Task::none(),
                )
            }
            "tab.exec" => {
                let Some(q) = param_str(params, "tab") else {
                    return (Err("missing tab".into()), Task::none());
                };
                match self.cli_exec(
                    &q,
                    param_str(params, "agent").as_deref(),
                    param_str(params, "prompt").as_deref(),
                    param_str(params, "prompt-file").as_deref(),
                ) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "pane.list" => (
                self.cli_pane_list(param_str(params, "tab").as_deref()),
                Task::none(),
            ),
            "pane.send" => {
                let Some(text) = param_str(params, "text") else {
                    return (Err("missing text".into()), Task::none());
                };
                let enter = params
                    .get("enter")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                match self.cli_send(param_str(params, "pane").as_deref(), &text, enter) {
                    Ok(pane) => (
                        Ok(serde_json::json!({ "ok": true, "pane": pane })),
                        Task::none(),
                    ),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "pane.read" => {
                let lines = params
                    .get("lines")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                match self.cli_read(param_str(params, "pane").as_deref(), lines) {
                    Ok((pane, text)) => (
                        Ok(serde_json::json!({ "text": text, "pane": pane })),
                        Task::none(),
                    ),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "whoami" => (
                self.cli_whoami(
                    param_str(params, "pane").as_deref(),
                    param_str(params, "path").as_deref(),
                ),
                Task::none(),
            ),
            other => (Err(format!("unknown method {other}")), Task::none()),
        }
    }

    fn ps_json(&self) -> serde_json::Value {
        let projects: Vec<serde_json::Value> = self
            .projects
            .iter()
            .map(|p| {
                let tabs: Vec<serde_json::Value> =
                    catalog::ordered_for_project(&p.id, &self.tabs)
                        .into_iter()
                        .map(|w| cli::tab_json(w, Some(&self.selected)))
                        .collect();
                let mut row = cli::project_json(p);
                row["tabs"] = serde_json::Value::Array(tabs);
                row
            })
            .collect();
        serde_json::json!({ "projects": projects, "selected": self.selected })
    }

    fn cli_tab_list(&self, project: Option<&str>) -> Result<serde_json::Value, String> {
        let filter = match project {
            Some(q) => Some(catalog::resolve_project(&self.projects, q)?.id.clone()),
            None => None,
        };
        let list: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .filter(|w| filter.as_ref().is_none_or(|id| w.project_id == *id))
            .map(|w| cli::tab_json(w, None))
            .collect();
        Ok(serde_json::json!({ "tabs": list }))
    }

    fn cli_spawn(
        &mut self,
        project: &str,
        name: &str,
        agent: Option<&str>,
        prompt: Option<&str>,
        prompt_file: Option<&str>,
        parent: Option<String>,
        branch: Option<&str>,
        base: Option<&str>,
        title: Option<&str>,
        in_root: bool,
        select: bool,
    ) -> Result<(serde_json::Value, Task<Msg>), String> {
        let prompt = cli::read_prompt(prompt, prompt_file)?;
        let agent = match (cli::first_class_agent(agent)?, prompt.as_deref()) {
            (Some(a), _) => Some(a),
            (None, Some(_)) => Some("grok"),
            (None, None) => None,
        };
        let (id, startup_err) = self.spawn_tab(
            project, name, parent, agent, branch, base, title, in_root, select,
        )?;
        let task = if let Some(a) = agent {
            let args = cli::agent_argv(a, prompt.as_deref());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.attach_pane(&id, &refs)
        } else {
            self.attach_pane(&id, &[])
        };
        let mut data = self
            .tabs
            .iter()
            .find(|w| w.id == id)
            .map(|w| cli::spawn_json(w, select))
            .unwrap_or_else(|| serde_json::json!({ "id": id, "selected": select }));
        if let Some(e) = startup_err {
            data["startup_error"] = serde_json::json!(e);
        }
        let task = match self.tabs.iter().find(|w| w.id == id) {
            Some(w) if select => {
                let project = w.project_id.clone();
                Task::batch([task, self.show_project(&project), self.reveal_tab(&id)])
            }
            _ => task,
        };
        Ok((data, task))
    }

    fn cli_startup(
        &mut self,
        project: Option<&str>,
        script: Option<&str>,
        set: bool,
    ) -> Result<serde_json::Value, String> {
        let q = match project {
            Some(q) => q.to_string(),
            None => self
                .selected_project_id()
                .ok_or_else(|| "no project selected".to_string())?,
        };
        let id = catalog::resolve_project(&self.projects, &q)?.id.clone();
        if set {
            if let Some(p) = self.projects.iter_mut().find(|p| p.id == id) {
                p.startup = script.unwrap_or("").to_string();
            }
            self.persist_catalog();
        }
        let p = catalog::resolve_project(&self.projects, &id)?;
        Ok(serde_json::json!({
            "project": p.id,
            "name": p.name,
            "script": p.startup,
        }))
    }

    fn cli_rm(&self, q: &str, checkout: bool) -> Result<String, String> {
        let ws = catalog::resolve_tab(&self.tabs, q)?;
        if !catalog::can_close(ws) {
            return Err("cannot close the project root".into());
        }
        if checkout && !catalog::has_worktree(ws) {
            return Err(format!(
                "{} has no worktree (root tab) — drop --worktree to close the tab",
                ws.id
            ));
        }
        Ok(ws.id.clone())
    }

    fn cli_rm_project(&self, q: &str) -> Result<String, String> {
        Ok(catalog::resolve_project(&self.projects, q)?.id.clone())
    }

    fn cli_select(&mut self, q: &str) -> Result<(serde_json::Value, Task<Msg>), String> {
        let id = catalog::resolve_tab(&self.tabs, q)?
            .id
            .clone();
        let task = self.show_tab(&id);
        Ok((serde_json::json!({ "id": id, "selected": true }), task))
    }

    fn cli_set(
        &mut self,
        q: &str,
        name: Option<&str>,
        title: Option<&str>,
        branch: Option<&str>,
        before: Option<Option<String>>,
    ) -> Result<serde_json::Value, String> {
        let id = catalog::resolve_tab(&self.tabs, q)?
            .id
            .clone();
        let mut err = None;
        if let Some(raw) = name {
            if let Err(e) = self.rename_tab(&id, raw) {
                err = Some(e);
            }
        }
        if err.is_none() {
            if let Some(raw) = branch {
                if let Err(e) = self.rename_tab_branch(&id, raw) {
                    err = Some(e);
                }
            }
        }
        if let Some(raw) = title {
            if let Some(ws) = self.tabs.iter_mut().find(|w| w.id == id) {
                cli::apply_rail_title(ws, raw);
            }
        }
        if err.is_none() {
            if let Some(before) = before {
                let before_id = match before.as_deref() {
                    None => None,
                    Some(q) => Some(
                        catalog::resolve_tab(&self.tabs, q)?
                            .id
                            .clone(),
                    ),
                };
                if let Err(e) = catalog::move_tab_before(
                    &mut self.tabs,
                    &id,
                    before_id.as_deref(),
                ) {
                    err = Some(e);
                }
            }
        }
        self.persist_catalog();
        if let Some(e) = err {
            return Err(e);
        }
        let ws = catalog::resolve_tab(&self.tabs, &id)?;
        Ok(cli::tab_json(ws, Some(&self.selected)))
    }

    fn cli_reorder_project(
        &mut self,
        q: &str,
        before: Option<Option<String>>,
    ) -> Result<serde_json::Value, String> {
        let id = catalog::resolve_project(&self.projects, q)?.id.clone();
        let before_id = match before {
            None => None,
            Some(None) => None,
            Some(Some(q)) => Some(catalog::resolve_project(&self.projects, &q)?.id.clone()),
        };
        // Missing --before still moves to end when the method is called.
        catalog::move_project_before(&mut self.projects, &id, before_id.as_deref())?;
        self.persist_catalog();
        Ok(cli::project_json(catalog::resolve_project(
            &self.projects,
            &id,
        )?))
    }

    /// Rail slug + `git worktree move` to `.worktrees/<slug>`. Id stays.
    fn rename_tab(&mut self, id: &str, raw: &str) -> Result<(), String> {
        let slug = spawn::slug(raw);
        spawn::check_slug(&slug)?;
        let ws = catalog::resolve_tab(&self.tabs, id)?;
        if ws.kind == catalog::Kind::Root {
            return Err("this tab has no worktree to move — use --title for the rail label".into());
        }
        if !catalog::has_worktree(ws) {
            return Err("cannot rename the project root".into());
        }
        let project_id = ws.project_id.clone();
        let from = ws.path.clone();
        if catalog::worktree_name_taken(&self.tabs, &project_id, &slug, id) {
            return Err(format!("tab '{slug}' already exists"));
        }
        let root = catalog::resolve_project(&self.projects, &project_id)?
            .root
            .clone();
        let dest = spawn::worktree_path(&root, &slug);
        let path_changed = !catalog::path_same(&from, &dest);
        if path_changed {
            spawn::move_worktree(&root, &from, &slug)?;
        }
        if let Some(ws) = self.tabs.iter_mut().find(|w| w.id == id) {
            ws.name = slug;
            ws.path = dest;
        } else {
            return Err(format!("unknown tab '{id}'"));
        }
        if path_changed {
            self.restamp_tab_path(id);
        }
        Ok(())
    }

    fn rename_tab_branch(&self, id: &str, raw: &str) -> Result<(), String> {
        let ws = catalog::resolve_tab(&self.tabs, id)?;
        if ws.kind == catalog::Kind::Root {
            return Err("this tab has no worktree — it uses the project root's branch".into());
        }
        if !catalog::has_worktree(ws) {
            return Err("cannot rename the project root branch".into());
        }
        spawn::rename_branch(&ws.path, raw)
    }

    fn restamp_tab_path(&self, id: &str) {
        let Some(ws) = self.tabs.iter().find(|w| w.id == id) else {
            return;
        };
        let cwd = ws.path.to_string_lossy();
        for pane in ws.layout().leaves() {
            let session = tmux::session_name(&pane);
            if tmux::has_session(&session) {
                tmux::set_environment(&session, catalog::SOLA_WS_PATH, cwd.as_ref());
            }
        }
    }

    fn cli_exec(
        &mut self,
        q: &str,
        agent: Option<&str>,
        prompt: Option<&str>,
        prompt_file: Option<&str>,
    ) -> Result<(serde_json::Value, Task<Msg>), String> {
        let agent = cli::first_class_agent(agent)?.unwrap_or("grok");
        let prompt = cli::read_prompt(prompt, prompt_file)?;
        let ws_id = catalog::resolve_tab(&self.tabs, q)?
            .id
            .clone();
        let pane = self.preferred_pane_for(Some(&ws_id), Some(agent))?;
        let is_same = self
            .pane_status
            .get(&pane)
            .and_then(|s| s.agent.as_deref())
            .is_some_and(|a| a.eq_ignore_ascii_case(agent));
        if is_same {
            if let Some(text) = prompt.as_deref() {
                self.write_pane(&pane, text, true)?;
            }
            return Ok((
                serde_json::json!({
                    "tab": ws_id,
                    "pane": pane,
                    "started": false,
                    "sent": prompt.is_some(),
                }),
                Task::none(),
            ));
        }
        let session = tmux::session_name(&pane);
        let new_session = !tmux::has_session(&session);
        let task = if new_session {
            let args = cli::agent_argv(agent, prompt.as_deref());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.attach_pane(&pane, &refs)
        } else {
            let attach = self.attach_pane(&pane, &[]);
            let line = cli::agent_shell_line(agent, prompt.as_deref());
            self.write_pane(&pane, &line, true)?;
            attach
        };
        Ok((
            serde_json::json!({
                "tab": ws_id,
                "pane": pane,
                "started": true,
                "sent": false,
            }),
            task,
        ))
    }

    fn cli_add_project(&mut self, path: &str) -> Result<(serde_json::Value, Task<Msg>), String> {
        let (project_id, main_id, task) = self.register_project(path)?;
        // CLI adds stay quiet: the window joins its zone behind.
        let task = Task::batch([task, self.open_window(&project_id, true)]);
        let project = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| "project vanished".to_string())?;
        let mut data = cli::project_json(project);
        data["tab"] = serde_json::json!(main_id);
        Ok((data, task))
    }

    fn cli_pane_list(&self, tab: Option<&str>) -> Result<serde_json::Value, String> {
        let id = match tab {
            Some(q) => catalog::resolve_tab(&self.tabs, q)?
                .id
                .clone(),
            None => self.selected.clone(),
        };
        if id.is_empty() {
            return Err("no tab selected".into());
        }
        let leaves = self
            .tabs
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.layout().leaves())
            .unwrap_or_else(|| vec![id.clone()]);
        let panes: Vec<serde_json::Value> = leaves
            .into_iter()
            .map(|pid| {
                let st = self.pane_status.get(&pid);
                cli::pane_json(
                    &pid,
                    st.map(|s| s.status).unwrap_or_default(),
                    st.and_then(|s| s.agent.as_deref()),
                    st.and_then(|s| s.owner_session.as_deref()),
                )
            })
            .collect();
        Ok(serde_json::json!({ "panes": panes }))
    }

    fn pane_agents(&self, leaves: &[String]) -> Vec<(String, Option<String>)> {
        leaves
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    self.pane_status.get(id).and_then(|s| s.agent.clone()),
                )
            })
            .collect()
    }

    fn preferred_pane(&self, hint: Option<&str>) -> Result<String, String> {
        self.preferred_pane_for(hint, Some("grok"))
    }

    fn preferred_pane_for(
        &self,
        hint: Option<&str>,
        prefer: Option<&str>,
    ) -> Result<String, String> {
        if let Some(q) = hint {
            if self.tabs.iter().any(|w| w.owns_pane(q) && w.id != q) {
                return Ok(q.to_string());
            }
            let ws = catalog::resolve_tab(&self.tabs, q)?;
            let leaves = ws.layout().leaves();
            let agents = self.pane_agents(&leaves);
            return Ok(cli::prefer_agent_pane(
                &leaves,
                &agents,
                &ws.active_pane_id(),
                None,
                prefer,
            ));
        }
        if !self.focused.is_empty() {
            return Ok(self.focused.clone());
        }
        if !self.selected.is_empty() {
            return self.preferred_pane_for(Some(&self.selected), prefer);
        }
        Err("no pane".into())
    }

    fn cli_pane_id(&self, pane: Option<&str>) -> Result<String, String> {
        self.preferred_pane(pane)
    }

    fn write_pane(&self, id: &str, text: &str, enter: bool) -> Result<(), String> {
        let session = tmux::session_name(id);
        tmux::send_prompt(&session, text, enter)
    }

    fn cli_send(&self, pane: Option<&str>, text: &str, enter: bool) -> Result<String, String> {
        let id = self.cli_pane_id(pane)?;
        self.write_pane(&id, text, enter)?;
        Ok(id)
    }

    fn cli_read(&self, pane: Option<&str>, lines: Option<u32>) -> Result<(String, String), String> {
        let id = self.cli_pane_id(pane)?;
        let session = tmux::session_name(&id);
        let text = tmux::capture_scrollback(&session)?;
        let text = match lines {
            Some(n) if n > 0 => {
                let keep = n as usize;
                let mut v: Vec<&str> = text.lines().collect();
                if v.len() > keep {
                    v = v.split_off(v.len() - keep);
                }
                v.join("\n")
            }
            _ => text,
        };
        Ok((id, text))
    }

    fn cli_whoami(
        &self,
        pane: Option<&str>,
        path: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let ws = if let Some(q) = pane {
            catalog::resolve_tab(&self.tabs, q)?
        } else if let Some(p) = path {
            catalog::resolve_tab(&self.tabs, p)?
        } else {
            return Err(
                "not in a Projects pane (pass --pane/--path or run from a Projects PTY)".into(),
            );
        };
        let pane_id = if let Some(q) = pane {
            if ws.owns_pane(q) {
                q.to_string()
            } else {
                self.preferred_pane(Some(&ws.id))?
            }
        } else {
            self.preferred_pane(Some(&ws.id))?
        };
        let st = self.pane_status.get(&pane_id);
        let project = catalog::resolve_project(&self.projects, &ws.project_id).ok();
        Ok(serde_json::json!({
            "pane": pane_id,
            "tab": ws.id,
            "tab_name": cli::display_name(ws),
            "project": ws.project_id,
            "project_name": project.map(|p| p.name.as_str()),
            "path": ws.path,
            "kind": cli::kind_str(ws.kind),
            "status": cli::status_str(st.map(|s| s.status).unwrap_or(ws.status)),
            "agent": st.and_then(|s| s.agent.clone()).or_else(|| ws.agent.clone()),
            "session_id": st.and_then(|s| s.owner_session.clone()),
        }))
    }

    fn cli_wait(&mut self, inc: sola_call::Incoming) -> Task<Msg> {
        let want = match param_str(&inc.params, "status") {
            Some(s) => match cli::parse_status(&s) {
                Ok(st) => st,
                Err(e) => {
                    inc.reply.err(e);
                    return Task::none();
                }
            },
            None => status::AgentStatus::Done,
        };
        let pane = match self.cli_pane_id(param_str(&inc.params, "pane").as_deref()) {
            Ok(id) => id,
            Err(e) => {
                inc.reply.err(e);
                return Task::none();
            }
        };
        let secs = cli::wait_timeout_secs(inc.params.get("timeout").and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        }));
        let fresh = inc
            .params
            .get("fresh")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let now = self
            .pane_status
            .get(&pane)
            .map(|s| s.status)
            .unwrap_or_default();
        if !fresh && now == want {
            inc.reply.ok(serde_json::json!({
                "pane": pane,
                "status": cli::status_str(want),
            }));
            return Task::none();
        }
        self.pending_waits.push(PendingWait {
            pane,
            want,
            fresh,
            armed: !fresh || now != want,
            reply: inc.reply,
            deadline: Instant::now() + Duration::from_secs(secs),
        });
        Task::none()
    }

    fn flush_waits(&mut self) {
        let now = Instant::now();
        let mut keep = Vec::new();
        for mut wait in self.pending_waits.drain(..) {
            if now >= wait.deadline {
                wait.reply.err(format!(
                    "timeout waiting for pane {} to be {}",
                    wait.pane,
                    cli::status_str(wait.want)
                ));
                continue;
            }
            let status = self
                .pane_status
                .get(&wait.pane)
                .map(|s| s.status)
                .unwrap_or_default();
            if wait.fresh && !wait.armed {
                if status != wait.want {
                    wait.armed = true;
                }
                keep.push(wait);
                continue;
            }
            if status == wait.want {
                wait.reply.ok(serde_json::json!({
                    "pane": wait.pane,
                    "status": cli::status_str(wait.want),
                }));
                continue;
            }
            keep.push(wait);
        }
        self.pending_waits = keep;
    }

    fn add_project(&mut self) -> Task<Msg> {
        let raw = self.add.path.clone();
        match self.register_project(&raw) {
            Ok((project_id, main_id, task)) => {
                self.add = cards::AddDraft::default();
                // The new project's window, in front, on its root tab.
                let open = self.open_window(&project_id, false);
                let select = self.select_tab(&main_id);
                Task::batch([task, open, select])
            }
            Err(e) => {
                self.add.error = Some(e);
                Task::none()
            }
        }
    }

    fn register_project(&mut self, raw: &str) -> Result<(String, String, Task<Msg>), String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("folder path required".into());
        }
        let root = match catalog::expand_user_path(raw).canonicalize() {
            Ok(p) if p.is_dir() => p,
            Ok(_) => return Err("not a folder".into()),
            Err(e) => return Err(format!("path: {e}")),
        };
        if self.projects.iter().any(|p| p.root == root) {
            return Err("already a project".into());
        }
        let slug = spawn::slug(
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("project"),
        );
        let taken_p: HashSet<String> = self.projects.iter().map(|p| p.id.clone()).collect();
        let project_id = catalog::unique_id("proj", &slug, &taken_p);
        let mut taken_w: HashSet<String> = self.tabs.iter().map(|w| w.id.clone()).collect();
        let main_id = catalog::main_tab_id(&root, &taken_w);
        taken_w.insert(main_id.clone());
        let (project, ws) = catalog::project_from_root(&root, &project_id, &main_id);
        self.projects.push(project);
        if self.selected.is_empty() {
            self.selected = ws.id.clone();
            self.focused = ws.active_pane_id();
        }
        let attach_id = ws.id.clone();
        self.tabs.push(ws);
        self.persist_catalog();
        let task = if self.selected == attach_id {
            self.attach_tab(&attach_id)
        } else {
            Task::none()
        };
        Ok((project_id, attach_id, task))
    }

    fn teardown_pane(&mut self, id: &str) {
        self.detach_pane(id).close();
    }

    /// Stop drawing this pane. Caller must `close()` the result (on the
    /// iced thread only when the tmux session name may be reused next,
    /// e.g. last-leaf ⌘W).
    fn detach_pane(&mut self, id: &str) -> DetachedPane {
        let backend = self.runtimes.remove(id).map(|rt| rt.backend);
        self.pane_status.remove(id);
        self.pane_grids.remove(id);
        DetachedPane {
            backend,
            session: tmux::session_name(id),
        }
    }

    fn attach_selected_if_needed(&mut self) -> Task<Msg> {
        if self.selected.is_empty() {
            self.focused.clear();
            return Task::none();
        }
        if let Some(ws) = self.tabs.iter().find(|w| w.id == self.selected) {
            self.focused = ws.active_pane_id();
        }
        let next = self.selected.clone();
        self.attach_tab(&next)
    }

    /// Close a sibling tab. `checkout` also `git worktree remove`s (after
    /// tmux dies so the pane is not sitting in that cwd). Hover × leaves
    /// the folder. A gone path just prunes git metadata.
    ///
    /// The rail and catalog update here; `tmux kill-session` and git run
    /// off the iced thread so a `solactl` rm (or hover ×) does not freeze
    /// every other pane.
    fn close_tab(&mut self, id: &str, checkout: bool, force: bool) -> Task<Msg> {
        let Some((panes, path, project_id, has_worktree)) =
            self.tabs.iter().find(|w| w.id == id).and_then(|ws| {
                catalog::can_close(ws).then(|| {
                    (
                        ws.layout().leaves(),
                        ws.path.clone(),
                        ws.project_id.clone(),
                        catalog::has_worktree(ws),
                    )
                })
            })
        else {
            return Task::none();
        };
        // A root tab's path is the project root — never `git worktree remove` it.
        let checkout = checkout && has_worktree;
        tracing::info!(tab = %id, checkout, "closing tab");
        let detached: Vec<DetachedPane> = panes.iter().map(|pane| self.detach_pane(pane)).collect();
        self.tabs.retain(|w| w.id != id);
        for w in &mut self.tabs {
            if w.parent.as_deref() == Some(id) {
                w.parent = None;
            }
        }
        if self.selected == id {
            self.selected = self
                .tabs
                .first()
                .map(|w| w.id.clone())
                .unwrap_or_default();
        }
        self.persist_catalog();
        status::persist_all(&self.pane_status);
        let git = checkout
            .then(|| {
                self.projects
                    .iter()
                    .find(|p| p.id == project_id)
                    .map(|p| (p.root.clone(), path.clone(), force))
            })
            .flatten();
        let attach = self.attach_selected_if_needed();
        Task::batch([attach, close_io_task(detached, git, id.to_string())])
    }

    /// If a sibling's checkout is already gone (`git worktree remove` from
    /// inside the pane, or a deleted folder), drop the tab. The pane cannot
    /// call `tab.rm` after its cwd vanishes — the rail used to keep
    /// a working spinner forever.
    fn reap_missing_worktrees(&mut self) -> Task<Msg> {
        let gone = catalog::missing_worktree_ids(&self.tabs);
        if gone.is_empty() {
            return Task::none();
        }
        let mut tasks = Vec::with_capacity(gone.len());
        for id in gone {
            tracing::info!(tab = %id, "worktree path gone; closing tab");
            tasks.push(self.close_tab(&id, true, false));
        }
        Task::batch(tasks)
    }

    /// Unregister the project and every tab under it. Kills those
    /// tmux sessions. Leaves git worktrees and folders on disk.
    fn drop_project(&mut self, project_id: &str) -> Task<Msg> {
        let pane_ids: Vec<String> = self
            .tabs
            .iter()
            .filter(|w| w.project_id == project_id)
            .flat_map(|w| w.layout().leaves())
            .collect();
        let mut catalog = catalog::Catalog {
            version: 1,
            selected: if self.selected.is_empty() {
                None
            } else {
                Some(self.selected.clone())
            },
            projects: std::mem::take(&mut self.projects),
            tabs: std::mem::take(&mut self.tabs),
        };
        let removed = catalog::unregister_project(&mut catalog, project_id);
        self.projects = catalog.projects;
        self.tabs = catalog.tabs;
        self.selected = catalog.selected.unwrap_or_default();
        let detached: Vec<DetachedPane> = pane_ids.iter().map(|id| self.detach_pane(id)).collect();
        // Its window goes; with no project left, the no-project window.
        let mut tasks = Vec::new();
        if self.projects.is_empty() {
            tasks.push(self.open_window(windows::EMPTY, false));
        }
        if let Some(window) = self.windows.window_of(project_id) {
            self.windows.forget(window);
            tasks.push(window::close(window));
        }
        self.publish_menu();
        if removed.is_empty() {
            tasks.push(close_io_task(detached, None, project_id.to_string()));
            return Task::batch(tasks);
        }
        self.persist_catalog();
        status::persist_all(&self.pane_status);
        let attach = self.attach_selected_if_needed();
        tasks.push(attach);
        tasks.push(close_io_task(detached, None, project_id.to_string()));
        Task::batch(tasks)
    }

    fn split_focused(&mut self, dir: SplitDir) -> Task<Msg> {
        let Some(ws_idx) = self.tabs.iter().position(|w| w.id == self.selected) else {
            return Task::none();
        };
        let source = self.tabs[ws_idx].active_pane_id();
        if !self.tabs[ws_idx].owns_pane(&source) && source != self.tabs[ws_idx].id {
            return Task::none();
        }
        let mut taken: HashSet<String> = self
            .tabs
            .iter()
            .flat_map(|w| w.layout().leaves())
            .collect();
        for w in &self.tabs {
            taken.insert(w.id.clone());
            taken.extend(w.layout().split_ids());
        }
        let ws_id = self.tabs[ws_idx].id.clone();
        let new_pane = catalog::unique_id(&ws_id, "p", &taken);
        let split_id = catalog::unique_id("split", &ws_id, &taken);
        let mut node = self.tabs[ws_idx].layout().to_node();
        if !term_state::split_leaf(&mut node, &source, &split_id, dir, &new_pane) {
            return Task::none();
        }
        self.tabs[ws_idx].set_tree(node, new_pane.clone());
        self.focused = new_pane.clone();
        self.persist_catalog();
        self.attach_pane(&new_pane, &[])
    }

    fn close_pane(&mut self, pane_id: &str) -> Task<Msg> {
        let Some(ws_idx) = self.tabs.iter().position(|w| w.owns_pane(pane_id)) else {
            return Task::none();
        };
        let ws_id = self.tabs[ws_idx].id.clone();
        let node = self.tabs[ws_idx].layout().to_node();
        let next_focus = term_state::sibling_first_leaf(&node, pane_id);
        match term_state::close_leaf(node, pane_id) {
            None => {
                // Last leaf: kill the shell, keep the tab, reuse
                // the stable tab id for the next Start new shell.
                self.teardown_pane(pane_id);
                self.tabs[ws_idx]
                    .set_tree(term_state::PaneNode::Leaf(ws_id.clone()), ws_id.clone());
                self.focused = ws_id;
                self.sync_all_rows();
                self.persist_catalog();
                status::persist_all(&self.pane_status);
                Task::none()
            }
            Some(kept) => {
                let focus = next_focus
                    .or_else(|| term_state::leaves_of(&kept).into_iter().next())
                    .unwrap_or_else(|| ws_id.clone());
                self.teardown_pane(pane_id);
                self.tabs[ws_idx].set_tree(kept, focus.clone());
                self.focused = focus;
                self.sync_all_rows();
                self.persist_catalog();
                status::persist_all(&self.pane_status);
                self.resize_all_panes();
                Task::none()
            }
        }
    }

    fn drag_split(&mut self, split_id: &str, x: f32, y: f32) {
        // The split is in a tab one of the windows shows.
        let Some((window, tab_id)) = self.windows.iter().find_map(|(window, project_id)| {
            let id = self.selected_of(project_id)?;
            let ws = self.tabs.iter().find(|w| w.id == id)?;
            ws.layout()
                .split_ids()
                .iter()
                .any(|s| s == split_id)
                .then(|| (window, id.to_string()))
        }) else {
            return;
        };
        let Some(ws) = self.tabs.iter().find(|w| w.id == tab_id) else {
            return;
        };
        let node = ws.layout().to_node();
        let content = self.content_rect(window);
        let Some((_, area, dir)) = term_state::split_rects(&node, content)
            .into_iter()
            .find(|(id, _, _)| id == split_id)
        else {
            return;
        };
        let ratio = term_state::ratio_for_drag(area, dir, x, y, MIN_PANE_PX);
        let Some(ws) = self.tabs.iter_mut().find(|w| w.id == tab_id) else {
            return;
        };
        let mut node = ws.layout().to_node();
        if term_state::set_ratio(&mut node, split_id, ratio) {
            let active = ws.active_pane_id();
            ws.set_tree(node, active);
            self.resize_all_panes();
        }
    }

    /// Where a project window's panes sit: under its top bar (and the
    /// hairline below it).
    fn content_rect(&self, window: window::Id) -> term_state::Rect {
        let size = self.windows.size(window).unwrap_or(DEFAULT_WINDOW);
        let top = topbar::HEIGHT + topbar::RULE;
        term_state::Rect {
            x: 0.0,
            y: top,
            w: size.width,
            h: (size.height - top).max(0.0),
        }
    }

    /// Size each window's shown panes to the window.
    fn resize_all_panes(&mut self) {
        let mut targets: Vec<(String, u16, u16)> = Vec::new();
        for (window, project_id) in self.windows.iter() {
            let Some(ws) = self
                .selected_of(project_id)
                .and_then(|id| self.tabs.iter().find(|w| w.id == id))
            else {
                continue;
            };
            let content = self.content_rect(window);
            for (id, rect) in term_state::pane_rects(&ws.layout().to_node(), content) {
                let (c, r) =
                    term_view::cols_rows_for(iced::Size::new(rect.w, rect.h), self.metrics);
                targets.push((id, c.max(2), r.max(1)));
            }
        }
        for (pane_id, cols, rows) in targets {
            if self.pane_grids.get(&pane_id) == Some(&(cols, rows)) {
                continue;
            }
            let Some(rt) = self.runtimes.get(&pane_id) else {
                continue;
            };
            rt.emulator.resize(cols, rows);
            rt.backend.resize(cols, rows);
            rt.backend.sigwinch();
            rt.cache.clear();
            self.pane_grids.insert(pane_id, (cols, rows));
        }
    }

    fn dialog_open(&self) -> bool {
        self.spawn.is_open() || self.add.open || self.startup.is_open()
    }

    fn copy_selection(&self) -> Task<Msg> {
        if self.startup.is_open() {
            if let Some(sel) = self.startup.content.selection() {
                if !sel.is_empty() {
                    return iced::clipboard::write(sel);
                }
            }
            let t = self.startup.content.text();
            if !t.is_empty() {
                return iced::clipboard::write(t);
            }
            return Task::none();
        }
        if self.spawn.is_open() && !self.spawn.name.is_empty() {
            return iced::clipboard::write(self.spawn.name.clone());
        }
        if self.add.open && !self.add.path.is_empty() {
            return iced::clipboard::write(self.add.path.clone());
        }
        if let Some((_, draft)) = &self.renaming {
            if !draft.is_empty() {
                return iced::clipboard::write(draft.clone());
            }
            return Task::none();
        }
        let Some(rt) = self.runtimes.get(&self.focused) else {
            return Task::none();
        };
        let text = { rt.emulator.term().lock().selection_to_string() };
        match text {
            Some(s) if !s.is_empty() => iced::clipboard::write(s),
            _ => Task::none(),
        }
    }

    fn paste_clipboard(&self) -> Task<Msg> {
        iced::clipboard::read().map(Msg::Pasted)
    }

    fn on_pasted(&mut self, text: Option<String>) -> Task<Msg> {
        let Some(text) = text else {
            return Task::none();
        };
        if self.startup.is_open() {
            self.startup
                .content
                .perform(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Paste(std::sync::Arc::new(text)),
                ));
            return Task::none();
        }
        if self.spawn.is_open() {
            self.spawn.name.push_str(&text.replace('\n', ""));
            return Task::none();
        }
        if self.add.open {
            self.add.path.push_str(&text.replace('\n', ""));
            return Task::none();
        }
        if let Some((_, draft)) = &mut self.renaming {
            draft.push_str(&text.replace('\n', ""));
            return Task::none();
        }
        let Some(rt) = self.runtimes.get(&self.focused) else {
            return Task::none();
        };
        let mode = { *rt.emulator.term().lock().mode() };
        rt.backend.write(&input::paste(&text, mode));
        Task::none()
    }

    fn on_input(&mut self, event: iced::Event) -> Task<Msg> {
        // Modifier tracking must run even while a spawn / add / startup
        // dialog is open. ⌘T and ⌘N open those dialogs; if Super-up is
        // dropped here, `keys_held_mods` / a latched snapshot keep LOGO
        // and every later key is swallowed until quit.
        if let iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) = event {
            self.keyboard_mods = mods;
            return Task::none();
        }
        if let iced::Event::Keyboard(keyboard::Event::KeyReleased {
            key, physical_key, ..
        }) = &event
        {
            self.apply_modifier_key(key, physical_key, false);
            return Task::none();
        }

        if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) = &event
        {
            if self.renaming.is_some() {
                self.renaming = None;
                return Task::none();
            }
            if self.dialog_open() {
                self.spawn = cards::SpawnDraft::default();
                self.add = cards::AddDraft::default();
                self.startup = cards::StartupDraft::default();
                return Task::none();
            }
        }
        if self.dialog_open() || self.renaming.is_some() {
            return Task::none();
        }

        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key,
            modifiers,
            location,
            text,
            repeat,
            ..
        }) = event
        else {
            return Task::none();
        };

        self.apply_modifier_key(&key, &physical_key, true);
        if modifier_key_bit(&key, &physical_key).is_some() {
            return Task::none();
        }

        let modifiers = input::merge_modifiers(modifiers, self.keyboard_mods, self.keys_held_mods);
        if modifiers.logo() {
            return Task::none();
        }

        let Some(rt) = self.runtimes.get(&self.focused) else {
            return Task::none();
        };
        let mut mode = { *rt.emulator.term().lock().mode() };
        if extkeys::level(&self.focused) >= 1 {
            mode |= alacritty_terminal::term::TermMode::DISAMBIGUATE_ESC_CODES;
        }
        let mods = Mods::from(modifiers);
        let enter_key = match (&key, &modified_key) {
            (keyboard::Key::Named(keyboard::key::Named::Enter), _)
            | (_, keyboard::Key::Named(keyboard::key::Named::Enter)) => {
                keyboard::Key::Named(keyboard::key::Named::Enter)
            }
            _ => modified_key.clone(),
        };
        let Some(bytes) = input::resolve_bytes(&input::KeyInput {
            key: &key,
            modified_key: &enter_key,
            mods,
            mode,
            location,
            text: text.as_deref(),
            repeat,
            modify_other_keys: extkeys::level(&self.focused) >= 1,
        }) else {
            return Task::none();
        };
        {
            let term = rt.emulator.term();
            let mut guard = term.lock();
            if guard.grid().display_offset() != 0 {
                guard.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
            }
        }
        rt.backend.write(&bytes);
        Task::none()
    }

    fn apply_modifier_key(
        &mut self,
        key: &keyboard::Key,
        physical: &keyboard::key::Physical,
        pressed: bool,
    ) {
        let Some(bit) = modifier_key_bit(key, physical) else {
            return;
        };
        if pressed {
            self.keys_held_mods = self.keys_held_mods | bit;
        } else {
            self.keys_held_mods = self.keys_held_mods & !bit;
        }
    }
}

fn modifier_key_bit(
    key: &keyboard::Key,
    physical: &keyboard::key::Physical,
) -> Option<keyboard::Modifiers> {
    use keyboard::key::{Code, Named, Physical};
    if let keyboard::Key::Named(n) = key {
        match n {
            Named::Shift => return Some(keyboard::Modifiers::SHIFT),
            Named::Control => return Some(keyboard::Modifiers::CTRL),
            Named::Alt => return Some(keyboard::Modifiers::ALT),
            Named::Super | Named::Meta => return Some(keyboard::Modifiers::LOGO),
            _ => {}
        }
    }
    match physical {
        Physical::Code(Code::ShiftLeft | Code::ShiftRight) => Some(keyboard::Modifiers::SHIFT),
        Physical::Code(Code::ControlLeft | Code::ControlRight) => Some(keyboard::Modifiers::CTRL),
        Physical::Code(Code::AltLeft | Code::AltRight) => Some(keyboard::Modifiers::ALT),
        Physical::Code(Code::SuperLeft | Code::SuperRight) => Some(keyboard::Modifiers::LOGO),
        _ => None,
    }
}

fn param_str(params: &serde_json::Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `None` = key omitted. `Some(None)` = empty / `end` (append).
fn param_before(params: &serde_json::Value, key: &str) -> Option<Option<String>> {
    let raw = params.get(key)?.as_str()?.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("end") {
        Some(None)
    } else {
        Some(Some(raw.to_string()))
    }
}

fn param_bool(params: &serde_json::Value, key: &str) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}
