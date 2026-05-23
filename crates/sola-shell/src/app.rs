//! Shell — central state for the iced shell. Bus dispatch lives in
//! `bus.rs`; per-window handlers are filled in by tasks 5-10.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sola_bus::topics::{ApplicationsConfig, Window};
use sola_kit::theme;

use crate::launcher::state::LauncherState;
use crate::menu::state::MenuCache;
use crate::menubar;
use crate::menubar::MenubarState;
use crate::switcher::state::SwitcherState;
use crate::zoning::ZoningState;

pub mod bus;

#[derive(Clone, Debug)]
pub enum WindowKind {
    Menubar,
    // Launcher, Menu, Switcher — added in Tasks 7-9.
}

#[derive(Clone, Debug)]
pub enum Msg {
    Bus(Arc<sola_bus::Message>),
    /// Fired by `iced::window::open`'s Task when a window's OS handle is ready.
    WindowOpened(WindowKind, iced::window::Id),
    /// Open the menu at the given app-menu index (0 = app-name slot).
    /// `is_system` true means the system-menu button was pressed.
    OpenMenu { index: usize, is_system: bool },
    /// Hover over a menu label — only re-opens if a different menu is already open.
    HoverMenu { index: usize },
    /// Clock subscription tick.
    ClockTick,
    /// Expire the toast for `generation` if it matches the current generation.
    ToastExpire(u64),
    Noop,
}

pub struct Shell {
    pub theme: iced::Theme,

    // iced window ids — None until the daemon opens each window.
    pub menubar_window_id: Option<iced::window::Id>,

    // Focus
    pub focused_app_id: Option<String>,
    pub focused_window_id: Option<u32>,

    // MRU (most-recently-used)
    pub mru_apps: Vec<String>,
    /// Most-recently-focused window per app, for switcher restore.
    pub mru_window_by_app: HashMap<String, u32>,

    // Window registry (from Topic::Windows — sola-river)
    pub known_windows: Vec<Window>,
    /// Maps (app_id, title) → window_id for fast lookup.
    pub window_id_by_key: HashMap<(String, String), u32>,

    // Application catalog (built-ins + user entries from Topic::Application)
    pub applications: ApplicationsConfig,

    // Menu cache (built up from Topic::SetAppMenu replays)
    pub menus: MenuCache,

    // Output geometry (from Topic::OutputGeometry; i32 matches OutputGeometry fields)
    pub output_size: Option<(i32, i32)>,

    // Per-window / per-surface state
    pub menu_open: bool,
    pub menu_anchor_x: f32,
    pub switcher: SwitcherState,
    pub launcher: LauncherState,
    pub zoning: ZoningState,

    // Menubar state (clock, toast, label positions)
    pub menubar: MenubarState,

    // Focus-hover generation counter (replaces legacy AppRuntimeHandle pattern).
    // Incremented on every schedule_focus_from_pointer call so stale timer
    // callbacks can detect they've been superseded.
    pub pending_focus_generation: u64,
}

impl Shell {
    /// Boot the daemon: initialise state and immediately open the menubar window.
    /// Returns `(Self, Task<Msg>)` — the Task opens the menubar window and
    /// maps the resulting `window::Id` into `Msg::WindowOpened(Menubar, id)`.
    pub fn boot() -> (Self, iced::Task<Msg>) {
        let theme = theme::default_theme();

        // Seed Topic::Theme at startup so other kit apps have a sticky value
        // to replay against on connect. main() installs BusSetup before iced
        // starts, so the bus lock is safe here.
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let bus_theme = theme::to_bus_theme();
            let _ = bus.emit(sola_bus::topics::Topic::Theme(bus_theme));
        }

        // Pre-allocate the menubar window id and produce the open task.
        let (menubar_id, open_task) = menubar::open_window();
        let task = open_task.map(|id| Msg::WindowOpened(WindowKind::Menubar, id));

        let state = Self {
            theme,
            menubar_window_id: Some(menubar_id),
            focused_app_id: None,
            focused_window_id: None,
            mru_apps: Vec::new(),
            mru_window_by_app: HashMap::new(),
            known_windows: Vec::new(),
            window_id_by_key: HashMap::new(),
            applications: ApplicationsConfig { apps: sola_core::applications::builtin_apps() },
            menus: MenuCache::new(),
            output_size: None,
            menu_open: false,
            menu_anchor_x: 0.0,
            switcher: SwitcherState::default(),
            launcher: LauncherState::default(),
            zoning: ZoningState::new(),
            menubar: MenubarState::new(),
            pending_focus_generation: 0,
        };

        (state, task)
    }

    pub fn title(&self, _window: iced::window::Id) -> String {
        "sola-shell".to_string()
    }

    pub fn theme(&self, _window: iced::window::Id) -> iced::Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> iced::Subscription<Msg> {
        use iced::time;
        iced::Subscription::batch([
            sola_kit::app::bus_subscription().map(Msg::Bus),
            time::every(Duration::from_secs(10)).map(|_| Msg::ClockTick),
        ])
    }

    pub fn update(&mut self, msg: Msg) -> iced::Task<Msg> {
        match msg {
            Msg::Bus(arc) => self.handle_bus(&arc),
            Msg::WindowOpened(_kind, _id) => {
                // Window id was pre-allocated in boot(); the OS confirmed it.
                // Nothing else needed here for now; future tasks will store
                // launcher/menu/switcher ids similarly.
                iced::Task::none()
            }
            Msg::ClockTick => {
                self.menubar.clock_now = chrono::Local::now();
                iced::Task::none()
            }
            Msg::ToastExpire(toast_gen) => {
                self.menubar.expire_toast(toast_gen);
                iced::Task::none()
            }
            Msg::OpenMenu { index, is_system: _ } => {
                // TODO Task 7: open the menu window at the correct anchor.
                self.menu_open = true;
                let _ = index;
                iced::Task::none()
            }
            Msg::HoverMenu { index } => {
                // Only re-open if a *different* menu is already open.
                if self.menu_open {
                    // TODO Task 7: switch to the hovered menu.
                    let _ = index;
                }
                iced::Task::none()
            }
            Msg::Noop => iced::Task::none(),
        }
    }

    /// Per-window view dispatch — called by the daemon for each dirty window.
    pub fn view(&self, window: iced::window::Id) -> iced::Element<'_, Msg> {
        if Some(window) == self.menubar_window_id {
            return menubar::view::view(self);
        }
        // Fallback for any window we don't recognise yet (shouldn't happen
        // under normal operation, but prevents a panic).
        iced::widget::container(iced::widget::text(""))
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into()
    }
}
