//! Shell — central state for the iced shell. Bus dispatch lives in
//! `bus.rs`; per-window handlers are filled in by tasks 5-10.

use std::collections::HashMap;
use std::sync::Arc;

use iced::widget::{container, text};
use iced::{Element, Length, Subscription};

use sola_bus::topics::{ApplicationsConfig, Window};
use sola_kit::theme;

use crate::launcher::state::LauncherState;
use crate::menu::state::MenuCache;
use crate::switcher::state::SwitcherState;
use crate::zoning::ZoningState;

pub mod bus;

#[derive(Clone, Debug)]
pub enum Msg {
    Bus(Arc<sola_bus::Message>),
    Noop,
}

pub struct Shell {
    pub theme: iced::Theme,

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

    // Focus-hover generation counter (replaces legacy AppRuntimeHandle pattern).
    // Incremented on every schedule_focus_from_pointer call so stale timer
    // callbacks can detect they've been superseded.
    pub pending_focus_generation: u64,
}

impl Shell {
    pub fn default() -> Self {
        let theme = theme::default_theme();

        // Seed Topic::Theme at startup so other kit apps have a sticky value
        // to replay against on connect. main() installs BusSetup before iced
        // starts, so the bus lock is safe here.
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let bus_theme = theme::to_bus_theme();
            let _ = bus.emit(sola_bus::topics::Topic::Theme(bus_theme));
        }

        Self {
            theme,
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
            pending_focus_generation: 0,
        }
    }

    pub fn title(&self) -> String {
        "sola-shell".to_string()
    }

    pub fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        sola_kit::app::bus_subscription().map(Msg::Bus)
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Bus(arc) => self.handle_bus(&arc),
            Msg::Noop => {}
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        container(text("sola-shell (iced) — skeleton"))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }
}
