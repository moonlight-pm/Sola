//! sola-design app: project rail + plane.
//!
//! The plane stacks the artboard textures (wgpu shader), the overlay canvas
//! (frames, titles, pick outline, all input), and a status pill (zoom, pick).
//! Boards are CEF board tabs in one `--engine` helper. Board lifecycle lives
//! in [`live`]; the memory budget in [`budget`]; the call owner `design` in
//! [`cli`].

mod budget;
mod canvases;
mod cli;
mod live;
mod tour;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use iced::widget::{Shader, canvas as iced_canvas, column, container, row, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Point, Size, Subscription, Task, Theme};
use sola_browser::CefEngine;
use sola_browser::agent::{AgentOp, AgentReply, AgentRequest};
use sola_browser::cef::engine::InputEvent;
use sola_browser::engine::{BoardPace, Cmd, Engine, TabId};
use sola_bus::Message;
use sola_bus::topics::{FocusTarget, Topic, TopicKind};
use sola_call::Incoming;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, window_settings_transparent,
};
use sola_kit::components::{Dest, Drop as SidebarDrop, SidebarEvent, SidebarMsg};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::APP_ID;
use crate::annotate;
use crate::boards::Board;
use crate::calls;
use crate::camera::{self, Camera};
use crate::canvas;
use crate::detail::{self, Details};
use crate::frames::{self, BoardFrames};
use crate::glow::Glows;
use crate::instance;
use crate::loads::Loads;
use crate::memory::Ledger;
use crate::menu;
use crate::overlay::{self, Overlay};
use crate::plane::{self, Plane};
use crate::projects::{self, CanvasRef, Catalog};
use crate::rail::{self, AddDraft, CanvasDraft, RailState};
use crate::select::{self, Selection};
use crate::serve::Server;
use crate::share::{self, Share};
use crate::tour::{Nav, Tour, Tween};
use crate::watch;

const WINDOW_TITLE: &str = "Design";
/// Helper view size before any board sets its own.
const SPAWN_W: u32 = 1280;
const SPAWN_H: u32 = 800;
/// After a reload, re-find the selected element once the page has painted.
const REQUERY_DELAY: Duration = Duration::from_millis(400);
/// Tick for `wait` deadlines and `auto` measures, only while one is pending.
const PENDING_TICK: Duration = Duration::from_millis(200);
/// How long a status-pill note stays.
const NOTICE: Duration = Duration::from_millis(1800);
/// How often idle off-screen canvases are looked for.
const EVICT_TICK: Duration = Duration::from_secs(60);
/// Tick while a detail plan is due (see [`crate::detail::SETTLE`]).
const DETAIL_TICK: Duration = Duration::from_millis(40);
/// Tick while an agent's recent call keeps a canvas awake, so its boards
/// sleep once the call ages out.
const PACE_TICK: Duration = Duration::from_secs(5);
/// Board surfaces cover the plane in steps of this many px, so a window
/// resize rarely resizes them.
const SURFACE_STEP: u32 = 256;

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
    /// Plan detail renders if the camera has rested.
    DetailTick,
    /// Animation frame while something glows or the camera glides.
    Animate,
    /// Close idle off-screen canvases.
    Evict,
    /// Check the machine's free memory ([`budget`]).
    Budget,
    /// Re-pace boards (an agent's recent call aged out).
    Pace,
    /// The window gained (`true`) or lost keyboard focus.
    Focused(bool),
    /// Titlebar: start a move (floating), the close dot, an edge resize.
    TitleDrag,
    TitleClose,
    TitleResize(iced::window::Direction),
    /// Copy the shown canvas's share link (`true`: a new link first).
    ShareLink(bool),
    /// Open / close the flows list above the pill.
    Flows,
    /// Start the flow with this id on the canvas on screen.
    StartFlow(String),
    /// Open / close every note card.
    ToggleNotes,
    /// A tour-bar button (keys arrive as `Plane(Nav)`).
    Tour(Nav),
    /// Frame the tour's step again.
    TourReturn,
    /// The window's output scale.
    Scale(f32),
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
            TopicKind::WindowChrome,
            // River delivers a bare Super press as a chord: Super+click.
            TopicKind::Chord,
            TopicKind::ChordReleased,
        ])
        .calls(calls::OWNER, calls::methods())
        .install();
    publish_menu();

    let engine = CefEngine::spawn(APP_ID, "", SPAWN_W, SPAWN_H);
    // Boards load from this loopback origin (D5); without it they fall back
    // to `file://`.
    let serve = Server::start()
        .map_err(|e| tracing::error!(error = %e, "design origin did not start; boards use file://"))
        .ok();
    // Live share links (and the tunnel connector, when configured).
    let share = share::load_config(&share::config_path())
        .map_err(|e| tracing::error!(error = %e, "share config; sharing is off"))
        .ok()
        .and_then(|cfg| {
            Share::start(cfg)
                .map_err(|e| tracing::error!(error = %e, "share listener did not start"))
                .ok()
        });
    let cell = std::cell::Cell::new(Some((engine, serve, share)));
    let mut settings = window_settings_transparent(APP_ID);
    settings.size = Size::new(1280.0, 820.0);

    iced::application(
        move || {
            let (engine, serve, share) = cell.take().expect("design boots once");
            App::new(engine, serve, share)
        },
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
    sola_kit::menu::publish(menu::app_menu());
}

