//! sola-design app: project rail + plane.
//!
//! The plane stacks the artboard textures (wgpu shader), the overlay canvas
//! (frames, titles, pick outline, all input), and a status pill (zoom, pick).
//! Boards are CEF board tabs in one `--engine` helper. Board lifecycle lives
//! in [`live`]; the call owner `design` in [`cli`].

mod cli;
mod live;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use iced::widget::{Shader, canvas as iced_canvas, column, container, row, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Point, Size, Subscription, Task, Theme};
use sola_browser::CefEngine;
use sola_browser::agent::{AgentOp, AgentReply, AgentRequest};
use sola_browser::cef::engine::InputEvent;
use sola_browser::engine::{Cmd, Engine, TabId};
use sola_bus::Message;
use sola_bus::topics::{FocusTarget, Topic, TopicKind};
use sola_call::Incoming;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, window_settings_transparent,
};
use sola_kit::components::{SidebarEvent, SidebarMsg};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::APP_ID;
use crate::boards::Board;
use crate::calls;
use crate::camera::{self, Camera};
use crate::canvas;
use crate::frames::{self, BoardFrames};
use crate::instance;
use crate::loads::Loads;
use crate::menu;
use crate::overlay::{self, Overlay};
use crate::plane::{self, Plane};
use crate::projects::{self, CanvasRef, Catalog};
use crate::rail::{self, AddDraft, CanvasDraft, RailState};
use crate::select::{self, Selection};
use crate::watch;

const WINDOW_TITLE: &str = "Design";
/// Helper view size before any board sets its own.
const SPAWN_W: u32 = 1280;
const SPAWN_H: u32 = 800;
/// After a reload, re-find the selected element once the page has painted.
const REQUERY_DELAY: Duration = Duration::from_millis(400);
/// Tick for `wait` deadlines and `auto` measures, only while one is pending.
const PENDING_TICK: Duration = Duration::from_millis(200);

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    /// Board frames are waiting for the plane.
    Frame,
    /// Helper queues have work (agent replies, tabs, dialogs).
    Poll,
    /// `solactl design …`.
    Call(Incoming),
    /// Re-check pending loads, measures, and `wait` deadlines.
    WaitTick,
    Plane(overlay::Event),
    Sidebar(SidebarMsg),
    SelectCanvas(String),
    Canvases(HashMap<String, Vec<canvas::Entry>>),
    CanvasChanged(PathBuf, watch::Change),
    Requery,
    OpenAdd,
    AddPath(String),
    AddProject,
    OpenNewCanvas(String),
    CanvasName(String),
    CreateCanvas,
    DismissDialog,
    Ignore,
}

pub fn run() -> iced::Result {
    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::Windows,
            TopicKind::WindowFloating,
            // River delivers a bare Super press as a chord: Super+click.
            TopicKind::Chord,
            TopicKind::ChordReleased,
        ])
        .calls(calls::OWNER, calls::methods())
        .install();
    publish_menu();

    let engine = CefEngine::spawn(APP_ID, "", SPAWN_W, SPAWN_H);
    let cell = std::cell::Cell::new(Some(engine));
    let mut settings = window_settings_transparent(APP_ID);
    settings.size = Size::new(1280.0, 820.0);

    iced::application(
        move || App::new(cell.take().expect("design boots once")),
        App::update,
        App::view,
    )
    .title(App::title)
    .subscription(App::subscription)
    .theme(App::theme)
    .default_font(fonts::ui())
    .window(settings)
    .run()
}

fn publish_menu() {
    if let Ok(mut client) = bus().lock() {
        if let Err(e) = client.emit(Topic::SetAppMenu(menu::app_menu())) {
            tracing::warn!("app-menu publish failed: {e:?}");
        }
    }
}

struct OpenCanvas {
    at: CanvasRef,
    dir: PathBuf,
    /// Last `canvas.toml` read failure; the previous layout stays up.
    error: Option<String>,
}

