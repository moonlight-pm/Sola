//! sola-agent-terminal — project / workspace rail + agent-aware PTYs.
//!
//! Persist + spawn: catalog on disk, siblings under `.worktrees/`.
//! Grok hooks, OSC 9999, process-tree. `sat` and toast-on-done later.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{canvas, container, row, stack};
use iced::{Element, Event, Length, Subscription, Task, Theme};
use iced::{event, keyboard};

use sola_agent_terminal::cli::{Request, Response};
use sola_bus::topics::{AppToast, Topic, TopicKind};
use sola_bus::Message;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup,
    window_settings_transparent,
};
use sola_kit::fonts;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};
use sola_terminal::emulator::{self, Emulator, Listener};
use sola_terminal::input::{self, Mods};
use sola_terminal::pty::PtyBackend;
use sola_terminal::state::PaneRuntime;
use sola_terminal::term_view::{self, CellMetrics, Palette};
use sola_terminal::{extkeys, links, tmux};

mod cli_server;
mod hooks;
mod menu;
mod presence;
mod sidebar;
mod spawn;
mod status;
mod workspace;

const APP_ID: &str = "sola-agent-terminal";
const WINDOW_TITLE: &str = "Workspaces";

const TMUX_SOCKET: &str = "sola-at";
const TMUX_UNIT: &str = "sola-at-tmux.service";
const TMUX_PREFIX: &str = "sat-";

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

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
    runtimes: HashMap<String, PaneRuntime>,
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
    /// Previous pane id if we renamed an orphan tmux session onto `ws-main`.
    adopted_from: Option<String>,
    spawn: sidebar::SpawnDraft,
    add: sidebar::AddDraft,
    drop_armed: Option<String>,
    window_focused: bool,
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
    #[allow(dead_code)]
    Pasted(Option<String>),
    BlinkTick,
    SidebarDragStart,
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
    DismissDialog,
    SpawnName(String),
    Spawn,
    AddPath(String),
    AddProject,
    CloseWorkspace(String),
    HoverSidebar(Option<String>),
    Ignore,
    StatusTick,
    Hook(hooks::Incoming),
    Osc(String, sola_terminal::osc9999::OscStatus),
    PresenceTick,
    Cli(cli_server::Incoming),
    WindowFocus(bool),
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let hook_paths = hooks::start();
        cli_server::start();
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
        let adopted_from = if catalog.workspaces.iter().any(|w| w.id == workspace::LIVE_ID)
        {
            workspace::adopt_orphan_session()
        } else {
            None
        };
        let mut pane_status = HashMap::new();
        for ws in &catalog.workspaces {
            if let Some(st) = status::hydrate(&ws.id) {
                pane_status.insert(ws.id.clone(), st);
            }
        }
        let mut app = Self {
            projects: catalog.projects,
            workspaces: catalog.workspaces,
            selected: selected.clone(),
            runtimes: HashMap::new(),
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
            drop_armed: None,
            window_focused: true,
        };
        app.sync_all_rows();
        let attach = if selected.is_empty() {
            Task::none()
        } else {
            app.attach_pane(&selected, &[])
        };
        (
            app,
            Task::batch([sola_kit::window_ready_task(Msg::WindowReady), attach]),
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
            cli_server::subscription().map(Msg::Cli),
            sola_terminal::osc9999::subscription()
                .map(|(id, payload)| Msg::Osc(id, payload)),
            iced::time::every(Duration::from_secs(1)).map(|_| Msg::PresenceTick),
            event::listen_with(|ev, status, _| match &ev {
                Event::Window(iced::window::Event::Focused) => Some(Msg::WindowFocus(true)),
                Event::Window(iced::window::Event::Unfocused) => Some(Msg::WindowFocus(false)),
                Event::Keyboard(_) => Some(Msg::Input(ev.clone())),
                _ if matches!(status, iced::event::Status::Ignored) => Some(Msg::Input(ev.clone())),
                _ => None,
            }),
            iced::window::resize_events().map(|(_id, size)| Msg::Resized(size)),
            iced::time::every(Duration::from_millis(530)).map(|_| Msg::BlinkTick),
            if self
                .workspaces
                .iter()
                .any(|w| w.status == status::AgentStatus::Working)
            {
                iced::window::frames().map(|_| Msg::StatusTick)
            } else {
                Subscription::none()
            },
            event::listen_with(|ev, _, _| match ev {
                Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x, position.y))
                }
                Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                _ => None,
            }),
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
            Msg::StatusTick => Task::none(),
            Msg::WindowFocus(on) => {
                self.window_focused = on;
                Task::none()
            }
            Msg::Cli(incoming) => self.on_cli(incoming),
            Msg::Hook(incoming) => {
                if let Some(id) = self.resolve_pane(&incoming.pane_id) {
                    let prev = self
                        .pane_status
                        .get(&id)
                        .map(|s| s.status)
                        .unwrap_or_default();
                    let st = self.pane_status.entry(id.clone()).or_default();
                    st.apply_hook(&incoming);
                    let now = st.status;
                    let unconfirmed = st.restored_unconfirmed;
                    status::persist_all(&self.pane_status);
                    self.sync_row(&id);
                    self.maybe_toast_done(&id, prev, now, unconfirmed);
                }
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
                    self.maybe_toast_done(&id, prev, now, unconfirmed);
                }
                Task::none()
            }
            Msg::PresenceTick => {
                let ids: Vec<String> = self.workspaces.iter().map(|w| w.id.clone()).collect();
                for id in ids {
                    let tmux_session = tmux::session_name(&id);
                    let who = presence::scan_session(&tmux_session);
                    self.pane_status.entry(id).or_default().apply_presence(who);
                }
                self.sync_all_rows();
                Task::none()
            }
            Msg::SelectWorkspace(id) => {
                if self.workspaces.iter().any(|w| w.id == id) {
                    self.selected = id.clone();
                    self.drop_armed = None;
                    self.persist_catalog();
                    return self.attach_pane(&id, &[]);
                }
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
            Msg::DismissDialog => {
                self.spawn = sidebar::SpawnDraft::default();
                self.add = sidebar::AddDraft::default();
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
            Msg::CloseWorkspace(id) => self.close_workspace(&id),
            Msg::HoverSidebar(id) => {
                self.sidebar.hovered = id;
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
                self.resize_pane();
                Task::none()
            }
            Msg::SelectionChanged => {
                if let Some(rt) = self.runtimes.get(&self.selected) {
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
            Msg::Pasted(text) => {
                if let (Some(text), Some(rt)) = (text, self.runtimes.get(&self.selected)) {
                    let mode = { *rt.emulator.term().lock().mode() };
                    rt.backend.write(&input::paste(&text, mode));
                }
                Task::none()
            }
            Msg::SidebarDragStart => {
                self.sidebar.dragging_divider = true;
                self.sidebar.drag_anchor = None;
                Task::none()
            }
            Msg::CursorMoved(x, _y) => {
                if self.sidebar.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.sidebar.drag_anchor {
                        self.sidebar.width =
                            sola_kit::components::panel_dragged_width(anchor_x, anchor_w, x);
                        self.resize_pane();
                    } else {
                        self.sidebar.drag_anchor = Some((x, self.sidebar.width));
                    }
                }
                Task::none()
            }
            Msg::CursorReleased => {
                self.sidebar.dragging_divider = false;
                self.sidebar.drag_anchor = None;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        let pane: Element<'_, Msg> = match self.runtimes.get(&self.selected) {
            Some(rt) => {
                let view = term_view::TermView {
                    term: rt.emulator.term(),
                    cursor_snap: rt.emulator.cursor_snap(),
                    cache: &rt.cache,
                    palette: &self.palette,
                    metrics: self.metrics,
                    cursor_on: self.cursor_on,
                    active: true,
                    on_select: Msg::SelectionChanged,
                    on_scroll: Msg::Scrolled(self.selected.clone()),
                    on_open_url: Box::new(Msg::OpenUrl),
                    on_wheel_pty: Box::new({
                        let pid = self.selected.clone();
                        move |bytes| Msg::WheelToPty(pid.clone(), bytes)
                    }),
                };
                canvas(view).width(Length::Fill).height(Length::Fill).into()
            }
            None => container(
                sola_kit::components::text::body(if self.workspaces.is_empty() {
                    "Add a project to open a pane."
                } else {
                    "no pane"
                })
                .style(sola_kit::components::text::muted),
            )
            .padding(sola_kit::components::style::SPACE_MD)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        };

        let rail_pane: Element<'_, Msg> = row![
            sidebar::view(
                &self.sidebar,
                &self.projects,
                &self.workspaces,
                &self.selected,
                self.drop_armed.as_deref(),
                &self.theme,
                self.palette.bg,
            ),
            pane,
        ]
        .into();

        let body: Element<'_, Msg> = match sidebar::overlay(&self.spawn, &self.add, &self.projects)
        {
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
                self.metrics =
                    CellMetrics::for_font(self.metrics.font_size, fonts::mono_metrics());
                self.resize_pane();
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
                    "drop-workspace" => {
                        let id = self.selected.clone();
                        if id.is_empty() {
                            Task::none()
                        } else {
                            self.close_workspace(&id)
                        }
                    }
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

    fn attach_pane(&mut self, id: &str, exec: &[&str]) -> Task<Msg> {
        if self.runtimes.contains_key(id) {
            self.resize_pane();
            return Task::none();
        }
        let Some(ws) = self.workspaces.iter().find(|w| w.id == id) else {
            return Task::none();
        };
        let (cols, rows) = self.cols_rows();
        let cols = if cols == 0 { DEFAULT_COLS } else { cols };
        let rows = if rows == 0 { DEFAULT_ROWS } else { rows };
        let tmux_session = tmux::session_name(id);
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
            ("SOLA_AT_HOOKS_SOCK", hook_sock.as_str()),
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
        Task::none()
    }

    fn resolve_pane(&self, id: &str) -> Option<String> {
        if self.workspaces.iter().any(|w| w.id == id) {
            return Some(id.to_string());
        }
        if self.adopted_from.as_deref() == Some(id)
            && self
                .workspaces
                .iter()
                .any(|w| w.id == workspace::LIVE_ID)
        {
            return Some(workspace::LIVE_ID.into());
        }
        None
    }

    fn sync_row(&mut self, id: &str) {
        let Some(st) = self.pane_status.get(id) else {
            return;
        };
        let status = st.status;
        let agent = st.agent.clone();
        if let Some(ws) = workspace::find_workspace_mut(&mut self.workspaces, id) {
            ws.status = status;
            ws.agent = agent;
        }
    }

    fn sync_all_rows(&mut self) {
        let ids: Vec<String> = self.workspaces.iter().map(|w| w.id.clone()).collect();
        for id in ids {
            self.sync_row(&id);
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
        self.spawn = sidebar::SpawnDraft::open(project_id);
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(sidebar::SPAWN_INPUT_ID))
    }

    fn open_add(&mut self) -> Task<Msg> {
        self.spawn = sidebar::SpawnDraft::default();
        self.add = sidebar::AddDraft {
            open: true,
            path: String::new(),
            error: None,
        };
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(sidebar::ADD_INPUT_ID))
    }

    fn spawn_sibling(&mut self) -> Task<Msg> {
        let Some(project_id) = self.spawn.project_id.clone() else {
            return Task::none();
        };
        let name = self.spawn.name.clone();
        match self.spawn_workspace(&project_id, &name, None, None) {
            Ok(id) => {
                self.spawn = sidebar::SpawnDraft::default();
                self.attach_pane(&id, &[])
            }
            Err(e) => {
                self.spawn.error = Some(e);
                Task::none()
            }
        }
    }

    /// Create a worktree + catalog row. Does not attach a PTY.
    fn spawn_workspace(
        &mut self,
        project_q: &str,
        name: &str,
        parent: Option<String>,
        agent: Option<&str>,
    ) -> Result<String, String> {
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
        let dest = spawn::add_worktree(&project.root, &slug)?;
        let taken: HashSet<String> = self.workspaces.iter().map(|w| w.id.clone()).collect();
        let id = workspace::unique_id("ws", &slug, &taken);
        let parent = match parent {
            Some(p) => Some(workspace::resolve_workspace(&self.workspaces, &p)?.id.clone()),
            None => self
                .workspaces
                .iter()
                .find(|w| w.project_id == project.id && w.kind == workspace::Kind::Main)
                .map(|w| w.id.clone()),
        };
        self.workspaces.push(workspace::Workspace {
            id: id.clone(),
            project_id: project.id,
            name: name.trim().to_string(),
            path: dest,
            kind: workspace::Kind::Worktree,
            parent,
            status: status::AgentStatus::Idle,
            agent: agent.map(str::to_string),
        });
        self.selected = id.clone();
        self.persist_catalog();
        Ok(id)
    }

    fn maybe_toast_done(
        &self,
        id: &str,
        prev: status::AgentStatus,
        now: status::AgentStatus,
        unconfirmed: bool,
    ) {
        if now != status::AgentStatus::Done || prev == status::AgentStatus::Done {
            return;
        }
        if unconfirmed || self.window_focused {
            return;
        }
        let ws = self.workspaces.iter().find(|w| w.id == id);
        let name = ws.map(|w| w.name.as_str()).unwrap_or(id);
        let agent = self
            .pane_status
            .get(id)
            .and_then(|s| s.agent.as_deref())
            .unwrap_or("agent");
        if let Ok(mut client) = bus().lock() {
            let _ = client.emit(Topic::AppToast(AppToast {
                text: format!("{name} · {agent} is done"),
            }));
        }
    }

    fn on_cli(&mut self, incoming: cli_server::Incoming) -> Task<Msg> {
        let (resp, attach) = self.dispatch_cli(incoming.req);
        let _ = incoming.reply.send(resp);
        attach
    }

    fn dispatch_cli(&mut self, req: Request) -> (Response, Task<Msg>) {
        match req {
            Request::Ps => (Response::ok(self.ps_json()), Task::none()),
            Request::ProjectList => {
                let projects: Vec<serde_json::Value> = self
                    .projects
                    .iter()
                    .map(|p| serde_json::json!({"id": p.id, "name": p.name}))
                    .collect();
                (
                    Response::ok(serde_json::json!({ "projects": projects })),
                    Task::none(),
                )
            }
            Request::WorkspaceList { project } => match self.cli_workspace_list(project.as_deref())
            {
                Ok(v) => (Response::ok(v), Task::none()),
                Err(e) => (Response::err(e), Task::none()),
            },
            Request::WorkspaceSpawn {
                project,
                name,
                agent,
                prompt,
                parent,
            } => match self.cli_spawn(&project, &name, agent.as_deref(), prompt.as_deref(), parent)
            {
                Ok((id, task)) => (Response::ok(serde_json::json!({ "id": id })), task),
                Err(e) => (Response::err(e), Task::none()),
            },
            Request::WorkspaceRm { workspace } => match self.cli_rm(&workspace) {
                Ok(task) => (Response::ok(serde_json::json!({ "ok": true })), task),
                Err(e) => (Response::err(e), Task::none()),
            },
            Request::PaneList { workspace } => match self.cli_pane_list(workspace.as_deref()) {
                Ok(v) => (Response::ok(v), Task::none()),
                Err(e) => (Response::err(e), Task::none()),
            },
            Request::PaneSend { pane, text, enter } => {
                match self.cli_send(pane.as_deref(), &text, enter) {
                    Ok(()) => (Response::ok(serde_json::json!({ "ok": true })), Task::none()),
                    Err(e) => (Response::err(e), Task::none()),
                }
            }
            Request::PaneRead { pane, lines } => match self.cli_read(pane.as_deref(), lines) {
                Ok(text) => (Response::ok(serde_json::json!({ "text": text })), Task::none()),
                Err(e) => (Response::err(e), Task::none()),
            },
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
                        .map(|w| {
                            let title = if w.kind == workspace::Kind::Main {
                                "root"
                            } else {
                                w.name.as_str()
                            };
                            serde_json::json!({
                                "id": w.id,
                                "name": title,
                                "status": format!("{:?}", w.status).to_lowercase(),
                                "agent": w.agent,
                                "selected": w.id == self.selected,
                            })
                        })
                        .collect();
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "workspaces": workspaces,
                })
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
            .map(|w| {
                serde_json::json!({
                    "id": w.id,
                    "name": w.name,
                    "status": format!("{:?}", w.status).to_lowercase(),
                    "project": w.project_id,
                })
            })
            .collect();
        Ok(serde_json::json!({ "workspaces": list }))
    }

    fn cli_spawn(
        &mut self,
        project: &str,
        name: &str,
        agent: Option<&str>,
        prompt: Option<&str>,
        parent: Option<String>,
    ) -> Result<(String, Task<Msg>), String> {
        let agent = match (agent, prompt) {
            (Some(a), _) => Some(a),
            (None, Some(_)) => Some("grok"),
            (None, None) => None,
        };
        let id = self.spawn_workspace(project, name, parent, agent)?;
        let task = if agent == Some("grok") {
            let mut args = vec!["grok".to_string()];
            if let Some(p) = prompt {
                let p = p.trim();
                if !p.is_empty() {
                    args.push(p.to_string());
                }
            }
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.attach_pane(&id, &refs)
        } else {
            self.attach_pane(&id, &[])
        };
        Ok((id, task))
    }

    fn cli_rm(&mut self, q: &str) -> Result<Task<Msg>, String> {
        let id = workspace::resolve_workspace(&self.workspaces, q)?.id.clone();
        self.drop_armed = Some(id.clone());
        Ok(self.close_workspace(&id))
    }

    fn cli_pane_list(&self, workspace: Option<&str>) -> Result<serde_json::Value, String> {
        let id = match workspace {
            Some(q) => workspace::resolve_workspace(&self.workspaces, q)?.id.clone(),
            None => self.selected.clone(),
        };
        if id.is_empty() {
            return Err("no workspace selected".into());
        }
        let st = self.pane_status.get(&id);
        Ok(serde_json::json!({
            "panes": [{
                "id": id,
                "status": format!("{:?}", st.map(|s| s.status).unwrap_or_default()).to_lowercase(),
                "agent": st.and_then(|s| s.agent.clone()),
            }]
        }))
    }

    fn cli_pane_id(&self, pane: Option<&str>) -> Result<String, String> {
        if let Some(q) = pane {
            return Ok(workspace::resolve_workspace(&self.workspaces, q)?.id.clone());
        }
        if !self.selected.is_empty() {
            return Ok(self.selected.clone());
        }
        Err("no pane".into())
    }

    fn cli_send(&self, pane: Option<&str>, text: &str, enter: bool) -> Result<(), String> {
        let id = self.cli_pane_id(pane)?;
        let session = tmux::session_name(&id);
        if let Some(rt) = self.runtimes.get(&id) {
            rt.backend.write(text.as_bytes());
            if enter {
                rt.backend.write(b"\r");
            }
            return Ok(());
        }
        if !tmux::send_literal(&session, text) {
            return Err("send failed".into());
        }
        if enter && !tmux::send_enter(&session) {
            return Err("enter failed".into());
        }
        Ok(())
    }

    fn cli_read(&self, pane: Option<&str>, lines: Option<u32>) -> Result<String, String> {
        let id = self.cli_pane_id(pane)?;
        let session = tmux::session_name(&id);
        let text = tmux::capture_scrollback(&session)?;
        match lines {
            Some(n) if n > 0 => {
                let keep = n as usize;
                let mut v: Vec<&str> = text.lines().collect();
                if v.len() > keep {
                    v = v.split_off(v.len() - keep);
                }
                Ok(v.join("\n"))
            }
            _ => Ok(text),
        }
    }

    fn add_project(&mut self) -> Task<Msg> {
        let raw = self.add.path.trim();
        if raw.is_empty() {
            self.add.error = Some("folder path required".into());
            return Task::none();
        }
        let root = match PathBuf::from(raw).canonicalize() {
            Ok(p) if p.is_dir() => p,
            Ok(_) => {
                self.add.error = Some("not a folder".into());
                return Task::none();
            }
            Err(e) => {
                self.add.error = Some(format!("path: {e}"));
                return Task::none();
            }
        };
        if self.projects.iter().any(|p| p.root == root) {
            self.add.error = Some("already in the rail".into());
            return Task::none();
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
        }
        let attach_id = ws.id.clone();
        self.workspaces.push(ws);
        self.add = sidebar::AddDraft::default();
        self.persist_catalog();
        if self.selected == attach_id {
            self.attach_pane(&attach_id, &[])
        } else {
            Task::none()
        }
    }

    fn close_workspace(&mut self, id: &str) -> Task<Msg> {
        if self.drop_armed.as_deref() != Some(id) {
            self.drop_armed = Some(id.to_string());
            return Task::none();
        }
        self.drop_armed = None;
        if let Some(rt) = self.runtimes.remove(id) {
            rt.backend.close();
        } else {
            tmux::kill_session(&tmux::session_name(id));
        }
        self.pane_status.remove(id);
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
        if self.selected.is_empty() || self.runtimes.contains_key(&self.selected) {
            Task::none()
        } else {
            let next = self.selected.clone();
            self.attach_pane(&next, &[])
        }
    }

    fn resize_pane(&mut self) {
        let Some(rt) = self.runtimes.get(&self.selected) else {
            return;
        };
        let (cols, rows) = self.cols_rows();
        rt.emulator.resize(cols, rows);
        rt.backend.resize(cols, rows);
        rt.backend.sigwinch();
        rt.cache.clear();
    }

    fn dialog_open(&self) -> bool {
        self.spawn.is_open() || self.add.open
    }

    fn on_input(&mut self, event: iced::Event) -> Task<Msg> {
        if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) = &event
        {
            if self.dialog_open() {
                self.spawn = sidebar::SpawnDraft::default();
                self.add = sidebar::AddDraft::default();
                return Task::none();
            }
        }
        if self.dialog_open() {
            return Task::none();
        }
        if let iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) = event {
            self.keyboard_mods = mods;
            return Task::none();
        }
        if let iced::Event::Keyboard(keyboard::Event::KeyReleased {
            key,
            physical_key,
            ..
        }) = &event
        {
            self.apply_modifier_key(key, physical_key, false);
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

        let modifiers = modifiers | self.keyboard_mods | self.keys_held_mods;
        self.keyboard_mods = modifiers;
        if modifiers.logo() {
            return Task::none();
        }

        let Some(rt) = self.runtimes.get(&self.selected) else {
            return Task::none();
        };
        let mut mode = { *rt.emulator.term().lock().mode() };
        if extkeys::level(&self.selected) >= 1 {
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
            modify_other_keys: extkeys::level(&self.selected) >= 1,
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