/// Agent calls waiting for a reply. Nav calls are fire-and-forget.
enum Pending {
    Pick { at: CanvasRef, artboard: String, tab: u64 },
    Requery { tab: u64 },
    /// A `solactl design` call answered by the page of board `tab`.
    Call { inc: Incoming, kind: cli::CallKind, tab: u64 },
    /// A board's post-load settle: content height and loaded files, for
    /// load generation `generation`.
    Settle { tab: u64, generation: u64 },
    /// An `auto` board's page waiting to report its next height change.
    Height { tab: u64, generation: u64 },
    /// What a settled board's page declares, read again (its notes moved,
    /// or its height changed) — see [`crate::marks`].
    Marks { tab: u64, generation: u64 },
}

struct App {
    engine: CefEngine,
    /// Loopback http origin the boards load from (`None`: `file://`).
    serve: Option<Server>,
    /// Live share links (`None`: sharing did not start).
    share: Option<Share>,
    /// A short note in the status pill ("Link copied") and until when.
    notice: Option<(String, Instant)>,
    frames: Arc<BoardFrames>,
    catalog: Catalog,
    canvases: HashMap<String, Vec<canvas::Entry>>,
    /// Canvases with live boards: the one on screen and any an agent works
    /// on in the background (see [`canvases`]).
    live: Vec<canvases::Live>,
    /// Key of the canvas on screen.
    shown: Option<String>,
    /// Board tabs the helper has listed at least once. One that drops out
    /// without us closing it means the helper restarted (empty).
    listed: HashSet<u64>,
    /// The camera of the canvas on screen (off-screen ones are parked).
    camera: Camera,
    plane_size: Option<Size>,
    /// Window output scale (physical px per logical px).
    scale_factor: f32,
    /// Board surfaces are at least this many px (the plane, rounded up;
    /// only grows).
    surface_cap: (u32, u32),
    details: Details,
    fit_pending: bool,
    /// The human (or `view`) moved the camera since the canvas opened;
    /// until then an `auto` board that grows re-fits the canvas.
    user_camera: bool,
    /// The human moved the camera since the canvas opened: the view is
    /// theirs, and an agent's `view` no longer moves it.
    human_camera: bool,
    /// Shimmer over what agents touch (overlay only).
    glows: Glows,
    /// The tour on the canvas on screen (see [`tour`]).
    tour: Option<Tour>,
    /// The camera gliding to a tour step.
    tween: Option<Tween>,
    /// Note cards clicked open, and the open-every-card toggle.
    open_notes: HashSet<usize>,
    notes_open: bool,
    /// The flows list above the pill is open.
    flows_open: bool,
    /// Board with the keyboard.
    focus: Option<u64>,
    /// The window has keyboard focus.
    focused: bool,
    /// Pace last sent per board tab; a board missing here is `Live`.
    paces: HashMap<u64, BoardPace>,
    /// When each open board was last in view or working ([`budget`]).
    seen: HashMap<u64, Instant>,
    /// Discarded boards whose last picture stays on the plane.
    pictures: HashSet<u64>,
    /// Board opens and discards free memory does not show yet.
    ledger: Ledger,
    /// The last budget check found no idle boards left to discard.
    budget_short: bool,
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
    fn new(engine: CefEngine, serve: Option<Server>, share: Option<Share>) -> (Self, Task<Msg>) {
        let catalog = projects::load_from(&projects::catalog_path());
        let mut app = Self {
            engine,
            serve,
            share,
            notice: None,
            frames: BoardFrames::new(),
            catalog,
            canvases: HashMap::new(),
            live: Vec::new(),
            shown: None,
            listed: HashSet::new(),
            camera: Camera::default(),
            plane_size: None,
            scale_factor: 1.0,
            surface_cap: (0, 0),
            details: Details::default(),
            fit_pending: false,
            user_camera: false,
            human_camera: false,
            glows: Glows::default(),
            tour: None,
            tween: None,
            open_notes: HashSet::new(),
            notes_open: false,
            flows_open: false,
            focus: None,
            focused: true,
            paces: HashMap::new(),
            seen: HashMap::new(),
            pictures: HashSet::new(),
            ledger: Ledger::default(),
            budget_short: false,
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
        app.serve_projects();
        if let Some(at) = app.catalog.selected.clone() {
            app.show_canvas(at);
        }
        let scale = iced::window::latest()
            .and_then(iced::window::scale_factor)
            .map(Msg::Scale);
        (app, scale)
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
        for l in &self.live {
            subs.push(watch::subscription(
                l.dir.clone(),
                self.linked(&l.at.key()),
                Msg::CanvasChanged,
            ));
        }
        if self.has_background() {
            subs.push(iced::time::every(EVICT_TICK).map(|_| Msg::Evict));
        }
        if self.open_cost() > 0 {
            subs.push(iced::time::every(budget::BUDGET_TICK).map(|_| Msg::Budget));
        }
        if !self.agent_busy().is_empty() {
            subs.push(iced::time::every(PACE_TICK).map(|_| Msg::Pace));
        }
        if !self.waits.is_empty() || self.loads.has_due() {
            subs.push(iced::time::every(PENDING_TICK).map(|_| Msg::WaitTick));
        }
        if self.details.due.is_some() {
            subs.push(iced::time::every(DETAIL_TICK).map(|_| Msg::DetailTick));
        }
        if self.glows.is_active() || self.tween.is_some() {
            subs.push(iced::window::frames().map(|_| Msg::Animate));
        }
        subs.push(iced::event::listen_with(|event, _, _| match event {
            iced::Event::Window(iced::window::Event::Rescaled(f)) => Some(Msg::Scale(f)),
            iced::Event::Window(iced::window::Event::Focused) => Some(Msg::Focused(true)),
            iced::Event::Window(iced::window::Event::Unfocused) => Some(Msg::Focused(false)),
            _ => None,
        }));
        Subscription::batch(subs)
    }

    /// Detail renders follow the camera, plane, and boards: any change to
    /// them plans again once things rest. Board paces follow at once.
    /// Boards whose patch missed are asked to paint whole.
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        let before = (self.camera, self.plane_size, self.scale_factor);
        let task = self.handle(msg);
        if (self.camera, self.plane_size, self.scale_factor) != before {
            self.details.poke(Instant::now());
        }
        // A patch the plane could not apply: that board paints whole.
        for tab in self.frames.take_want_whole() {
            tracing::debug!(tab, "board repaint (patch missed its picture)");
            self.send(Cmd::BoardRepaint { id: TabId(tab) });
        }
        self.pace_boards();
        task
    }