/// Agent calls waiting for a reply. Nav calls are fire-and-forget.
enum Pending {
    Pick { at: CanvasRef, artboard: String, tab: u64 },
    Requery { tab: u64 },
    /// A `solactl design` call answered by the page.
    Call { inc: Incoming, kind: cli::CallKind },
    /// Content height of an `auto` artboard.
    Measure { tab: u64 },
}

struct App {
    engine: CefEngine,
    frames: Arc<BoardFrames>,
    catalog: Catalog,
    canvases: HashMap<String, Vec<canvas::Entry>>,
    open: Option<OpenCanvas>,
    boards: Vec<Board>,
    /// Board tabs the helper has listed at least once. One that drops out
    /// without us closing it means the helper restarted (empty).
    listed: HashSet<u64>,
    camera: Camera,
    plane_size: Option<Size>,
    fit_pending: bool,
    /// The human (or `view`) moved the camera since the canvas opened;
    /// until then an `auto` board that grows re-fits the canvas.
    user_camera: bool,
    /// Board with the keyboard.
    focus: Option<u64>,
    selection: Option<Selection>,
    next_req: u64,
    pending: HashMap<u64, Pending>,
    waits: Vec<cli::Wait>,
    loads: Loads,
    rail: RailState,
    add: AddDraft,
    canvas_draft: CanvasDraft,
    theme: Theme,
    float: sola_kit::FloatState,
}

impl App {
    fn new(engine: CefEngine) -> (Self, Task<Msg>) {
        let catalog = projects::load_from(&projects::catalog_path());
        let mut app = Self {
            engine,
            frames: BoardFrames::new(),
            catalog,
            canvases: HashMap::new(),
            open: None,
            boards: Vec::new(),
            listed: HashSet::new(),
            camera: Camera::default(),
            plane_size: None,
            fit_pending: false,
            user_camera: false,
            focus: None,
            selection: None,
            next_req: 1,
            pending: HashMap::new(),
            waits: Vec::new(),
            loads: Loads::default(),
            rail: RailState::default(),
            add: AddDraft::default(),
            canvas_draft: CanvasDraft::default(),
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
        };
        app.rescan();
        if let Some(at) = app.catalog.selected.clone() {
            app.open_canvas(at);
        }
        (app, Task::none())
    }

    fn title(&self) -> String {
        WINDOW_TITLE.to_string()
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    fn subscription(&self) -> Subscription<Msg> {
        let projects: Vec<(String, PathBuf)> = self
            .catalog
            .projects
            .iter()
            .map(|p| (p.id.clone(), p.root.clone()))
            .collect();
        let mut subs = vec![
            frames::subscription(self.engine.frames(), self.frames.clone(), Msg::Frame),
            Subscription::run(poll_stream),
            bus_subscription().map(Msg::Bus),
            sola_kit::call_subscription().map(Msg::Call),
            watch::canvases(projects, Msg::Canvases),
        ];
        if let Some(open) = &self.open {
            subs.push(watch::subscription(open.dir.clone(), Msg::CanvasChanged));
        }
        if !self.waits.is_empty() || self.loads.has_due() {
            subs.push(iced::time::every(PENDING_TICK).map(|_| Msg::WaitTick));
        }
        Subscription::batch(subs)
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => return self.on_bus(&message),
            Msg::Frame => {
                self.frames.wake_done();
                if !self.waits.is_empty() || self.loads.has_due() {
                    self.observe_loads();
                    self.check_waits();
                }
            }
            Msg::Poll => self.poll(),
            Msg::Call(inc) => return self.on_call(inc),
            Msg::WaitTick => {
                self.observe_loads();
                self.check_waits();
            }
            Msg::Plane(ev) => return self.on_plane(ev),
            Msg::Sidebar(m) => {
                if let Some(ev) = self.rail.gestures.update(m) {
                    self.on_rail(ev);
                }
            }
            Msg::SelectCanvas(key) => {
                if let Some(at) = CanvasRef::from_key(&key) {
                    self.open_canvas(at);
                }
            }
            Msg::Canvases(found) => self.canvases = found,
            Msg::CanvasChanged(dir, change) => {
                if self.open.as_ref().is_some_and(|o| o.dir == dir) {
                    return self.on_canvas_change(change);
                }
            }
            Msg::Requery => self.requery(),
            Msg::OpenAdd => return self.open_add(),
            Msg::AddPath(s) => self.add.path = s,
            Msg::AddProject => return self.add_project(),
            Msg::OpenNewCanvas(project) => return self.open_new_canvas(project),
            Msg::CanvasName(s) => self.canvas_draft.name = s,
            Msg::CreateCanvas => self.create_canvas(),
            Msg::DismissDialog => {
                self.add = AddDraft::default();
                self.canvas_draft = CanvasDraft::default();
            }
            Msg::Ignore => {}
        }
        Task::none()
    }

