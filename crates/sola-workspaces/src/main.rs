//! sola-workspaces — project / workspace rail + agent-aware PTYs.
//!
//! Persist + spawn: catalog on disk, siblings under `.worktrees/`.
//! Grok hooks, OSC 9999, process-tree. Calls on sola-call owner `workspaces`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{canvas, container, mouse_area, row, stack};
use iced::{event, keyboard};
use iced::{Element, Event, Length, Subscription, Task, Theme};

use sola_bus::topics::{AppNotification, SplitDir, Topic, TopicKind};
use sola_bus::Message;
use sola_kit::app::{
    apply_theme_update, bus, bus_subscription, is_self_quit, startup, window_settings_transparent,
    BusSetup,
};
use sola_kit::fonts;
use sola_kit::theme::{atoms_from_bus_theme, default_theme, Atoms};
use sola_terminal::emulator::{self, Emulator, Listener};
use sola_terminal::input::{self, Mods};
use sola_terminal::pty::PtyBackend;
use sola_terminal::state::{self as term_state, PaneRuntime};
use sola_terminal::term_view::{self, CellMetrics, Palette};
use sola_terminal::{extkeys, links, tmux};

mod calls;
mod cli;
mod hooks;
mod menu;
mod paths;
mod presence;
mod sidebar;
mod spawn;
mod startup;
mod status;
mod workspace;

const APP_ID: &str = "sola-workspaces";
const WINDOW_TITLE: &str = "Workspaces";

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
        ])
        .calls(calls::OWNER, calls::methods())
        .install();

    if let Ok(mut client) = bus().lock() {
        if let Err(e) = client.emit(Topic::SetAppMenu(menu::app_menu())) {
            tracing::warn!("app-menu publish failed: {e:?}");
        }
    }

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .window(window_settings_transparent(APP_ID))
        .run()
}

struct App {
    projects: Vec<workspace::Project>,
    workspaces: Vec<workspace::Workspace>,
    selected: String,
    focused: String,
    runtimes: HashMap<String, PaneRuntime>,
    dragging_split: Option<String>,
    /// Last applied grid per pane. Skip TIOCSWINSZ when a divider drag
    /// has not changed cols/rows (same as sola-terminal).
    pane_grids: HashMap<String, (u16, u16)>,
    theme: Theme,
    palette: Palette,
    sidebar: sidebar::SidebarState,
    window_size: iced::Size,
    metrics: CellMetrics,
    cursor_on: bool,
    keyboard_mods: keyboard::Modifiers,
    keys_held_mods: keyboard::Modifiers,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    pane_status: HashMap<String, status::PaneStatus>,
    hook_sock: String,
    /// Previous pane id if we renamed a *path-matched* orphan onto `ws-main`.
    adopted_from: Option<String>,
    spawn: sidebar::SpawnDraft,
    add: sidebar::AddDraft,
    startup: sidebar::StartupDraft,
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
    Resized(iced::Size),
    SelectionChanged,
    Scrolled(String),
    OpenUrl(String),
    WheelToPty(String, Vec<u8>),
    Pasted(Option<String>),
    BlinkTick,
    Sidebar(sola_kit::components::SidebarMsg),
    CursorMoved(f32, f32),
    CursorReleased,
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    SelectWorkspace(String),
    ToggleProject(String),
    OpenSpawn(String),
    OpenAdd,
    StartupAction(iced::widget::text_editor::Action),
    SaveStartup,
    DismissDialog,
    SpawnName(String),
    Spawn,
    AddPath(String),
    AddProject,
    CloseWorkspace(String),
    /// Close the tab and `git worktree remove` the checkout (`--worktree`).
    RmCheckout {
        id: String,
        force: bool,
    },
    DropProject(String),
    RestartShell(String),
    PaneFocused(String),
    SplitDividerPress(String),
    Ignore,
    Hook(hooks::Incoming),
    Osc(String, sola_terminal::osc9999::OscStatus),
    PresenceTick,
    Call(sola_call::Incoming),
    WindowFocus(bool),
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let hook_paths = hooks::start();
        let _ = sola_terminal::osc9999::sender();
        let mut catalog = workspace::load();
        if catalog.projects.is_empty() {
            if let Some((project, ws)) = workspace::seed_from_cwd() {
                catalog.selected = Some(ws.id.clone());
                catalog.projects.push(project);
                catalog.workspaces.push(ws);
                workspace::save(&catalog);
            }
        }
        let selected = catalog
            .selected
            .clone()
            .filter(|id| catalog.workspaces.iter().any(|w| w.id == *id))
            .or_else(|| catalog.workspaces.first().map(|w| w.id.clone()))
            .unwrap_or_default();
        let adopted_from = catalog
            .workspaces
            .iter()
            .find(|w| w.id == workspace::LIVE_ID)
            .and_then(|w| {
                let claimed: HashSet<String> =
                    catalog.workspaces.iter().map(|x| x.id.clone()).collect();
                workspace::adopt_orphan_session(&w.path, &claimed)
            });
        let mut pane_status = HashMap::new();
        for ws in &catalog.workspaces {
            for pane_id in ws.layout().leaves() {
                if let Some(st) = status::hydrate(&pane_id) {
                    pane_status.insert(pane_id, st);
                }
            }
        }
        let focused = catalog
            .workspaces
            .iter()
            .find(|w| w.id == selected)
            .map(|w| w.active_pane_id())
            .unwrap_or_else(|| selected.clone());
        let mut app = Self {
            projects: catalog.projects,
            workspaces: catalog.workspaces,
            selected: selected.clone(),
            focused,
            runtimes: HashMap::new(),
            dragging_split: None,
            pane_grids: HashMap::new(),
            theme: default_theme(),
            palette: Palette::from_kit_theme(&Atoms::default()),
            sidebar: sidebar::SidebarState::default(),
            window_size: iced::Size::new(1100.0, 720.0),
            metrics: CellMetrics::for_font(15.0, fonts::mono_metrics()),
            cursor_on: true,
            keyboard_mods: keyboard::Modifiers::empty(),
            keys_held_mods: keyboard::Modifiers::empty(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            pane_status,
            hook_sock: hook_paths.socket_path.to_string_lossy().into_owned(),
            adopted_from,
            spawn: sidebar::SpawnDraft::default(),
            add: sidebar::AddDraft::default(),
            startup: sidebar::StartupDraft::default(),
            window_focused: true,
            pending_waits: Vec::new(),
        };
        app.sync_all_rows();
        let reap = app.reap_missing_worktrees();
        let attach = if app.selected.is_empty() {
            Task::none()
        } else {
            let id = app.selected.clone();
            app.attach_workspace(&id)
        };
        (
            app,
            Task::batch([sola_kit::window_ready_task(Msg::WindowReady), reap, attach]),
        )
    }