    fn handle(&mut self, msg: Msg) -> Task<Msg> {
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
            Msg::Evict => self.evict_idle(Instant::now()),
            Msg::Budget => self.budget_tick(),
            Msg::Pace => {}
            Msg::Focused(focused) => self.focused = focused,
            Msg::TitleDrag => {
                return iced::window::latest().then(|id| id.map_or(Task::none(), iced::window::drag));
            }
            Msg::TitleResize(dir) => {
                return iced::window::latest().then(move |id| {
                    id.map_or(Task::none(), |id| iced::window::drag_resize(id, dir))
                });
            }
            Msg::TitleClose => sola_kit::close_app(APP_ID),
            Msg::ShareLink(rotate) => return self.copy_share_link(rotate),
            Msg::Animate => {
                let now = Instant::now();
                let loads = &self.loads;
                self.glows.tick(now, |tab| loads.is_settled(tab));
                self.glide(now);
            }
            Msg::Flows => self.flows_open = !self.flows_open,
            Msg::StartFlow(id) => self.start_flow(&id),
            Msg::ToggleNotes => self.nav(Nav::Notes),
            Msg::Tour(nav) => self.nav(nav),
            Msg::TourReturn => self.tour_return(),
            Msg::DetailTick => {
                if self.details.due.is_some_and(|due| Instant::now() >= due) {
                    self.plan_details();
                }
            }
            Msg::Scale(f) => {
                if f.is_finite() && f > 0.0 {
                    self.scale_factor = f;
                    self.grow_surfaces();
                }
            }
            Msg::Plane(ev) => return self.on_plane(ev),
            Msg::Sidebar(m) => {
                if let Some(ev) = self.rail.gestures.update(m) {
                    self.on_rail(ev);
                }
            }
            Msg::SelectCanvas(key) => {
                if let Some(at) = CanvasRef::from_key(&key) {
                    self.show_canvas(at);
                }
            }
            Msg::Canvases(mut found) => {
                for p in &self.catalog.projects {
                    if let Some(list) = found.get_mut(&p.id) {
                        p.order_canvases(list);
                    }
                }
                self.canvases = found;
                self.share_canvases();
            }
            Msg::CanvasChanged(dir, change) => {
                let key = self.live.iter().find(|l| l.dir == dir).map(|l| l.at.key());
                if let Some(key) = key {
                    return self.on_canvas_change(&key, change);
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
                    .shown_live()
                    .map(|l| l.at.project.clone())
                    .or_else(|| self.catalog.projects.first().map(|p| p.id.clone()));
                if let Some(project) = project {
                    return self.open_new_canvas(project);
                }
            }
            menu::DROP_PROJECT => {
                if let Some(project) = self.shown_live().map(|l| l.at.project.clone()) {
                    self.drop_project(&project);
                }
            }
            menu::FIT => {
                self.take_camera();
                self.fit();
            }
            menu::ACTUAL_SIZE => {
                if let Some(c) = self.plane_center() {
                    self.take_camera();
                    self.camera.set_zoom_at(c, 1.0);
                }
            }
            menu::ZOOM_IN => self.zoom_step(camera::STEP),
            menu::ZOOM_OUT => self.zoom_step(1.0 / camera::STEP),
            menu::SHARE_COPY => return self.copy_share_link(false),
            menu::SHARE_ROTATE => return self.copy_share_link(true),
            menu::RELOAD => {
                if let Some(key) = self.shown.clone() {
                    return self.reload_boards(&key);
                }
            }
            _ => {}
        }
        Task::none()
    }