    // ── bus + menu ────────────────────────────────────────────────

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        self.float.update(message);
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            return iced::exit();
        }
        match Topic::parse(message) {
            Some(Topic::MenuAction(m)) if m.app_id == APP_ID => return self.on_menu(&m.action_id),
            Some(Topic::Chord(c)) => {
                sola_browser::input::apply_super_chord(true, c.keysym);
            }
            Some(Topic::ChordReleased(c)) => {
                sola_browser::input::apply_super_chord(false, c.keysym);
            }
            _ => {}
        }
        Task::none()
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            menu::ADD_PROJECT => return self.open_add(),
            menu::NEW_CANVAS => {
                let project = self
                    .open
                    .as_ref()
                    .map(|o| o.at.project.clone())
                    .or_else(|| self.catalog.projects.first().map(|p| p.id.clone()));
                if let Some(project) = project {
                    return self.open_new_canvas(project);
                }
            }
            menu::DROP_PROJECT => {
                if let Some(project) = self.open.as_ref().map(|o| o.at.project.clone()) {
                    self.drop_project(&project);
                }
            }
            menu::FIT => self.fit(),
            menu::ACTUAL_SIZE => {
                if let Some(c) = self.plane_center() {
                    self.user_camera = true;
                    self.camera.set_zoom_at(c, 1.0);
                }
            }
            menu::ZOOM_IN => self.zoom_step(camera::STEP),
            menu::ZOOM_OUT => self.zoom_step(1.0 / camera::STEP),
            menu::RELOAD => return self.reload_boards(),
            _ => {}
        }
        Task::none()
    }

    // ── plane input ───────────────────────────────────────────────

    fn on_plane(&mut self, ev: overlay::Event) -> Task<Msg> {
        match ev {
            overlay::Event::Bounds(b) => {
                self.plane_size = Some(b.size());
                if self.fit_pending {
                    self.fit();
                }
            }
            overlay::Event::Pan(d) => {
                self.user_camera = true;
                self.camera.pan(d);
            }
            overlay::Event::Zoom { at, factor } => {
                self.user_camera = true;
                self.camera.zoom_at(at, factor);
            }
            overlay::Event::Pick { tab, x, y } => self.pick(tab, x, y),
            overlay::Event::ClearSelection => self.selection = None,
            overlay::Event::Input(events) => {
                for (tab, input) in events {
                    if matches!(input, InputEvent::PointerButton { down: true, .. }) {
                        self.focus_board(Some(tab));
                    }
                    self.send(Cmd::BoardInput {
                        id: TabId(tab),
                        input,
                    });
                }
            }
            overlay::Event::Blur => self.focus_board(None),
        }
        Task::none()
    }

    fn focus_board(&mut self, tab: Option<u64>) {
        if self.focus == tab {
            return;
        }
        if let Some(old) = self.focus.take() {
            self.send(Cmd::BoardFocus {
                id: TabId(old),
                focused: false,
            });
        }
        if let Some(new) = tab {
            self.send(Cmd::BoardFocus {
                id: TabId(new),
                focused: true,
            });
        }
        self.focus = tab;
    }

    fn fit(&mut self) {
        let Some(view) = self.plane_size else {
            self.fit_pending = true;
            return;
        };
        let Some(content) = camera::bounds_of(self.boards.iter().map(Board::rect)) else {
            return;
        };
        self.camera = Camera::fit(content, view, 1.0);
        self.fit_pending = false;
    }

    fn plane_center(&self) -> Option<Point> {
        self.plane_size
            .map(|s| Point::new(s.width / 2.0, s.height / 2.0))
    }

    fn zoom_step(&mut self, factor: f32) {
        if let Some(c) = self.plane_center() {
            self.user_camera = true;
            self.camera.zoom_at(c, factor);
        }
    }

    // ── selection ─────────────────────────────────────────────────

    fn pick(&mut self, tab: u64, x: f32, y: f32) {
        let (Some(open), Some(board)) = (
            self.open.as_ref(),
            self.boards.iter().find(|b| b.tab.0 == tab),
        ) else {
            return;
        };
        let why = Pending::Pick {
            at: open.at.clone(),
            artboard: board.art.id.clone(),
            tab,
        };
        let expression = select::pick_script(x, y);
        self.agent(TabId(tab), AgentOp::Eval { expression }, Some(why));
    }

    fn requery_later(&self) -> Task<Msg> {
        if self.selection.is_none() {
            return Task::none();
        }
        Task::perform(tokio::time::sleep(REQUERY_DELAY), |_| Msg::Requery)
    }

    fn requery(&mut self) {
        let Some(sel) = &self.selection else {
            return;
        };
        let tab = sel.tab;
        let expression = select::requery_script(&sel.picked.selector);
        self.agent(TabId(tab), AgentOp::Eval { expression }, Some(Pending::Requery { tab }));
    }

    fn on_reply(&mut self, reply: AgentReply) {
        let Some(why) = self.pending.remove(&reply.id) else {
            return;
        };
        let why = match why {
            Pending::Call { inc, kind } => return self.finish_call(inc, kind, reply),
            Pending::Measure { tab } => return self.on_measured(tab, reply),
            other => other,
        };
        if !reply.ok {
            tracing::warn!(id = reply.id, error = ?reply.error, "board eval failed");
            return;
        }
        let picked = match select::parse(reply.data.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "pick result");
                return;
            }
        };
        match why {
            Pending::Pick { at, artboard, tab } => {
                let current = self.open.as_ref().is_some_and(|o| o.at == at)
                    && self.boards.iter().any(|b| b.tab.0 == tab);
                if !current {
                    return;
                }
                self.selection = picked.map(|picked| Selection {
                    canvas: at,
                    artboard,
                    tab,
                    picked,
                });
                if let Some(s) = &self.selection {
                    tracing::info!(selection = %s.label(), "picked");
                }
            }
            Pending::Requery { tab } => match (&mut self.selection, picked) {
                (Some(sel), Some(picked)) if sel.tab == tab => sel.picked = picked,
                (Some(sel), None) if sel.tab == tab => self.selection = None,
                _ => {}
            },
            Pending::Call { .. } | Pending::Measure { .. } => unreachable!("answered above"),
        }
    }

    // ── engine plumbing ───────────────────────────────────────────

    fn send(&self, cmd: Cmd<CefEngine>) {
        if self.engine.cmd_sender().send(cmd).is_err() {
            tracing::warn!("engine command channel closed");
        }
    }

    fn agent(&mut self, tab: TabId, op: AgentOp, why: Option<Pending>) {
        let id = self.next_req;
        self.next_req += 1;
        if let Some(why) = why {
            self.pending.insert(id, why);
        }
        self.send(Cmd::Agent(AgentRequest { id, tab: tab.0, op }));
    }

    /// Drain helper queues (and a second launch's raise). Design pages have
    /// no chrome for dialogs, popups, or auth: dialogs are accepted, auth is
    /// cancelled, the rest dropped.
    fn poll(&mut self) {
        sola_browser::chrome_wake::take_queued();
        if instance::take_activate() {
            self.raise();
        }
        let replies = std::mem::take(&mut *self.engine.agent_handle().lock().unwrap());
        for reply in replies {
            self.on_reply(reply);
        }
        let dialogs = std::mem::take(&mut *self.engine.js_dialogs_handle().lock().unwrap());
        for ev in dialogs {
            if let sola_browser::js_dialog::Event::Open(d) = ev {
                tracing::info!(tab = d.tab_id, message = %d.message, "accepting page dialog");
                self.send(Cmd::JsDialog {
                    id: d.id,
                    success: true,
                    input: d.default_prompt,
                });
            }
        }
        let auth = std::mem::take(&mut *self.engine.http_auth_handle().lock().unwrap());
        for ev in auth {
            if let sola_browser::http_auth::Event::Open(a) = ev {
                self.send(Cmd::HttpAuth {
                    id: a.id,
                    success: false,
                    username: String::new(),
                    password: String::new(),
                });
            }
        }
        self.check_helper_restart();
        self.observe_loads();
        self.check_waits();
        self.engine.background_tabs_handle().lock().unwrap().clear();
        self.engine.notifications_handle().lock().unwrap().clear();
        self.engine.favicons_handle().lock().unwrap().clear();
        self.engine.page_menus_handle().lock().unwrap().clear();
        self.engine.downloads_handle().lock().unwrap().clear();
    }

    fn raise(&self) {
        let Some(window_id) = self.float.any_window_id() else {
            return;
        };
        if let Ok(mut b) = bus().lock() {
            let _ = b.emit(Topic::Focus(FocusTarget { window_id }));
        }
    }

    // ── rail + dialogs ────────────────────────────────────────────

    fn on_rail(&mut self, ev: SidebarEvent) {
        match ev {
            SidebarEvent::Activate { id } => {
                if let Some(at) = CanvasRef::from_key(&id) {
                    self.open_canvas(at);
                }
            }
            SidebarEvent::ToggleSection { id } => {
                if let Some(p) = self.catalog.projects.iter_mut().find(|p| p.id == id) {
                    p.collapsed = !p.collapsed;
                    self.persist();
                }
            }
            SidebarEvent::Resize { width } => self.rail.width = width,
            SidebarEvent::Edit { .. } | SidebarEvent::Drop(_) => {}
        }
    }

    fn rescan(&mut self) {
        self.canvases = self
            .catalog
            .projects
            .iter()
            .map(|p| (p.id.clone(), canvas::scan(&p.root)))
            .collect();
    }

    fn persist(&self) {
        if let Err(e) = projects::save_to(&projects::catalog_path(), &self.catalog) {
            tracing::warn!(error = %e, "save design projects");
        }
    }

    fn dialog_open(&self) -> bool {
        self.add.open || self.canvas_draft.project.is_some()
    }

    fn open_add(&mut self) -> Task<Msg> {
        self.canvas_draft = CanvasDraft::default();
        self.add = AddDraft {
            open: true,
            ..AddDraft::default()
        };
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(rail::ADD_INPUT_ID))
    }

    fn add_project(&mut self) -> Task<Msg> {
        match self.catalog.add(&self.add.path) {
            Ok(id) => {
                tracing::info!(project = %id, "added design project");
                self.add = AddDraft::default();
                self.persist();
                self.rescan();
            }
            Err(e) => self.add.error = Some(e),
        }
        Task::none()
    }

    fn open_new_canvas(&mut self, project: String) -> Task<Msg> {
        self.add = AddDraft::default();
        self.canvas_draft = CanvasDraft {
            project: Some(project),
            ..CanvasDraft::default()
        };
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(rail::CANVAS_INPUT_ID))
    }

    fn create_canvas(&mut self) {
        let Some(project) = self.canvas_draft.project.clone() else {
            return;
        };
        let Some(root) = self.catalog.project(&project).map(|p| p.root.clone()) else {
            return;
        };
        match canvas::create(&root, &self.canvas_draft.name) {
            Ok(name) => {
                self.canvas_draft = CanvasDraft::default();
                self.rescan();
                self.open_canvas(CanvasRef {
                    project,
                    canvas: name,
                });
            }
            Err(e) => self.canvas_draft.error = Some(e),
        }
    }

    fn drop_project(&mut self, project: &str) {
        if self.open.as_ref().is_some_and(|o| o.at.project == project) {
            self.close_canvas();
        }
        self.catalog.drop_project(project);
        self.persist();
        self.rescan();
    }

    // ── view ──────────────────────────────────────────────────────

    fn view(&self) -> Element<'_, Msg> {
        let plane_bg = self.theme.extended_palette().background.base.color;
        let rail = rail::view(
            &self.rail,
            &self.catalog.projects,
            &self.canvases,
            self.catalog.selected.as_ref(),
            &self.theme,
            plane_bg,
        );
        let main = row![rail, self.plane_view(plane_bg)];
        match rail::dialog(&self.add, &self.canvas_draft, &self.catalog.projects) {
            Some(dialog) => stack![main, dialog].into(),
            None => main.into(),
        }
    }

    fn plane_view(&self, bg: Color) -> Element<'_, Msg> {
        let body: Element<'_, Msg> = match &self.open {
            None => hint("Pick a canvas, or add a project (⌘N)."),
            Some(open) => {
                let items = self
                    .boards
                    .iter()
                    .map(|b| plane::Item {
                        tab: b.tab.0,
                        rect: b.rect(),
                    })
                    .collect();
                let textures = Shader::new(Plane {
                    frames: self.frames.clone(),
                    items,
                    camera: self.camera,
                })
                .width(Length::Fill)
                .height(Length::Fill);
                let overlay = Overlay {
                    camera: self.camera,
                    boards: &self.boards,
                    painted: self
                        .boards
                        .iter()
                        .map(|b| self.frames.is_painted(b.tab.0))
                        .collect(),
                    selection: self.selection.as_ref().map(|s| (s.tab, s.picked.rect)),
                    focus: self.focus,
                    cursor: self.engine.cursor_handle().load(Ordering::Relaxed),
                    live: !self.dialog_open(),
                };
                let overlay: Element<'_, overlay::Event> = iced_canvas(overlay)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                let mut layers = stack![textures, overlay.map(Msg::Plane)];
                if let Some(e) = &open.error {
                    layers = layers.push(error_chip(e));
                } else if self.boards.is_empty() {
                    let file = format!(
                        "No artboards yet. Add [[artboard]] entries to design/{}/canvas.toml.",
                        open.at.canvas
                    );
                    layers = layers.push(hint(&file));
                }
                layers.push(self.status_bar()).into()
            }
        };
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(bg)),
                ..container::Style::default()
            })
            .into()
    }

    /// Zoom and the current pick, floating at the bottom of the plane.
    fn status_bar(&self) -> Element<'_, Msg> {
        let small = |t: String| {
            text(t)
                .size(12)
                .font(fonts::mono())
                .style(sola_kit::components::text::muted)
        };
        let mut bar = row![small(format!("{:.0}%", self.camera.zoom * 100.0))]
            .spacing(10)
            .align_y(Alignment::Center);
        if let Some(sel) = &self.selection {
            bar = bar.push(small(sel.label()));
        }
        let pill = container(bar).padding([6, 10]).style(pill_style);
        container(pill)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::End)
            .padding(16)
            .into()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

