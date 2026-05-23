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
    Menu,
    // Launcher, Switcher — added in Tasks 8-9.
}#[derive(Clone, Debug)]
pub enum Msg {
    Bus(Arc<sola_bus::Message>),
    /// Fired by `iced::window::open`'s Task when a window's OS handle is ready.
    WindowOpened(WindowKind, iced::window::Id),
    /// Open the menu at the given app-menu index (0 = app-name slot).
    /// `is_system` true means the system-menu button was pressed.
    OpenMenu { index: usize, is_system: bool },
    /// Hover over a menu label — only re-opens if a different menu is already open.
    HoverMenu { index: usize },
    /// Close the currently open menu (backdrop click, focus change, Escape, etc.)
    CloseMenu,
    /// User selected a menu action: route to bus and close menu.
    MenuAction { app_id: String, action_id: String },
    /// Menubar view reports the laid-out X position of a label at `index`.
    MenuLabelPosition { index: usize, x: f32 },
    /// Clock subscription tick.
    ClockTick,
    /// Expire the toast for `generation` if it matches the current generation.
    ToastExpire(u64),
    Noop,
}pub struct Shell {
    pub theme: iced::Theme,

    // iced window ids — None until the daemon opens each window.
    pub menubar_window_id: Option<iced::window::Id>,
    pub menu_window_id: Option<iced::window::Id>,

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
    /// Index of the currently open menu (menus[n] of the focused app).
    /// None when no menu is open.
    pub current_open_index: Option<usize>,
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

        // Pre-allocate window ids and produce open tasks for menubar + menu.
        let (menubar_id, menubar_task) = menubar::open_window();
        let (menu_id, menu_task) = crate::menu::open_window();
        let task = iced::Task::batch([
            menubar_task.map(|id| Msg::WindowOpened(WindowKind::Menubar, id)),
            menu_task.map(|id| Msg::WindowOpened(WindowKind::Menu, id)),
        ]);

        let state = Self {
            theme,
            menubar_window_id: Some(menubar_id),
            menu_window_id: Some(menu_id),
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
            current_open_index: None,
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

    /// Estimate the left-edge X of menu label `index` in the menubar row.
    ///
    /// This is font-metric math, not a post-layout measurement.  It gives a
    /// reasonable approximation until Task 10 wires up real geometry or a
    /// custom widget provides exact bounds.
    ///
    /// Layout is:
    ///   [system-btn ~34px] [app-title: (chars×7.5)+16] [label[1]: ...] ...
    ///
    /// Average character width at the default ~13px font size: ~7.5px.
    /// Padding per label: [2, 8] = 2×8 = 16px total horizontal.
    ///
    /// Index 0 is the app-title/system-menu (left edge ≈ 34px).
    /// Index n≥1 is menus[n] (accumulates label widths from index 1 onward).
    pub fn estimate_label_x(&self, index: usize) -> f32 {
        const CHAR_WIDTH: f32 = 7.5;
        const PAD_H: f32 = 16.0; // 2×8px horizontal padding
        const SYSTEM_BTN_W: f32 = 34.0;

        // Width of the app title label (index 0 slot).
        let title_label = self
            .focused_app_id
            .as_deref()
            .and_then(|id| self.menus.get_menu(id))
            .and_then(|p| p.menus.first())
            .map(|m| m.label.as_str())
            .unwrap_or("");
        let title_w = title_label.len() as f32 * CHAR_WIDTH + PAD_H;

        if index == 0 {
            // The system-menu/app-title slot — leftmost.
            return SYSTEM_BTN_W;
        }

        // Accumulate widths for labels [1..index].
        let app_id = self.focused_app_id.as_deref().unwrap_or("");
        let payload = self.menus.get_menu(app_id);

        let mut x = SYSTEM_BTN_W + title_w;
        for i in 1..index {
            let label_len = payload
                .and_then(|p| p.menus.get(i))
                .map(|m| m.label.len())
                .unwrap_or(6); // fallback 6 chars
            x += label_len as f32 * CHAR_WIDTH + PAD_H;
        }
        x
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
                // Prefer a previously-measured position; fall back to font-metric
                // math if label_positions hasn't been populated yet.
                self.menu_anchor_x = self
                    .menubar
                    .label_positions
                    .get(index)
                    .copied()
                    .filter(|x| *x > 0.0)
                    .unwrap_or_else(|| self.estimate_label_x(index));
                self.menu_open = true;
                self.current_open_index = Some(index);
                // TODO Task 10: emit Topic::Composition to make menu surface visible.
                iced::Task::none()
            }
            Msg::HoverMenu { index } => {
                // Hover-sweep: only switch if a *different* menu is already open.
                if self.menu_open && self.current_open_index != Some(index) {
                    self.menu_anchor_x = self
                        .menubar
                        .label_positions
                        .get(index)
                        .copied()
                        .filter(|x| *x > 0.0)
                        .unwrap_or_else(|| self.estimate_label_x(index));
                    self.current_open_index = Some(index);
                }
                iced::Task::none()
            }
            Msg::CloseMenu => {
                self.menu_open = false;
                self.current_open_index = None;
                // TODO Task 10: emit Topic::Composition to hide menu surface.
                iced::Task::none()
            }
            Msg::MenuAction { app_id, action_id } => {
                // Route to bus then close.
                if let Ok(mut bus) = sola_kit::app::bus().lock() {
                    use sola_bus::topics::Topic;
                    if app_id == "sola-shell" && action_id == "exit" {
                        let _ = bus.emit(Topic::Shutdown);
                    } else if action_id == "_close" {
                        if let Some(ref focused) = self.focused_app_id.clone() {
                            let _ = bus.emit(Topic::CloseApp(focused.clone()));
                        }
                    } else {
                        let _ = bus.emit(Topic::MenuAction(
                            sola_bus::topics::MenuActionPayload { app_id, action_id },
                        ));
                    }
                }
                self.menu_open = false;
                self.current_open_index = None;
                // TODO Task 10: emit Topic::Composition to hide menu surface.
                iced::Task::none()
            }
            Msg::MenuLabelPosition { index, x } => {
                // Grow the vec to fit if needed, then store.
                if self.menubar.label_positions.len() <= index {
                    self.menubar.label_positions.resize(index + 1, 0.0);
                }
                self.menubar.label_positions[index] = x;
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
        if Some(window) == self.menu_window_id {
            return crate::menu::view::view(self);
        }
        // Fallback for any window we don't recognise yet (shouldn't happen
        // under normal operation, but prevents a panic).
        iced::widget::container(iced::widget::text(""))
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into()
    }
}