    // ── plane input ───────────────────────────────────────────────

    fn on_plane(&mut self, ev: overlay::Event) -> Task<Msg> {
        match ev {
            overlay::Event::Bounds(b) => {
                self.plane_size = Some(b.size());
                self.grow_surfaces();
                if self.fit_pending {
                    self.fit();
                }
            }
            overlay::Event::Pan(d) => {
                self.take_camera();
                self.tween = None;
                self.camera.pan(d);
            }
            overlay::Event::Zoom { at, factor } => {
                self.take_camera();
                self.tween = None;
                self.camera.zoom_at(at, factor);
            }
            overlay::Event::Follow { tab, to } => self.follow(tab, &to),
            overlay::Event::ToggleNote(n) => {
                if !self.open_notes.remove(&n) {
                    self.open_notes.insert(n);
                }
            }
            overlay::Event::Nav(nav) => self.nav(nav),
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

    /// Board `tab` has a picture on the plane: painted since it opened, or
    /// kept from before it was discarded.
    fn has_picture(&self, tab: u64) -> bool {
        self.frames.is_painted(tab) || self.pictures.contains(&tab)
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
        let Some(cam) = self.shown_live().and_then(|l| l.fit_camera(view)) else {
            return;
        };
        self.camera = cam;
        self.fit_pending = false;
    }

    fn plane_center(&self) -> Option<Point> {
        self.plane_size
            .map(|s| Point::new(s.width / 2.0, s.height / 2.0))
    }

    /// Ask each board for the detail the camera needs now.
    fn plan_details(&mut self) {
        let Some(plane) = self.plane_size else {
            self.details.due = None;
            return;
        };
        // A discarded board has no page to re-raster.
        let boards: Vec<Board> = self
            .shown_boards()
            .iter()
            .filter(|b| !b.discarded)
            .cloned()
            .collect();
        let asks = self.details.refresh(
            &boards,
            &self.camera,
            plane,
            self.scale_factor,
            self.surface_cap,
        );
        for ask in asks {
            self.ask(ask);
        }
    }

    fn ask(&mut self, ask: detail::Ask) {
        match ask {
            detail::Ask::Grow { tab, size } => {
                let board = self
                    .live
                    .iter_mut()
                    .flat_map(|l| l.boards.iter_mut())
                    .find(|b| b.tab == tab);
                if let Some(b) = board {
                    b.surface = size.surface;
                    b.grown = true;
                    self.send(Cmd::SetBoard { id: tab, size });
                }
            }
            detail::Ask::View { tab, seq, detail } => {
                self.send(Cmd::BoardDetail { id: tab, seq, detail });
            }
        }
    }

    /// The plane in px, rounded up: the floor for grown board surfaces, so
    /// a detail can cover any board on screen. Grows only.
    fn grow_surfaces(&mut self) {
        let Some(plane) = self.plane_size else {
            return;
        };
        let px = |logical: f32| {
            let px = (logical * self.scale_factor).ceil().max(1.0) as u32;
            px.div_ceil(SURFACE_STEP) * SURFACE_STEP
        };
        let cap = (
            self.surface_cap.0.max(px(plane.width)),
            self.surface_cap.1.max(px(plane.height)),
        );
        if cap != self.surface_cap {
            self.surface_cap = cap;
            let keys: Vec<String> = self.live.iter().map(|l| l.at.key()).collect();
            for key in keys {
                self.settle_rasters(&key);
            }
        }
    }

    fn zoom_step(&mut self, factor: f32) {
        if let Some(c) = self.plane_center() {
            self.take_camera();
            self.camera.zoom_at(c, factor);
        }
    }

    /// The human steers the camera from here on (this canvas).
    fn take_camera(&mut self) {
        self.user_camera = true;
        self.human_camera = true;
    }

    // ── selection ─────────────────────────────────────────────────

    fn pick(&mut self, tab: u64, x: f32, y: f32) {
        let Some(live) = self.shown_live() else {
            return;
        };
        let Some(board) = live.boards.iter().find(|b| b.tab.0 == tab) else {
            return;
        };
        let why = Pending::Pick {
            at: live.at.clone(),
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
            Pending::Call { inc, kind, tab } => return self.finish_call(inc, kind, tab, reply),
            Pending::Settle { tab, generation } => return self.on_settled(tab, generation, reply),
            Pending::Height { tab, generation } => return self.on_height(tab, generation, reply),
            Pending::Marks { tab, generation } => return self.on_marks(tab, generation, reply),
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
                let current = self.shown_live().is_some_and(|l| {
                    l.at == at && l.boards.iter().any(|b| b.tab.0 == tab)
                });
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
            Pending::Call { .. }
            | Pending::Settle { .. }
            | Pending::Height { .. }
            | Pending::Marks { .. } => {
                unreachable!("answered above")
            }
        }
    }

    // ── engine plumbing ───────────────────────────────────────────

    fn send(&self, cmd: Cmd<CefEngine>) {
        if self.engine.cmd_sender().send(cmd).is_err() {
            tracing::warn!("engine command channel closed");
        }
    }

    fn agent(&mut self, tab: TabId, op: AgentOp, why: Option<Pending>) {
        // A sleeping page answers what waits on a frame only once a
        // second: wake it before the op (`pace_boards` settles its pace
        // after).
        if self.paces.get(&tab.0) == Some(&BoardPace::Asleep) {
            self.set_pace(tab, BoardPace::Live);
        }
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
                    self.show_canvas(at);
                }
            }
            SidebarEvent::ToggleSection { id } => {
                if let Some(p) = self.catalog.projects.iter_mut().find(|p| p.id == id) {
                    p.collapsed = !p.collapsed;
                    self.persist();
                }
            }
            SidebarEvent::Resize { width } => self.rail.width = width,
            SidebarEvent::Drop(drop) => self.on_rail_drop(drop),
            SidebarEvent::Edit { .. } => {}
        }
    }

    /// A canvas dropped within its project, or a project header dropped
    /// in front of another project.
    fn on_rail_drop(&mut self, drop: SidebarDrop) {
        let moved = match drop.dest {
            Dest::Join { before, .. } => {
                let Some(at) = CanvasRef::from_key(&drop.id) else {
                    return;
                };
                let before = before.as_deref().and_then(CanvasRef::from_key);
                if before.as_ref().is_some_and(|b| b.project != at.project) {
                    return;
                }
                let listed: Vec<String> = self
                    .canvases
                    .get(&at.project)
                    .into_iter()
                    .flatten()
                    .map(|c| c.name.clone())
                    .collect();
                self.catalog.move_canvas_before(
                    &at.project,
                    &at.canvas,
                    before.as_ref().map(|b| b.canvas.as_str()),
                    &listed,
                )
            }
            // `before` is a project id, or a canvas row (its project).
            Dest::BlockBefore { before } => {
                let before = before.map(|b| match CanvasRef::from_key(&b) {
                    Some(at) => at.project,
                    None => b,
                });
                self.catalog.move_project_before(&drop.id, before.as_deref())
            }
            Dest::Loose { .. } | Dest::BeforeGroup { .. } | Dest::Sections(_) => return,
        };
        match moved {
            Ok(()) => {
                self.persist();
                self.rescan();
            }
            Err(e) => tracing::warn!(error = %e, "design rail drop"),
        }
    }

    fn rescan(&mut self) {
        self.canvases = self
            .catalog
            .projects
            .iter()
            .map(|p| (p.id.clone(), p.canvases()))
            .collect();
        self.share_canvases();
    }

    /// Every canvas the rail lists has a share link.
    fn share_canvases(&self) {
        let Some(share) = &self.share else {
            return;
        };
        let all = self.catalog.projects.iter().flat_map(|p| {
            self.canvases
                .get(&p.id)
                .into_iter()
                .flatten()
                .map(move |c| {
                    let key = CanvasRef {
                        project: p.id.clone(),
                        canvas: c.name.clone(),
                    }
                    .key();
                    (key, p.root.clone(), canvas::canvas_dir(&p.root, &c.name))
                })
        });
        share.set_canvases(all);
    }

    /// Copy the shown canvas's share link (`rotate`: a new one first).
    fn copy_share_link(&mut self, rotate: bool) -> Task<Msg> {
        let (Some(share), Some(key)) = (&self.share, self.shown.clone()) else {
            return Task::none();
        };
        let url = if rotate { share.rotate(&key) } else { share.url(&key) };
        let Some(url) = url else {
            return Task::none();
        };
        let note = if rotate { "New link copied" } else { "Link copied" };
        self.notice = Some((note.to_string(), Instant::now() + NOTICE));
        Task::batch([
            iced::clipboard::write(url),
            Task::perform(tokio::time::sleep(NOTICE), |()| Msg::Ignore),
        ])
    }

    fn persist(&self) {
        if let Err(e) = projects::save_to(&projects::catalog_path(), &self.catalog) {
            tracing::warn!(error = %e, "save design projects");
        }
        self.serve_projects();
    }

    /// The origin serves exactly the projects in the list.
    fn serve_projects(&self) {
        if let Some(server) = &self.serve {
            server.set_projects(
                self.catalog
                    .projects
                    .iter()
                    .map(|p| (p.id.as_str(), p.root.as_path())),
            );
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
                self.show_canvas(CanvasRef {
                    project,
                    canvas: name,
                });
            }
            Err(e) => self.canvas_draft.error = Some(e),
        }
    }

    fn drop_project(&mut self, project: &str) {
        let keys: Vec<String> = self
            .live
            .iter()
            .filter(|l| l.at.project == project)
            .map(|l| l.at.key())
            .collect();
        for key in keys {
            self.close_live(&key);
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
            &self.agent_busy(),
            &self.theme,
            plane_bg,
        );
        let main = row![rail, self.plane_view(plane_bg)];
        let body: Element<'_, Msg> =
            match rail::dialog(&self.add, &self.canvas_draft, &self.catalog.projects) {
                Some(dialog) => stack![main, dialog].into(),
                None => main.into(),
            };
        sola_kit::window_frame(
            self.float.chrome_any(),
            "Design",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body,
        )
    }

    fn plane_view(&self, bg: Color) -> Element<'_, Msg> {
        let body: Element<'_, Msg> = match self.shown_live() {
            None => hint("Pick a canvas, or add a project (⌘N)."),
            Some(open) => {
                // Every live board keeps its texture fresh (off-screen ones
                // show at once when their canvas comes up); only the shown
                // canvas draws.
                let items = self
                    .live
                    .iter()
                    .flat_map(|l| {
                        let shown = self.is_shown(&l.at.key());
                        l.boards.iter().map(move |b| (b, shown))
                    })
                    .map(|(b, shown)| {
                        let (views, detail) = self.details.plane_views(b.tab.0);
                        plane::Item {
                            tab: b.tab.0,
                            rect: b.rect(),
                            pixels: b.size().pixels(),
                            surface: b.surface,
                            views,
                            detail,
                            shown,
                        }
                    })
                    .collect();
                let textures = Shader::new(Plane {
                    frames: self.frames.clone(),
                    items,
                    camera: self.camera,
                })
                .width(Length::Fill)
                .height(Length::Fill);
                let scene = self.shown_scene();
                let sections = iced_canvas(annotate::Sections {
                    camera: self.camera,
                    scene: scene.clone(),
                })
                .width(Length::Fill)
                .height(Length::Fill);
                let overlay = Overlay {
                    scene,
                    step: self.tour.as_ref().map(Tour::step),
                    notes_open: self.notes_open,
                    open_notes: &self.open_notes,
                    camera: self.camera,
                    boards: &open.boards,
                    painted: open
                        .boards
                        .iter()
                        .map(|b| self.has_picture(b.tab.0))
                        .collect(),
                    selection: self.selection.as_ref().map(|s| (s.tab, s.picked.rect)),
                    focus: self.focus,
                    cursor: self.engine.cursor_handle().load(Ordering::Relaxed),
                    live: !self.dialog_open(),
                    glows: self.glows.lit(Instant::now()),
                };
                let overlay: Element<'_, overlay::Event> = iced_canvas(overlay)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                let overlay = crate::ime::target(overlay, self.ime_caret());
                let mut layers = stack![sections, textures, overlay.map(Msg::Plane)];
                if let Some(e) = &open.error {
                    layers = layers.push(error_chip(e));
                } else if open.boards.is_empty() {
                    let file = format!(
                        "No artboards yet. Add [[artboard]] entries to design/{}/canvas.toml.",
                        open.at.canvas
                    );
                    layers = layers.push(hint(&file));
                }
                if let Some(bar) = self.tour_bar().or_else(|| self.flows_list()) {
                    layers = layers.push(bar);
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

    /// Where the IME candidate window goes (plane-local), while a board
    /// has the keyboard: the page's caret, or near the board's corner
    /// until the page reports one.
    fn ime_caret(&self) -> Option<iced::Rectangle> {
        if self.dialog_open() {
            return None;
        }
        let board = self.shown_boards().iter().find(|b| Some(b.tab.0) == self.focus)?;
        let caret = *self.engine.ime_handle().lock().unwrap();
        let s = board.scale as f32;
        let (x, y, w, h) = if caret.w > 0 && caret.h > 0 {
            (caret.x as f32 / s, caret.y as f32 / s, caret.w as f32 / s, caret.h as f32 / s)
        } else {
            (16.0, 16.0, 1.0, 16.0)
        };
        let at = iced::Rectangle::new(
            Point::new(board.art.x + x, board.art.y + y),
            Size::new(w, h),
        );
        Some(self.camera.rect_to_screen(at))
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
        if let Some((note, until)) = &self.notice {
            if Instant::now() < *until {
                bar = bar.push(small(note.clone()));
            }
        }
        // Notes and flows, when the canvas has them; share: copy the live
        // link, or replace it.
        let mut right = 10.0;
        let annot = self.shown_live().map(|l| &l.annot);
        if annot.is_some_and(|a| !a.notes.is_empty()) {
            let tip = if self.notes_open { "Hide notes (N)" } else { "Show notes (N)" };
            bar = bar.push(pill_tool("lucide/sticky-note", tip, Msg::ToggleNotes));
            right = 3.0;
        }
        if annot.is_some_and(|a| !a.flows.is_empty()) {
            bar = bar.push(pill_tool("lucide/route", "Flows", Msg::Flows));
            right = 3.0;
        }
        if self.share.is_some() {
            bar = bar
                .push(pill_tool("lucide/link", "Copy share link", Msg::ShareLink(false)))
                .push(pill_tool(
                    "lucide/refresh-cw",
                    "New share link (the old one stops working)",
                    Msg::ShareLink(true),
                ));
            right = 3.0;
        }
        let pill = container(bar)
            .padding(iced::Padding {
                top: 3.0,
                right,
                bottom: 3.0,
                left: 10.0,
            })
            .style(pill_style);
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

/// A pill tool: icon button, tip above (the pill sits on the bottom edge).
fn pill_tool<'a>(icon: &str, tip: &'static str, msg: Msg) -> Element<'a, Msg> {
    use sola_kit::components::{icon::icon_handle, popover, toolbar};
    let button = toolbar::toolbar_icon(icon_handle(icon), 14).on_press(msg);
    let tip = container(text(tip).font(fonts::ui()).size(12))
        .padding([5, 8])
        .style(popover::style);
    iced::widget::tooltip(button, tip, iced::widget::tooltip::Position::Top)
        .gap(8)
        .delay(Duration::from_millis(280))
        .into()
}

pub(super) fn pill_style(theme: &Theme) -> container::Style {
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