/// `chrome_wake` → `Msg::Poll`. The router wakes when a helper queue has
/// work, so there is no idle timer.
fn poll_stream() -> impl iced::futures::Stream<Item = Msg> {
    use iced::futures::StreamExt;
    let (tx, rx) = iced::futures::channel::mpsc::unbounded();
    sola_browser::chrome_wake::install_tx(tx);
    rx.map(|()| Msg::Poll)
}

fn hint<'a>(message: &str) -> Element<'a, Msg> {
    container(
        sola_kit::components::text::caption(message.to_string())
            .style(sola_kit::components::text::muted),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn error_chip<'a>(message: &str) -> Element<'a, Msg> {
    let body = column![
        text("canvas.toml").size(12).font(fonts::ui_medium()),
        text(message.to_string()).size(12).font(fonts::mono()),
    ]
    .spacing(4);
    container(container(body).padding([8, 12]).style(|theme: &Theme| {
        let p = theme.extended_palette();
        container::Style {
            background: Some(Background::Color(p.background.base.color)),
            text_color: Some(p.danger.base.color),
            border: Border {
                color: p.danger.base.color,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..container::Style::default()
        }
    }))
    .padding(16)
    .into()
}

fn pill_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weak.color)),
        border: Border {
            color: p.background.strong.color,
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}
