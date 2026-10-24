//! sola-design app: a window per project, its canvases along the top
//! ([`crate::topbar`]), and the plane.
//!
//! The plane stacks the artboard textures (wgpu shader), the overlay canvas
//! (frames, titles, pick outline, all input), and a status pill (zoom, pick).
//! Boards are CEF board tabs in one `--engine` helper. Which window shows
//! what lives in [`canvases`]; board lifecycle in [`live`]; the memory
//! budget in [`budget`]; the call owner `design` in [`cli`].

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
use iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Size, Subscription, Task, Theme,
    window,
};
use sola_browser::CefEngine;
use sola_browser::agent::{AgentOp, AgentReply, AgentRequest};
use sola_browser::cef::engine::InputEvent;
use sola_browser::engine::{BoardPace, Cmd, Engine, TabId};
use sola_bus::Message;
use sola_bus::topics::{Activity, Topic, TopicKind};
use sola_call::Incoming;
use sola_kit::app::{BusSetup, apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::APP_ID;
use crate::annotate;
use crate::boards::Board;
use crate::calls;
use crate::camera::{self, Camera};
use crate::canvas;
use crate::detail::{self, Details};
use crate::dialogs::{self, AddDraft, CanvasDraft};
use crate::frames::{self, BoardFrames};
use crate::glow::Glows;
use crate::instance;
use crate::loads::Loads;
use crate::memory::Ledger;
use crate::menu;
use crate::overlay::{self, Overlay};
use crate::plane::{self, Plane};
use crate::projects::{self, CanvasRef, Catalog};
use crate::select::{self, Selection};
use crate::serve::Server;
use crate::share::{self, Share};
use crate::topbar;
use crate::tour::{Nav, Tour, Tween};
use crate::watch;
use crate::windows::{self, DesignWindows};

/// The no-project window's title.
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
    /// A message from window's view: the window becomes the active one
    /// first (see [`activates`]).
    At(window::Id, Box<Msg>),
    /// A window's focus, scale, and close events.
    Window(window::Id, window::Event),
    /// A window's output scale.
    Scale(window::Id, f32),
    /// Titlebar: start a move (floating), the close dot, an edge resize.
    TitleDrag(window::Id),
    TitleClose(window::Id),
    TitleResize(window::Id, window::Direction),
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
    Plane(overlay::Event),
    /// Top bar: the pointer over a chip (a canvas key), and off it.
    HoverIn(String),
    HoverOut(String),
    /// A chip pressed: show its canvas; a drag starts.
    ChipPress(String),
    /// The button came up: a dragged chip takes the hovered one's place.
    ChipDrop,
    /// The wheel over a project's strip.
    BarScroll(String, iced::mouse::ScrollDelta),
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
            // Which project window the shell paired and shows.
            TopicKind::TabWindow,
            TopicKind::CloseTab,
            // River delivers a bare Super press as a chord: Super+click.
            TopicKind::Chord,
            TopicKind::ChordReleased,
        ])
        .calls(calls::OWNER, calls::methods())
        .install();

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

    iced::daemon(
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
    .run()
}