    fn title(&self) -> String {
        WINDOW_TITLE.into()
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
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
            event::listen_with(|ev, status, _| match &ev {
                Event::Window(iced::window::Event::Focused) => Some(Msg::WindowFocus(true)),
                Event::Window(iced::window::Event::Unfocused) => Some(Msg::WindowFocus(false)),
                Event::Keyboard(_) => Some(Msg::Input(ev.clone())),
                // Mouse is handled by TermView / sidebar / the split-drag
                // sub below. Mapping ignored motion to Input presented the
                // whole window on every pixel (`on_input` ignores mouse).
                Event::Mouse(_) => None,
                _ if matches!(status, iced::event::Status::Ignored) => Some(Msg::Input(ev.clone())),
                _ => None,
            }),
            iced::window::resize_events().map(|(_id, size)| Msg::Resized(size)),
            iced::time::every(Duration::from_millis(530)).map(|_| Msg::BlinkTick),
            self.sidebar.gestures.subscription().map(Msg::Sidebar),
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
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::WindowReady(id) => {
                self.window_id = id;
                Task::none()
            }
            Msg::TitleDrag => sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                sola_kit::close_app(APP_ID);
                Task::none()
            }
            Msg::Ignore => Task::none(),
            Msg::WindowFocus(on) => {
                self.window_focused = on;
                if !on {
                    // Super release is often eaten by River while we are
                    // unfocused (switcher, another window). Drop the latch
                    // so the next focus does not treat every key as ⌘.
                    self.keyboard_mods = keyboard::Modifiers::empty();
                    self.keys_held_mods = keyboard::Modifiers::empty();
                }
                Task::none()
            }
            Msg::Call(incoming) => self.on_call(incoming),
            Msg::Hook(incoming) => {
                let Some(id) = self.resolve_pane(&incoming.pane_id) else {
                    tracing::debug!(
                        pane = %incoming.pane_id,
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
                let cwd = self.workspace_for_pane(&id).map(|w| w.path.clone());
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
                    .workspaces
                    .iter()
                    .flat_map(|w| w.layout().leaves())
                    .collect();
                let mut dirty = false;
                for id in ids {
                    let tmux_session = tmux::session_name(&id);
                    let who = presence::scan_session(&tmux_session);
                    let cwd = self.workspace_for_pane(&id).map(|w| w.path.clone());
                    let st = self.pane_status.entry(id).or_default();
                    let before = (st.status, st.agent.clone());
                    st.apply_presence(who);
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
            Msg::SelectWorkspace(id) => {
                if let Some(ws) = self.workspaces.iter().find(|w| w.id == id) {
                    self.selected = id.clone();
                    self.focused = ws.active_pane_id();
                    self.persist_catalog();
                    return self.attach_workspace(&id);
                }
                Task::none()
            }
            Msg::PaneFocused(pane_id) => {
                // Hover focuses for typing. It must not spawn a shell —
                // only the Start new shell button (or a sidebar click)
                // attaches a missing PTY.
                if let Some(ws) = self.workspaces.iter().find(|w| w.owns_pane(&pane_id)) {
                    let ws_id = ws.id.clone();
                    self.set_focus(&ws_id, &pane_id);
                }
                Task::none()
            }
            Msg::SplitDividerPress(id) => {
                self.dragging_split = Some(id);
                Task::none()
            }
            Msg::ToggleProject(id) => {
                if let Some(p) = self.projects.iter_mut().find(|p| p.id == id) {
                    p.collapsed = !p.collapsed;
                    self.persist_catalog();
                }
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
                self.spawn = sidebar::SpawnDraft::default();
                self.add = sidebar::AddDraft::default();
                self.startup = sidebar::StartupDraft::default();
                Task::none()
            }
            Msg::SpawnName(s) => {
                self.spawn.name = s;
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
            Msg::CloseWorkspace(id) => self.close_workspace(&id, false, false),
            Msg::RmCheckout { id, force } => self.close_workspace(&id, true, force),
            Msg::DropProject(id) => self.drop_project(&id),
            Msg::RestartShell(id) => self.attach_pane(&id, &[]),
            Msg::Sidebar(m) => {
                if let Some(sola_kit::components::SidebarEvent::Resize { width }) =
                    self.sidebar.gestures.update(m)
                {
                    self.sidebar.width = width;
                    self.resize_all_panes();
                }
                Task::none()
            }
            Msg::PtyOutput(id) => {
                if let Some(rt) = self.runtimes.get(&id) {
                    rt.cache.clear();
                }
                Task::none()
            }
            Msg::PtyExit(id) => {
                tracing::info!(pane = %id, "pane PTY exited");
                // Session is already gone — drop the client, do not
                // `close()` (that would try to kill a dead tmux session).
                self.runtimes.remove(&id);
                if let Some(st) = self.pane_status.get_mut(&id) {
                    st.status = status::AgentStatus::Idle;
                    st.agent = None;
                    st.tool = None;
                    st.owner_session = None;
                }
                // A split leaf that dies retracts. Start new shell only
                // on the last remaining pane (`close_pane` handles that).
                if self.workspace_for_pane(&id).is_some() {
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
            Msg::Resized(size) => {
                self.window_size = size;
                self.resize_all_panes();
                Task::none()
            }
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

    fn view(&self) -> Element<'_, Msg> {
        let pane: Element<'_, Msg> = if self.workspaces.is_empty() {
            sidebar::empty_pane()
        } else if let Some(ws) = self.workspaces.iter().find(|w| w.id == self.selected) {
            self.render_node(&ws.layout().to_node())
        } else {
            sidebar::empty_pane()
        };

        let rail_pane: Element<'_, Msg> = row![
            sidebar::view(
                &self.sidebar,
                &self.projects,
                &self.workspaces,
                &self.selected,
                &self.pane_status,
                &self.theme,
                self.palette.bg,
            ),
            pane,
        ]
        .into();

        let body: Element<'_, Msg> =
            match sidebar::overlay(&self.spawn, &self.add, &self.startup, &self.projects) {
                Some(veil) => stack![rail_pane, veil].into(),
                None => rail_pane,
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
            self.float.is_floating_any(),
            WINDOW_TITLE,
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            framed.into(),
        )
    }

    fn on_bus(&mut self, m: &Message) -> Task<Msg> {
        self.float.update(m);
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
                            .workspaces
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
                    "drop-workspace" => {
                        let pid = self
                            .workspaces
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

    fn pane_size(&self) -> iced::Size {
        let chrome = self.sidebar.width + sola_kit::components::DIVIDER_HIT_PX;
        iced::Size::new(
            (self.window_size.width - chrome).max(0.0),
            self.window_size.height,
        )
    }

    fn cols_rows(&self) -> (u16, u16) {
        let (c, r) = term_view::cols_rows_for(self.pane_size(), self.metrics);
        (c.max(2), r.max(1))
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
            None if self.is_sole_leaf(pane_id) => sidebar::exited_pane(pane_id),
            None => sidebar::empty_pane(),
        };
        mouse_area(inner)
            .on_enter(Msg::PaneFocused(pane_id.to_string()))
            .into()
    }

    fn attach_workspace(&mut self, workspace_id: &str) -> Task<Msg> {
        let leaves = self
            .workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .map(|w| w.layout().leaves())
            .unwrap_or_default();
        let mut tasks = Vec::new();
        for id in leaves {
            tasks.push(self.attach_pane(&id, &[]));
        }
        Task::batch(tasks)
    }

    fn is_sole_leaf(&self, pane_id: &str) -> bool {
        self.workspace_for_pane(pane_id)
            .map(|w| {
                let leaves = w.layout().leaves();
                leaves.len() <= 1 && leaves.first().map(String::as_str) == Some(pane_id)
            })
            .unwrap_or(true)
    }

    /// Record focus. Does not attach a PTY (hover must not spawn).
    fn set_focus(&mut self, workspace_id: &str, pane_id: &str) -> bool {
        let Some(ws) = self.workspaces.iter().find(|w| w.id == workspace_id) else {
            return false;
        };
        if !ws.owns_pane(pane_id) {
            return false;
        }
        let same = self.selected == workspace_id && self.focused == pane_id;
        self.selected = workspace_id.to_string();
        self.focused = pane_id.to_string();
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
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
            .workspaces
            .iter()
            .find(|w| w.owns_pane(id) || w.id == id)
        else {
            return Task::none();
        };
        let (cols, rows) = self.cols_rows();
        let cols = if cols == 0 { DEFAULT_COLS } else { cols };
        let rows = if rows == 0 { DEFAULT_ROWS } else { rows };
        let tmux_session = workspace::bind_session(id, &ws.path);
        if tmux_session.is_empty() {
            tracing::error!(pane = %id, "refusing attach; leftover tmux is another checkout");
            return Task::none();
        }
        let cwd = ws.path.to_string_lossy().into_owned();

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
            (workspace::SOLA_WS_PATH, cwd.as_str()),
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
            exec,
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

    fn resolve_pane(&self, id: &str) -> Option<String> {
        if self
            .workspaces
            .iter()
            .any(|w| w.owns_pane(id) || w.id == id)
        {
            return Some(id.to_string());
        }
        if self.adopted_from.as_deref() == Some(id)
            && self.workspaces.iter().any(|w| w.id == workspace::LIVE_ID)
        {
            return Some(workspace::LIVE_ID.into());
        }
        None
    }

    fn workspace_for_pane(&self, pane_id: &str) -> Option<&workspace::Workspace> {
        self.workspaces
            .iter()
            .find(|w| w.owns_pane(pane_id) || w.id == pane_id)
    }

    fn sync_row(&mut self, pane_id: &str) {
        let Some(ws_id) = self.workspace_for_pane(pane_id).map(|w| w.id.clone()) else {
            return;
        };
        let leaves = self
            .workspaces
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
                status::rollup_grok(panes.iter().copied()),
                panes
                    .iter()
                    .find(|s| s.is_grok())
                    .and_then(|s| s.agent.clone()),
            )
        };
        if let Some(ws) = workspace::find_workspace_mut(&mut self.workspaces, &ws_id) {
            ws.status = rolled;
            ws.agent = agent;
        }
    }

    fn sync_all_rows(&mut self) {
        let ids: Vec<String> = self.workspaces.iter().map(|w| w.id.clone()).collect();
        for id in ids {
            if let Some(first) = self
                .workspaces
                .iter()
                .find(|w| w.id == id)
                .and_then(|w| w.layout().leaves().into_iter().next())
            {
                self.sync_row(&first);
            }
        }
    }

    fn persist_catalog(&self) {
        workspace::save(&workspace::Catalog {
            version: 1,
            selected: if self.selected.is_empty() {
                None
            } else {
                Some(self.selected.clone())
            },
            projects: self.projects.clone(),
            workspaces: self.workspaces.clone(),
        });
    }

    fn open_spawn(&mut self, project_id: &str) -> Task<Msg> {
        if !self.projects.iter().any(|p| p.id == project_id) {
            return Task::none();
        }
        self.add = sidebar::AddDraft::default();
        self.startup = sidebar::StartupDraft::default();
        self.spawn = sidebar::SpawnDraft::open(project_id);
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(sidebar::SPAWN_INPUT_ID))
    }

    fn open_add(&mut self) -> Task<Msg> {
        self.spawn = sidebar::SpawnDraft::default();
        self.startup = sidebar::StartupDraft::default();
        self.add = sidebar::AddDraft {
            open: true,
            path: String::new(),
            error: None,
        };
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(sidebar::ADD_INPUT_ID))
    }

    fn selected_project_id(&self) -> Option<String> {
        self.workspaces
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
        self.spawn = sidebar::SpawnDraft::default();
        self.add = sidebar::AddDraft::default();
        self.startup = sidebar::StartupDraft::open(pid, &script);
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
        self.startup = sidebar::StartupDraft::default();
        Task::none()
    }

    fn spawn_sibling(&mut self) -> Task<Msg> {
        let Some(project_id) = self.spawn.project_id.clone() else {
            return Task::none();
        };
        let name = self.spawn.name.clone();
        match self.spawn_workspace(&project_id, &name, None, None, None, None, None, true) {
            Ok((id, startup_err)) => {
                self.spawn = sidebar::SpawnDraft::default();
                self.maybe_toast_startup(startup_err, &id);
                self.attach_pane(&id, &[])
            }
            Err(e) => {
                self.spawn.error = Some(e);
                Task::none()
            }
        }
    }

    /// Create a worktree + catalog row. Does not attach a PTY.
    /// Second value is a startup-script error (workspace still exists).
    /// `select` takes the rail (UI / `--select`). CLI default is false.
    fn spawn_workspace(
        &mut self,
        project_q: &str,
        name: &str,
        parent: Option<String>,
        agent: Option<&str>,
        branch: Option<&str>,
        base: Option<&str>,
        title: Option<&str>,
        select: bool,
    ) -> Result<(String, Option<String>), String> {
        let project = workspace::resolve_project(&self.projects, project_q)?.clone();
        let slug = spawn::slug(name);
        if slug.is_empty() {
            return Err("name needs a letter or number".into());
        }
        if let Some(a) = agent {
            if a != "grok" {
                return Err("only grok is first-class; other agents are presence-only".into());
            }
        }
        let dest = spawn::add_worktree_at(&project.root, &slug, branch, base)?;
        let taken: HashSet<String> = self.workspaces.iter().map(|w| w.id.clone()).collect();
        let id = workspace::unique_id("ws", &slug, &taken);
        let parent = match parent {
            Some(p) => Some(
                workspace::resolve_workspace(&self.workspaces, &p)?
                    .id
                    .clone(),
            ),
            None => self
                .workspaces
                .iter()
                .find(|w| w.project_id == project.id && w.kind == workspace::Kind::Main)
                .map(|w| w.id.clone()),
        };
        let title = title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ws = workspace::Workspace {
            id: id.clone(),
            project_id: project.id.clone(),
            name: name.trim().to_string(),
            title,
            path: dest,
            kind: workspace::Kind::Worktree,
            parent,
            layout: None,
            active_pane: None,
            status: status::AgentStatus::Idle,
            agent: agent.map(str::to_string),
        };
        let startup_err = startup::run(&project, &ws).err();
        if let Some(e) = &startup_err {
            tracing::warn!(workspace = %ws.id, "{e}");
        }
        self.workspaces.push(ws);
        workspace::apply_spawn_focus(&mut self.selected, &mut self.focused, &id, select);
        self.persist_catalog();
        Ok((id, startup_err))
    }

    fn toast_place(&self, pane_id: &str) -> (Option<String>, String) {
        let Some(ws) = self.workspace_for_pane(pane_id) else {
            return (None, pane_id.to_string());
        };
        let project =
            workspace::find_project(&self.projects, &ws.project_id).map(|p| p.name.clone());
        (project, cli::rail_label(ws))
    }

    fn emit_notice(&self, title: String, body: String, tag: String) {
        if let Ok(mut client) = bus().lock() {
            let _ = client.emit(Topic::AppNotification(AppNotification {
                id: format!("ws-{tag}-{}", now_millis()),
                app_id: APP_ID.into(),
                source: "Workspaces".into(),
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
            "workspace.list" => (
                self.cli_workspace_list(param_str(params, "project").as_deref()),
                Task::none(),
            ),
            "workspace.spawn" => {
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
                    params
                        .get("select")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                ) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "workspace.rm" => {
                let Some(ws) = param_str(params, "workspace") else {
                    return (Err("missing workspace".into()), Task::none());
                };
                let checkout = param_bool(params, "worktree");
                let force = param_bool(params, "force");
                if force && !checkout {
                    return (Err("--force needs --worktree".into()), Task::none());
                }
                match self.cli_rm(&ws) {
                    // Reply first; teardown on the next tick so solactl
                    // (maybe running in a pane we are about to kill)
                    // gets {ok:true} instead of a hang / timeout.
                    Ok(id) => (
                        Ok(serde_json::json!({ "ok": true })),
                        if checkout {
                            Task::done(Msg::RmCheckout { id, force })
                        } else {
                            Task::done(Msg::CloseWorkspace(id))
                        },
                    ),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "workspace.select" => {
                let Some(q) = param_str(params, "workspace") else {
                    return (Err("missing workspace".into()), Task::none());
                };
                match self.cli_select(&q) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "workspace.set" => {
                let Some(q) = param_str(params, "workspace") else {
                    return (Err("missing workspace".into()), Task::none());
                };
                (
                    self.cli_set(
                        &q,
                        params.get("name").and_then(|v| v.as_str()),
                        params.get("title").and_then(|v| v.as_str()),
                        params.get("branch").and_then(|v| v.as_str()),
                    ),
                    Task::none(),
                )
            }
            "workspace.exec" => {
                let Some(q) = param_str(params, "workspace") else {
                    return (Err("missing workspace".into()), Task::none());
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
                self.cli_pane_list(param_str(params, "workspace").as_deref()),
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
                let workspaces: Vec<serde_json::Value> =
                    workspace::ordered_for_project(&p.id, &self.workspaces)
                        .into_iter()
                        .map(|w| cli::workspace_json(w, Some(&self.selected)))
                        .collect();
                let mut row = cli::project_json(p);
                row["workspaces"] = serde_json::Value::Array(workspaces);
                row
            })
            .collect();
        serde_json::json!({ "projects": projects, "selected": self.selected })
    }

    fn cli_workspace_list(&self, project: Option<&str>) -> Result<serde_json::Value, String> {
        let filter = match project {
            Some(q) => Some(workspace::resolve_project(&self.projects, q)?.id.clone()),
            None => None,
        };
        let list: Vec<serde_json::Value> = self
            .workspaces
            .iter()
            .filter(|w| filter.as_ref().is_none_or(|id| w.project_id == *id))
            .map(|w| cli::workspace_json(w, None))
            .collect();
        Ok(serde_json::json!({ "workspaces": list }))
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
        select: bool,
    ) -> Result<(serde_json::Value, Task<Msg>), String> {
        let prompt = cli::read_prompt(prompt, prompt_file)?;
        let agent = match (cli::only_grok(agent)?, prompt.as_deref()) {
            (Some(a), _) => Some(a),
            (None, Some(_)) => Some("grok"),
            (None, None) => None,
        };
        let (id, startup_err) =
            self.spawn_workspace(project, name, parent, agent, branch, base, title, select)?;
        let task = if agent == Some("grok") {
            let args = cli::grok_argv(prompt.as_deref());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.attach_pane(&id, &refs)
        } else {
            self.attach_pane(&id, &[])
        };
        let mut data = self
            .workspaces
            .iter()
            .find(|w| w.id == id)
            .map(|w| cli::spawn_json(w, select))
            .unwrap_or_else(|| serde_json::json!({ "id": id, "selected": select }));
        if let Some(e) = startup_err {
            data["startup_error"] = serde_json::json!(e);
        }
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
        let id = workspace::resolve_project(&self.projects, &q)?.id.clone();
        if set {
            if let Some(p) = self.projects.iter_mut().find(|p| p.id == id) {
                p.startup = script.unwrap_or("").to_string();
            }
            self.persist_catalog();
        }
        let p = workspace::resolve_project(&self.projects, &id)?;
        Ok(serde_json::json!({
            "project": p.id,
            "name": p.name,
            "script": p.startup,
        }))
    }

    fn cli_rm(&self, q: &str) -> Result<String, String> {
        let ws = workspace::resolve_workspace(&self.workspaces, q)?;
        if !workspace::can_close(ws) {
            return Err("cannot close the project root".into());
        }
        Ok(ws.id.clone())
    }

    fn cli_rm_project(&self, q: &str) -> Result<String, String> {
        Ok(workspace::resolve_project(&self.projects, q)?.id.clone())
    }

    fn cli_select(&mut self, q: &str) -> Result<(serde_json::Value, Task<Msg>), String> {
        let id = workspace::resolve_workspace(&self.workspaces, q)?
            .id
            .clone();
        let focused = self
            .workspaces
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.active_pane_id())
            .unwrap_or_else(|| id.clone());
        self.selected = id.clone();
        self.focused = focused;
        self.persist_catalog();
        let task = self.attach_workspace(&id);
        Ok((serde_json::json!({ "id": id, "selected": true }), task))
    }

    fn cli_set(
        &mut self,
        q: &str,
        name: Option<&str>,
        title: Option<&str>,
        branch: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let id = workspace::resolve_workspace(&self.workspaces, q)?
            .id
            .clone();
        let mut err = None;
        if let Some(raw) = name {
            if let Err(e) = self.rename_workspace(&id, raw) {
                err = Some(e);
            }
        }
        if err.is_none() {
            if let Some(raw) = branch {
                if let Err(e) = self.rename_workspace_branch(&id, raw) {
                    err = Some(e);
                }
            }
        }
        if let Some(raw) = title {
            if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == id) {
                let t = raw.trim();
                ws.title = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
            }
        }
        self.persist_catalog();
        if let Some(e) = err {
            return Err(e);
        }
        let ws = workspace::resolve_workspace(&self.workspaces, &id)?;
        Ok(cli::workspace_json(ws, Some(&self.selected)))
    }

    /// Rail slug + `git worktree move` to `.worktrees/<slug>`. Id stays.
    fn rename_workspace(&mut self, id: &str, raw: &str) -> Result<(), String> {
        let slug = spawn::slug(raw);
        spawn::check_slug(&slug)?;
        let ws = workspace::resolve_workspace(&self.workspaces, id)?;
        if ws.kind != workspace::Kind::Worktree {
            return Err("cannot rename the project root".into());
        }
        let project_id = ws.project_id.clone();
        let from = ws.path.clone();
        if workspace::worktree_name_taken(&self.workspaces, &project_id, &slug, id) {
            return Err(format!("workspace '{slug}' already exists"));
        }
        let root = workspace::resolve_project(&self.projects, &project_id)?
            .root
            .clone();
        let dest = spawn::worktree_path(&root, &slug);
        let path_changed = !workspace::path_same(&from, &dest);
        if path_changed {
            spawn::move_worktree(&root, &from, &slug)?;
        }
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == id) {
            ws.name = slug;
            ws.path = dest;
        } else {
            return Err(format!("unknown workspace '{id}'"));
        }
        if path_changed {
            self.restamp_workspace_path(id);
        }
        Ok(())
    }

    fn rename_workspace_branch(&self, id: &str, raw: &str) -> Result<(), String> {
        let ws = workspace::resolve_workspace(&self.workspaces, id)?;
        if ws.kind != workspace::Kind::Worktree {
            return Err("cannot rename the project root branch".into());
        }
        spawn::rename_branch(&ws.path, raw)
    }

    fn restamp_workspace_path(&self, id: &str) {
        let Some(ws) = self.workspaces.iter().find(|w| w.id == id) else {
            return;
        };
        let cwd = ws.path.to_string_lossy();
        for pane in ws.layout().leaves() {
            let session = tmux::session_name(&pane);
            if tmux::has_session(&session) {
                tmux::set_environment(&session, workspace::SOLA_WS_PATH, cwd.as_ref());
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
        let agent = cli::only_grok(agent)?.unwrap_or("grok");
        if agent != "grok" {
            return Err("only grok is first-class; other agents are presence-only".into());
        }
        let prompt = cli::read_prompt(prompt, prompt_file)?;
        let ws_id = workspace::resolve_workspace(&self.workspaces, q)?
            .id
            .clone();
        let pane = self.preferred_pane(Some(&ws_id))?;
        let is_grok = self
            .pane_status
            .get(&pane)
            .and_then(|s| s.agent.as_deref())
            .is_some_and(|a| a.eq_ignore_ascii_case("grok"));
        if is_grok {
            if let Some(text) = prompt.as_deref() {
                self.write_pane(&pane, text, true)?;
            }
            return Ok((
                serde_json::json!({
                    "workspace": ws_id,
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
            let args = cli::grok_argv(prompt.as_deref());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.attach_pane(&pane, &refs)
        } else {
            let attach = self.attach_pane(&pane, &[]);
            let line = cli::grok_shell_line(prompt.as_deref());
            self.write_pane(&pane, &line, true)?;
            attach
        };
        Ok((
            serde_json::json!({
                "workspace": ws_id,
                "pane": pane,
                "started": true,
                "sent": false,
            }),
            task,
        ))
    }

    fn cli_add_project(&mut self, path: &str) -> Result<(serde_json::Value, Task<Msg>), String> {
        let (project_id, main_id, task) = self.register_project(path)?;
        let project = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| "project vanished".to_string())?;
        let mut data = cli::project_json(project);
        data["workspace"] = serde_json::json!(main_id);
        Ok((data, task))
    }

    fn cli_pane_list(&self, workspace: Option<&str>) -> Result<serde_json::Value, String> {
        let id = match workspace {
            Some(q) => workspace::resolve_workspace(&self.workspaces, q)?
                .id
                .clone(),
            None => self.selected.clone(),
        };
        if id.is_empty() {
            return Err("no workspace selected".into());
        }
        let leaves = self
            .workspaces
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
        if let Some(q) = hint {
            if self.workspaces.iter().any(|w| w.owns_pane(q) && w.id != q) {
                return Ok(q.to_string());
            }
            let ws = workspace::resolve_workspace(&self.workspaces, q)?;
            let leaves = ws.layout().leaves();
            let agents = self.pane_agents(&leaves);
            return Ok(cli::prefer_grok_pane(
                &leaves,
                &agents,
                &ws.active_pane_id(),
                None,
            ));
        }
        if !self.focused.is_empty() {
            return Ok(self.focused.clone());
        }
        if !self.selected.is_empty() {
            return self.preferred_pane(Some(&self.selected));
        }
        Err("no pane".into())
    }

    fn cli_pane_id(&self, pane: Option<&str>) -> Result<String, String> {
        self.preferred_pane(pane)
    }

    fn write_pane(&self, id: &str, text: &str, enter: bool) -> Result<(), String> {
        let session = tmux::session_name(id);
        if !tmux::send_prompt(&session, text, enter) {
            return Err("send failed".into());
        }
        Ok(())
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
            workspace::resolve_workspace(&self.workspaces, q)?
        } else if let Some(p) = path {
            workspace::resolve_workspace(&self.workspaces, p)?
        } else {
            return Err(
                "not in a workspaces pane (pass --pane/--path or run from a Workspaces PTY)".into(),
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
        let project = workspace::resolve_project(&self.projects, &ws.project_id).ok();
        Ok(serde_json::json!({
            "pane": pane_id,
            "workspace": ws.id,
            "workspace_name": cli::display_name(ws),
            "project": ws.project_id,
            "project_name": project.map(|p| p.name.as_str()),
            "path": ws.path,
            "kind": cli::kind_str(ws.kind),
            "status": cli::status_str(st.map(|s| s.status).unwrap_or(ws.status)),
            "agent": st.and_then(|s| s.agent.clone()).or_else(|| ws.agent.clone()),
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
            Ok((_, _, task)) => {
                self.add = sidebar::AddDraft::default();
                task
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
        let root = match workspace::expand_user_path(raw).canonicalize() {
            Ok(p) if p.is_dir() => p,
            Ok(_) => return Err("not a folder".into()),
            Err(e) => return Err(format!("path: {e}")),
        };
        if self.projects.iter().any(|p| p.root == root) {
            return Err("already in the rail".into());
        }
        let slug = spawn::slug(
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("project"),
        );
        let taken_p: HashSet<String> = self.projects.iter().map(|p| p.id.clone()).collect();
        let project_id = workspace::unique_id("proj", &slug, &taken_p);
        let mut taken_w: HashSet<String> = self.workspaces.iter().map(|w| w.id.clone()).collect();
        let main_id = workspace::main_workspace_id(&root, &taken_w);
        taken_w.insert(main_id.clone());
        let (project, ws) = workspace::project_from_root(&root, &project_id, &main_id);
        self.projects.push(project);
        if self.selected.is_empty() {
            self.selected = ws.id.clone();
            self.focused = ws.active_pane_id();
        }
        let attach_id = ws.id.clone();
        self.workspaces.push(ws);
        self.persist_catalog();
        let task = if self.selected == attach_id {
            self.attach_workspace(&attach_id)
        } else {
            Task::none()
        };
        Ok((project_id, attach_id, task))
    }

    fn teardown_pane(&mut self, id: &str) {
        if let Some(rt) = self.runtimes.remove(id) {
            rt.backend.close();
        } else {
            tmux::kill_session(&tmux::session_name(id));
        }
        self.pane_status.remove(id);
        self.pane_grids.remove(id);
    }

    fn attach_selected_if_needed(&mut self) -> Task<Msg> {
        if self.selected.is_empty() {
            self.focused.clear();
            return Task::none();
        }
        if let Some(ws) = self.workspaces.iter().find(|w| w.id == self.selected) {
            self.focused = ws.active_pane_id();
        }
        let next = self.selected.clone();
        self.attach_workspace(&next)
    }

    /// Close a sibling tab. `checkout` also `git worktree remove`s (after
    /// tmux dies so the pane is not sitting in that cwd). Hover × leaves
    /// the folder. A gone path just prunes git metadata.
    fn close_workspace(&mut self, id: &str, checkout: bool, force: bool) -> Task<Msg> {
        let Some((panes, path, project_id)) =
            self.workspaces.iter().find(|w| w.id == id).and_then(|ws| {
                workspace::can_close(ws)
                    .then(|| (ws.layout().leaves(), ws.path.clone(), ws.project_id.clone()))
            })
        else {
            return Task::none();
        };
        tracing::info!(workspace = %id, checkout, "closing workspace");
        for pane in &panes {
            self.teardown_pane(pane);
        }
        self.workspaces.retain(|w| w.id != id);
        for w in &mut self.workspaces {
            if w.parent.as_deref() == Some(id) {
                w.parent = None;
            }
        }
        if self.selected == id {
            self.selected = self
                .workspaces
                .first()
                .map(|w| w.id.clone())
                .unwrap_or_default();
        }
        self.persist_catalog();
        status::persist_all(&self.pane_status);
        if checkout {
            if let Some(root) = self
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .map(|p| p.root.clone())
            {
                if let Err(e) = spawn::remove_worktree(&root, &path, force) {
                    tracing::warn!(workspace = %id, %e, "git worktree remove failed");
                    self.emit_notice("Worktree leftover".into(), e, format!("git-rm-{id}"));
                }
            }
        }
        self.attach_selected_if_needed()
    }

    /// If a sibling's checkout is already gone (`git worktree remove` from
    /// inside the pane, or a deleted folder), drop the tab. The pane cannot
    /// call `workspace.rm` after its cwd vanishes — the rail used to keep
    /// a working spinner forever.
    fn reap_missing_worktrees(&mut self) -> Task<Msg> {
        let gone = workspace::missing_worktree_ids(&self.workspaces);
        if gone.is_empty() {
            return Task::none();
        }
        let mut tasks = Vec::with_capacity(gone.len());
        for id in gone {
            tracing::info!(workspace = %id, "worktree path gone; closing tab");
            tasks.push(self.close_workspace(&id, true, false));
        }
        Task::batch(tasks)
    }

    /// Unregister the project and every workspace under it. Kills those
    /// tmux sessions. Leaves git worktrees and folders on disk.
    fn drop_project(&mut self, project_id: &str) -> Task<Msg> {
        let pane_ids: Vec<String> = self
            .workspaces
            .iter()
            .filter(|w| w.project_id == project_id)
            .flat_map(|w| w.layout().leaves())
            .collect();
        let mut catalog = workspace::Catalog {
            version: 1,
            selected: if self.selected.is_empty() {
                None
            } else {
                Some(self.selected.clone())
            },
            projects: std::mem::take(&mut self.projects),
            workspaces: std::mem::take(&mut self.workspaces),
        };
        let removed = workspace::unregister_project(&mut catalog, project_id);
        self.projects = catalog.projects;
        self.workspaces = catalog.workspaces;
        self.selected = catalog.selected.unwrap_or_default();
        for id in pane_ids {
            self.teardown_pane(&id);
        }
        if removed.is_empty() {
            return Task::none();
        }
        self.persist_catalog();
        status::persist_all(&self.pane_status);
        self.attach_selected_if_needed()
    }

    fn split_focused(&mut self, dir: SplitDir) -> Task<Msg> {
        let Some(ws_idx) = self.workspaces.iter().position(|w| w.id == self.selected) else {
            return Task::none();
        };
        let source = self.workspaces[ws_idx].active_pane_id();
        if !self.workspaces[ws_idx].owns_pane(&source) && source != self.workspaces[ws_idx].id {
            return Task::none();
        }
        let mut taken: HashSet<String> = self
            .workspaces
            .iter()
            .flat_map(|w| w.layout().leaves())
            .collect();
        for w in &self.workspaces {
            taken.insert(w.id.clone());
            taken.extend(w.layout().split_ids());
        }
        let ws_id = self.workspaces[ws_idx].id.clone();
        let new_pane = workspace::unique_id(&ws_id, "p", &taken);
        let split_id = workspace::unique_id("split", &ws_id, &taken);
        let mut node = self.workspaces[ws_idx].layout().to_node();
        if !term_state::split_leaf(&mut node, &source, &split_id, dir, &new_pane) {
            return Task::none();
        }
        self.workspaces[ws_idx].set_tree(node, new_pane.clone());
        self.focused = new_pane.clone();
        self.persist_catalog();
        self.attach_pane(&new_pane, &[])
    }

    fn close_pane(&mut self, pane_id: &str) -> Task<Msg> {
        let Some(ws_idx) = self.workspaces.iter().position(|w| w.owns_pane(pane_id)) else {
            return Task::none();
        };
        let ws_id = self.workspaces[ws_idx].id.clone();
        let node = self.workspaces[ws_idx].layout().to_node();
        let next_focus = term_state::sibling_first_leaf(&node, pane_id);
        match term_state::close_leaf(node, pane_id) {
            None => {
                // Last leaf: kill the shell, keep the workspace, reuse
                // the stable workspace id for the next Start new shell.
                self.teardown_pane(pane_id);
                self.workspaces[ws_idx]
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
                self.workspaces[ws_idx].set_tree(kept, focus.clone());
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
        let Some(ws) = self.workspaces.iter().find(|w| w.id == self.selected) else {
            return;
        };
        let node = ws.layout().to_node();
        let content = self.content_rect();
        let Some((_, area, dir)) = term_state::split_rects(&node, content)
            .into_iter()
            .find(|(id, _, _)| id == split_id)
        else {
            return;
        };
        let ratio = term_state::ratio_for_drag(area, dir, x, y, MIN_PANE_PX);
        let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == self.selected) else {
            return;
        };
        let mut node = ws.layout().to_node();
        if term_state::set_ratio(&mut node, split_id, ratio) {
            let active = ws.active_pane_id();
            ws.set_tree(node, active);
            self.resize_all_panes();
        }
    }

    fn content_rect(&self) -> term_state::Rect {
        let chrome = self.sidebar.width + sola_kit::components::DIVIDER_HIT_PX;
        let size = self.pane_size();
        term_state::Rect {
            x: chrome,
            y: 0.0,
            w: size.width,
            h: size.height,
        }
    }

    fn resize_all_panes(&mut self) {
        let Some(ws) = self.workspaces.iter().find(|w| w.id == self.selected) else {
            return;
        };
        let node = ws.layout().to_node();
        let content = self.content_rect();
        let targets: Vec<(String, u16, u16)> = term_state::pane_rects(&node, content)
            .into_iter()
            .map(|(id, rect)| {
                let (c, r) =
                    term_view::cols_rows_for(iced::Size::new(rect.w, rect.h), self.metrics);
                (id, c.max(2), r.max(1))
            })
            .collect();
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
            if self.dialog_open() {
                self.spawn = sidebar::SpawnDraft::default();
                self.add = sidebar::AddDraft::default();
                self.startup = sidebar::StartupDraft::default();
                return Task::none();
            }
        }
        if self.dialog_open() {
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

fn param_bool(params: &serde_json::Value, key: &str) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}