/// A message from a window's view makes that window the active one, except
/// what only reports: its plane's size, the pointer crossing a chip.
fn activates(msg: &Msg) -> bool {
    !matches!(
        msg,
        Msg::Plane(overlay::Event::Bounds(_)) | Msg::HoverIn(_) | Msg::HoverOut(_) | Msg::Ignore
    )
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
    /// A window per project, and which is active.
    windows: DesignWindows,
    /// Canvases with live boards: each window's, and any an agent works
    /// on in the background (see [`canvases`]).
    live: Vec<canvases::Live>,
    /// Key of the shown canvas: the active window's. Its view is the
    /// fields below; every other live canvas keeps its view parked.
    shown: Option<String>,
    /// Board tabs the helper has listed at least once. One that drops out
    /// without us closing it means the helper restarted (empty).
    listed: HashSet<u64>,
    /// The camera of the shown canvas (the others are parked).
    camera: Camera,
    /// The active window's plane.
    plane_size: Option<Size>,
    /// The active window's output scale (physical px per logical px).
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
    /// The active window has keyboard focus.
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
    /// Top bar: the chip under the pointer, and the one being dragged
    /// (canvas keys).
    hover_chip: Option<String>,
    chip_drag: Option<String>,
    add: AddDraft,
    canvas_draft: CanvasDraft,
    theme: Theme,
    float: sola_kit::FloatState,
    /// Tasks a call or helper started (a window to open), run after the
    /// message that started them.
    later: Vec<Task<Msg>>,
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
            windows: DesignWindows::new(APP_ID),
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
            hover_chip: None,
            chip_drag: None,
            add: AddDraft::default(),
            canvas_draft: CanvasDraft::default(),
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            later: Vec::new(),
        };
        app.rescan();
        app.serve_projects();
        let open = app.open_all_windows();
        (app, open)
    }

    /// A window's title: its project's name (the shell's bar row).
    fn title(&self, window: window::Id) -> String {
        self.windows
            .project_of(window)
            .and_then(|p| self.catalog.project(p))
            .map_or_else(|| WINDOW_TITLE.to_string(), |p| p.name.clone())
    }

    fn theme(&self, window: window::Id) -> Theme {
        sola_kit::theme_for(self.chrome(window), &self.theme)
    }

    fn chrome(&self, window: window::Id) -> sola_kit::Chrome {
        self.windows
            .river_id(window)
            .map_or(sola_kit::Chrome::Tiled, |id| self.float.chrome(id))
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
        subs.push(iced::event::listen_with(|event, _, id| match event {
            iced::Event::Window(
                e @ (window::Event::Rescaled(_)
                | window::Event::Focused
                | window::Event::Unfocused
                | window::Event::CloseRequested
                | window::Event::Closed),
            ) => Some(Msg::Window(id, e)),
            _ => None,
        }));
        if self.chip_drag.is_some() {
            subs.push(iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                    Some(Msg::ChipDrop)
                }
                _ => None,
            }));
        }
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
        self.publish_activity();
        if self.later.is_empty() {
            return task;
        }
        let mut tasks = std::mem::take(&mut self.later);
        tasks.push(task);
        Task::batch(tasks)
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
            Msg::At(window, msg) => {
                if let Msg::Plane(overlay::Event::Bounds(b)) = *msg {
                    self.on_bounds(window, b.size());
                    return Task::none();
                }
                if activates(&msg) && self.windows.get(window).is_some() {
                    self.activate(window);
                }
                return self.handle(*msg);
            }
            Msg::Window(window, event) => return self.on_window(window, event),
            Msg::Scale(window, f) => {
                if f.is_finite() && f > 0.0 {
                    if let Some(w) = self.windows.get_mut(window) {
                        w.scale = f;
                    }
                    if self.windows.active == Some(window) {
                        self.scale_factor = f;
                        self.grow_surfaces();
                    }
                }
            }
            Msg::TitleDrag(window) => return window::drag(window),
            Msg::TitleResize(window, dir) => return window::drag_resize(window, dir),
            Msg::TitleClose(window) => return self.close_window(window),
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
            Msg::Plane(ev) => return self.on_plane(ev),
            Msg::HoverIn(key) => self.hover_chip = Some(key),
            Msg::HoverOut(key) => {
                // Moving leftwards, the next chip's enter comes before this
                // exit: only clear our own hover.
                if self.hover_chip.as_deref() == Some(key.as_str()) {
                    self.hover_chip = None;
                }
            }
            Msg::ChipPress(key) => {
                if let Some(at) = CanvasRef::from_key(&key) {
                    self.chip_drag = Some(key);
                    self.show_canvas(at);
                }
            }
            Msg::ChipDrop => {
                if let (Some(from), Some(onto)) = (self.chip_drag.take(), self.hover_chip.clone()) {
                    self.drop_chip_on(&from, &onto);
                }
            }
            Msg::BarScroll(project, delta) => {
                let (dx, dy) = match delta {
                    iced::mouse::ScrollDelta::Lines { x, y } => (x * 40.0, y * 40.0),
                    iced::mouse::ScrollDelta::Pixels { x, y } => (x, y),
                };
                // Wheel down (or right) moves along to the later canvases.
                return iced::widget::operation::scroll_by(
                    topbar::strip_id(&project),
                    iced::widget::scrollable::AbsoluteOffset { x: -(dx + dy), y: 0.0 },
                );
            }
            Msg::Canvases(mut found) => {
                for p in &self.catalog.projects {
                    if let Some(list) = found.get_mut(&p.id) {
                        p.order_canvases(list);
                    }
                }
                self.canvases = found;
                self.share_canvases();
                self.wake_windows();
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
        if matches!(Topic::parse(message), Some(Topic::TabWindow(_))) {
            self.windows.update(message);
            self.wake_windows();
            return Task::none();
        }
        // The tab bar's stoplight: as the window's own close.
        if let Some(window) = self.windows.close_request(message) {
            return self.on_window(window, window::Event::CloseRequested);
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

    /// The active window's project (not the no-project window).
    fn active_project(&self) -> Option<String> {
        self.windows
            .active
            .and_then(|w| self.windows.project_of(w))
            .filter(|p| !p.is_empty())
            .map(str::to_string)
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            menu::ADD_PROJECT => return self.open_add(),
            menu::NEW_CANVAS => {
                let project = self
                    .active_project()
                    .or_else(|| self.catalog.projects.first().map(|p| p.id.clone()));
                if let Some(project) = project {
                    return self.open_new_canvas(project);
                }
            }
            menu::DROP_PROJECT => {
                if let Some(project) = self.active_project() {
                    return self.drop_project(&project);
                }
            }
            menu::CLOSE_WINDOW => {
                if let Some(window) = self.windows.active {
                    return self.close_window(window);
                }
            }
            open if open.starts_with(menu::OPEN_PREFIX) => {
                let project = open[menu::OPEN_PREFIX.len()..].to_string();
                return self.open_window(&project, false);
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

    // ── windows ───────────────────────────────────────────────────

    /// Open a window for every project (the one last shown in front), or
    /// the no-project window when there are none.
    fn open_all_windows(&mut self) -> Task<Msg> {
        let ids: Vec<String> = self.catalog.projects.iter().map(|p| p.id.clone()).collect();
        if ids.is_empty() {
            return self.open_window(windows::EMPTY, false);
        }
        let front = self
            .catalog
            .selected
            .as_ref()
            .map(|s| s.project.clone())
            .filter(|p| ids.contains(p))
            .unwrap_or_else(|| ids[0].clone());
        let tasks: Vec<Task<Msg>> = ids
            .iter()
            .map(|id| self.open_window(id, *id != front))
            .collect();
        Task::batch(tasks)
    }

    /// Open project `project`'s window, or bring it up when it is open. A
    /// window opened in front becomes the active one. The no-project
    /// window goes once a project has one.
    pub(super) fn open_window(&mut self, project: &str, background: bool) -> Task<Msg> {
        if let Some(window) = self.windows.window_of(project) {
            if !background {
                self.activate(window);
                self.windows.show(window);
            }
            return Task::none();
        }
        let mut tasks = Vec::new();
        if !project.is_empty() {
            if let Some(empty) = self.windows.window_of(windows::EMPTY) {
                self.windows.forget(empty);
                tasks.push(window::close(empty));
            }
        }
        let (window, open) = self.windows.open(APP_ID, project, background);
        tasks.push(open.then(|id| window::scale_factor(id).map(move |f| Msg::Scale(id, f))));
        if !background || self.windows.active.is_none() {
            self.activate(window);
        }
        self.publish_menu();
        Task::batch(tasks)
    }

    /// Close a window. Its project stays (Project → Open brings it back);
    /// its canvas leaves the screen. The last window closing quits.
    fn close_window(&mut self, window: window::Id) -> Task<Msg> {
        self.forget_window(window);
        if self.windows.is_empty() {
            return Task::batch([window::close(window), iced::exit()]);
        }
        window::close(window)
    }

    /// A window is going (or gone under us): its canvas leaves the screen,
    /// and another window takes input.
    fn forget_window(&mut self, window: window::Id) {
        let canvas = self.canvas_of(window).map(|c| c.key());
        let Some(project) = self.windows.forget(window) else {
            return;
        };
        tracing::info!(%project, "design window closed");
        if let Some(key) = canvas {
            self.leave_screen(&key);
        }
        if self.windows.active.is_none() {
            let next = self.windows.iter().map(|(w, _)| w).next();
            if let Some(next) = next {
                self.activate(next);
            }
        }
        self.publish_menu();
    }

    fn on_window(&mut self, window: window::Id, event: window::Event) -> Task<Msg> {
        match event {
            window::Event::Focused => {
                if self.windows.get(window).is_some() {
                    self.activate(window);
                    self.focused = true;
                    // Its project's window opens in front next time.
                    if let Some(at) = self.canvas_of(window) {
                        if self.catalog.selected.as_ref() != Some(&at) {
                            self.catalog.select(&at);
                            self.persist();
                        }
                    }
                }
            }
            window::Event::Unfocused => {
                if self.windows.active == Some(window) {
                    self.focused = false;
                }
            }
            window::Event::Rescaled(f) => return self.handle(Msg::Scale(window, f)),
            window::Event::CloseRequested => return self.close_window(window),
            window::Event::Closed => {
                if self.windows.get(window).is_some() {
                    self.forget_window(window);
                }
                if self.windows.is_empty() {
                    return iced::exit();
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// A window's plane laid out at `size`. The active window's is the
    /// app's; another's re-fits its parked canvas until the human frames it.
    fn on_bounds(&mut self, window: window::Id, size: Size) {
        if let Some(w) = self.windows.get_mut(window) {
            w.plane = Some(size);
        }
        if self.windows.active == Some(window) {
            self.plane_size = Some(size);
            self.grow_surfaces();
            if self.fit_pending {
                self.fit();
            }
        } else if let Some(key) = self.canvas_of(window).map(|c| c.key()) {
            // A first view, fit; one the human has not framed, fit again.
            let _ = self.camera_of(&key);
            self.refit_parked(&key);
        }
    }

    /// Windows the shell shows (and the active one) have their canvas live;
    /// a window opened behind waits until it comes up.
    fn wake_windows(&mut self) {
        let want: Vec<CanvasRef> = self
            .windows
            .iter()
            .map(|(w, _)| w)
            .filter(|w| {
                self.windows.active == Some(*w)
                    || (self.windows.river_id(*w).is_some() && self.windows.shown(*w))
            })
            .filter_map(|w| self.canvas_of(w))
            .collect();
        for at in want {
            if let Some(key) = self.ensure_live(&at) {
                // A view to draw it with until it is shown.
                let _ = self.camera_of(&key);
            }
        }
        // The active window's canvas may have changed under it (a canvas
        // deleted, the first one made).
        if let Some(active) = self.windows.active {
            let key = self.canvas_of(active).map(|c| c.key());
            if key != self.shown {
                self.activate(active);
            }
        }
    }

    /// The app menu, with Project → Open for each project without a window.
    fn publish_menu(&self) {
        let closed: Vec<(String, String)> = self
            .catalog
            .projects
            .iter()
            .filter(|p| !self.windows.has(&p.id))
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        sola_kit::menu::publish(menu::app_menu(&closed));
    }

    /// Each window's row in the shell's bar rings while an agent works in
    /// one of its project's canvases.
    fn publish_activity(&mut self) {
        let busy = self.agent_busy();
        let rows: Vec<(window::Id, Activity)> = self
            .windows
            .iter()
            .map(|(w, win)| {
                let working = busy
                    .iter()
                    .filter_map(|k| CanvasRef::from_key(k))
                    .any(|at| at.project == win.project);
                (w, if working { Activity::Working } else { Activity::Idle })
            })
            .collect();
        for (w, activity) in rows {
            self.windows.set_activity(w, activity);
        }
    }

    // ── plane input ───────────────────────────────────────────────

    fn on_plane(&mut self, ev: overlay::Event) -> Task<Msg> {
        match ev {
            // Per window: see `on_bounds`.
            overlay::Event::Bounds(_) => {}
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

    /// A second launch: bring the active window up.
    fn raise(&self) {
        let window = self
            .windows
            .active
            .or_else(|| self.windows.iter().map(|(w, _)| w).next());
        if let Some(window) = window {
            self.windows.show(window);
        }
    }

    // ── top bar + dialogs ─────────────────────────────────────────

    /// Chip `from` was dropped on chip `onto` (canvas keys): it takes that
    /// place in its project.
    fn drop_chip_on(&mut self, from: &str, onto: &str) {
        let (Some(from), Some(onto)) = (CanvasRef::from_key(from), CanvasRef::from_key(onto)) else {
            return;
        };
        if from.project != onto.project {
            return;
        }
        let listed: Vec<String> = self
            .canvases
            .get(&from.project)
            .into_iter()
            .flatten()
            .map(|c| c.name.clone())
            .collect();
        let Some(before) = projects::drop_place(&listed, &from.canvas, &onto.canvas) else {
            return;
        };
        match self
            .catalog
            .move_canvas_before(&from.project, &from.canvas, before.as_deref(), &listed)
        {
            Ok(()) => {
                self.persist();
                self.rescan();
            }
            Err(e) => tracing::warn!(error = %e, "design canvas drop"),
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

    /// The window an open dialog shows in: New Canvas in its project's
    /// window, Add Project in the active one.
    fn dialog_window(&self) -> Option<window::Id> {
        if let Some(project) = &self.canvas_draft.project {
            return self.windows.window_of(project);
        }
        self.add.open.then_some(self.windows.active).flatten()
    }

    fn open_add(&mut self) -> Task<Msg> {
        self.canvas_draft = CanvasDraft::default();
        self.add = AddDraft {
            open: true,
            ..AddDraft::default()
        };
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(dialogs::ADD_INPUT_ID))
    }

    /// Add the folder in the dialog; its window opens in front.
    fn add_project(&mut self) -> Task<Msg> {
        match self.catalog.add(&self.add.path) {
            Ok(id) => {
                tracing::info!(project = %id, "added design project");
                self.add = AddDraft::default();
                self.persist();
                self.rescan();
                return self.open_window(&id, false);
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
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(dialogs::CANVAS_INPUT_ID))
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

    /// Unregister a project (its files stay): its window and canvases
    /// close. With no project left, the no-project window opens.
    fn drop_project(&mut self, project: &str) -> Task<Msg> {
        let mut tasks = Vec::new();
        if let Some(window) = self.windows.window_of(project) {
            self.forget_window(window);
            tasks.push(window::close(window));
        }
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
        if self.windows.is_empty() {
            tasks.push(self.open_window(windows::EMPTY, false));
        }
        self.publish_menu();
        Task::batch(tasks)
    }

    // ── view ──────────────────────────────────────────────────────

    /// A project window: its canvases along the top, then the plane of the
    /// one it shows. Every message from it activates it first.
    fn view(&self, window: window::Id) -> Element<'_, Msg> {
        let plane_bg = self.theme.extended_palette().background.base.color;
        let project = self.windows.project_of(window).unwrap_or(windows::EMPTY);
        let body: Element<'_, Msg> = if project.is_empty() {
            dialogs::no_project()
        } else {
            let at = self.canvas_of(window);
            let bar = topbar::view(topbar::Bar {
                project,
                canvases: self.canvases.get(project).map_or(&[], Vec::as_slice),
                selected: at.as_ref().map(|a| a.canvas.clone()),
                hovered: self.hover_chip.as_deref(),
                dragging: self.chip_drag.as_deref(),
                busy: self.agent_busy(),
            });
            column![
                bar,
                sola_kit::components::horizontal_divider(),
                self.plane_view(window, at, plane_bg),
            ]
            .into()
        };
        let dialog = (self.dialog_window() == Some(window))
            .then(|| dialogs::dialog(&self.add, &self.canvas_draft, &self.catalog.projects))
            .flatten();
        let body: Element<'_, Msg> = match dialog {
            Some(dialog) => stack![body, dialog].into(),
            None => body,
        };
        sola_kit::window_frame(
            self.chrome(window),
            self.title(window),
            Msg::TitleDrag(window),
            Msg::TitleClose(window),
            move |dir| Msg::TitleResize(window, dir),
            body,
        )
        .map(move |msg| Msg::At(window, Box::new(msg)))
    }

    /// What a window draws of canvas `key`: the shown canvas's view (the
    /// app's), else its parked one — fit to `plane` when it has none yet.
    fn view_of(&self, key: &str, plane: Option<Size>) -> Option<View<'_>> {
        if self.is_shown(key) {
            return Some(View {
                camera: self.camera,
                selection: self.selection.as_ref(),
                tour: self.tour.as_ref(),
                open_notes: &self.open_notes,
                notes_open: self.notes_open,
                active: true,
            });
        }
        let live = self.live(key)?;
        Some(match &live.parked {
            Some(p) => View {
                camera: p.camera,
                selection: p.selection.as_ref(),
                tour: p.tour.as_ref(),
                open_notes: &p.open_notes,
                notes_open: p.notes_open,
                active: false,
            },
            None => View {
                camera: plane.and_then(|v| live.fit_camera(v)).unwrap_or_default(),
                selection: None,
                tour: None,
                open_notes: &EMPTY_NOTES,
                notes_open: false,
                active: false,
            },
        })
    }

    fn plane_view(&self, window: window::Id, at: Option<CanvasRef>, bg: Color) -> Element<'_, Msg> {
        let key = at.as_ref().map(CanvasRef::key);
        let plane = self.windows.get(window).and_then(|w| w.plane);
        let open = key.as_deref().and_then(|k| Some((self.live(k)?, self.view_of(k, plane)?)));
        let body: Element<'_, Msg> = match (at, open) {
            (None, _) => hint("No canvases yet. + makes one (⌘⇧N)."),
            (Some(_), None) => hint("Opening…"),
            (Some(_), Some((open, view))) => {
                // Every live board keeps its texture fresh (one shader
                // pipeline serves every window, and a canvas shows at once
                // when it comes up); only this window's canvas draws.
                let items = self
                    .live
                    .iter()
                    .flat_map(|l| {
                        let shown = l.at == open.at;
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
                    camera: view.camera,
                })
                .width(Length::Fill)
                .height(Length::Fill);
                let scene = crate::scene::Scene::build(&open.boards, &open.annot);
                let sections = iced_canvas(annotate::Sections {
                    camera: view.camera,
                    scene: scene.clone(),
                })
                .width(Length::Fill)
                .height(Length::Fill);
                let overlay = Overlay {
                    scene,
                    step: view.tour.map(Tour::step),
                    notes_open: view.notes_open,
                    open_notes: view.open_notes,
                    camera: view.camera,
                    boards: &open.boards,
                    painted: open
                        .boards
                        .iter()
                        .map(|b| self.has_picture(b.tab.0))
                        .collect(),
                    selection: view.selection.map(|s| (s.tab, s.picked.rect)),
                    focus: if view.active { self.focus } else { None },
                    cursor: self.engine.cursor_handle().load(Ordering::Relaxed),
                    live: !self.dialog_open(),
                    glows: self.glows.lit(Instant::now()),
                };
                let overlay: Element<'_, overlay::Event> = iced_canvas(overlay)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                let caret = if view.active { self.ime_caret() } else { None };
                let overlay = crate::ime::target(overlay, caret);
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
                if view.active {
                    if let Some(bar) = self.tour_bar().or_else(|| self.flows_list()) {
                        layers = layers.push(bar);
                    }
                }
                layers.push(self.status_bar(open, &view)).into()
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

    /// Zoom and the pick of a window's canvas, floating at the bottom of
    /// its plane.
    fn status_bar(&self, open: &canvases::Live, view: &View<'_>) -> Element<'_, Msg> {
        let small = |t: String| {
            text(t)
                .size(12)
                .font(fonts::mono())
                .style(sola_kit::components::text::muted)
        };
        let mut bar = row![small(format!("{:.0}%", view.camera.zoom * 100.0))]
            .spacing(10)
            .align_y(Alignment::Center);
        if let Some(sel) = view.selection {
            bar = bar.push(small(sel.label()));
        }
        if let Some((note, until)) = self.notice.as_ref().filter(|_| view.active) {
            if Instant::now() < *until {
                bar = bar.push(small(note.clone()));
            }
        }
        // Notes and flows, when the canvas has them; share: copy the live
        // link, or replace it.
        let mut right = 10.0;
        let annot = &open.annot;
        if !annot.notes.is_empty() {
            let tip = if view.notes_open { "Hide notes (N)" } else { "Show notes (N)" };
            bar = bar.push(pill_tool("lucide/sticky-note", tip, Msg::ToggleNotes));
            right = 3.0;
        }
        if !annot.flows.is_empty() {
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

/// A window's view of its canvas (see [`App::view_of`]).
struct View<'a> {
    camera: Camera,
    selection: Option<&'a Selection>,
    tour: Option<&'a Tour>,
    open_notes: &'a HashSet<usize>,
    notes_open: bool,
    /// The shown canvas: the active window's.
    active: bool,
}

/// No note open (a canvas without a view yet).
static EMPTY_NOTES: std::sync::LazyLock<HashSet<usize>> = std::sync::LazyLock::new(HashSet::new);

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
