//! Shell — central state for the iced shell. Bus dispatch lives in
//! `bus.rs`; per-window handlers are filled in by tasks 5-10.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use sola_bus::topics::{
    ApplicationsConfig, CompositionEntry, FocusTarget, FrameUpdate, MenuActionPayload, MenuItem,
    RegisteredChord, Topic, Window,
};
use sola_core::{KeyChord, KeyCode};
use sola_kit::theme;

use crate::keys;
use crate::launcher::state::LauncherState;
use crate::menu::state::MenuCache;
use crate::menubar;
use crate::menubar::{FlashTarget, MenubarState};
use crate::switcher::state::SwitcherState;
use crate::zoning::ZoningState;

pub mod bus;

#[derive(Clone, Debug)]
pub enum WindowKind {
    Menubar,
    Menu,
    Launcher,
    Switcher,
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
    HoverMenu { index: usize, is_system: bool },
    /// Close the currently open menu (backdrop click, focus change, Escape, etc.)
    CloseMenu,
    /// User selected a menu action: route to bus and close menu.
    MenuAction { app_id: String, action_id: String },
    /// Menubar view reports the laid-out X position of a label at `index`.
    MenuLabelPosition { index: usize, x: f32 },
    /// Clock subscription tick.
    ClockTick,
    /// New system-stats sample from the background sampler.
    StatsTick(std::sync::Arc<crate::stats::Snapshot>),
    /// Toggle the calendar dropdown (clicking the menubar clock).
    ToggleCalendar,
    /// Toggle a stat detail panel (clicking a menubar indicator).
    ToggleStatPanel(crate::stats::Metric),
    /// Step the calendar to the previous month.
    CalendarPrevMonth,
    /// Step the calendar to the next month.
    CalendarNextMonth,
    /// Reset the calendar to the current month.
    CalendarToday,
    /// Expire the toast for `generation` if it matches the current generation.
    ToastExpire(u64),
    /// End a menubar shortcut-flash for `generation` if it's still current.
    MenuFlashExpire(u64),
    // --- Launcher messages ---
    /// Open the launcher: snapshot focus, reset query, focus text input.
    OpenLauncher,
    /// Close the launcher and restore prior focus.
    CloseLauncher,
    /// Query text changed — re-filter application list.
    LauncherQuery(String),
    /// Arrow-key navigation within the filtered list.
    LauncherNav { up: bool },
    /// Launch the selected application and close the launcher.
    Launch,
    // --- Switcher messages ---
    /// Cycle switcher selection forward (next=true) or backward (next=false).
    SwitcherNav { next: bool },
    /// Hover-select: mouse entered card at `index`.
    SwitcherHover { index: usize },
    /// Confirm selection: focus the MRU window of the selected app, deactivate.
    SwitcherConfirm,
    /// Cancel without focus change: deactivate.
    SwitcherCancel,
    /// Focus-hover timer fired: raise `window_id` if `generation` still matches.
    FocusHoverFire { window_id: u32, generation: u64 },
    /// Cycle to the next window of the currently focused app (Meta+`).
    CycleAppWindows,
    Noop,
}

/// Which non-menu panel the Menu window is hosting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Calendar,
    Stat(crate::stats::Metric),
}

pub struct Shell {
    pub theme: iced::Theme,
    /// Shell-specific chrome (shell-* tokens) — colors with alpha +
    /// switcher/launcher spacing. Refreshed alongside `theme` on every
    /// Topic::Theme delivery.
    pub style: theme::ShellStyle,

    // iced window ids — None until the daemon opens each window.
    pub menubar_window_id: Option<iced::window::Id>,
    pub menu_window_id: Option<iced::window::Id>,
    pub launcher_window_id: Option<iced::window::Id>,
    pub switcher_window_id: Option<iced::window::Id>,

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
    /// True when the currently open menu is the system menu (shell's own menu),
    /// false when it's a focused-app menu.
    pub current_open_is_system: bool,
    /// Which panel (calendar or stat graph) the Menu window is hosting, when
    /// `menu_open` is true and no app/system menu dropdown is active.
    /// `None` means the window is showing a regular menu dropdown.
    pub open_panel: Option<Panel>,
    /// The month shown in the calendar — always the 1st of that month.
    pub calendar_month: chrono::NaiveDate,
    pub switcher: SwitcherState,
    pub launcher: LauncherState,
    pub zoning: ZoningState,

    // Menubar state (clock, toast, label positions)
    pub menubar: MenubarState,

    /// Latest system-stats sample for the menubar indicators + panels.
    pub stats: std::sync::Arc<crate::stats::Snapshot>,
    /// Per-metric history for the dropdown graphs (cpu, mem, net-down, net-up).
    pub cpu_hist: crate::stats::History,
    pub mem_hist: crate::stats::History,
    pub net_down_hist: crate::stats::History,
    pub net_up_hist: crate::stats::History,
    pub gpu_hist: crate::stats::History,

    // Focus-hover generation counter (replaces legacy AppRuntimeHandle pattern).
    // Incremented on every schedule_focus_from_pointer call so stale timer
    // callbacks can detect they've been superseded.
    pub pending_focus_generation: u64,
}

impl Shell {
    /// Wayland app_id / bus app_id for the shell's own surfaces.
    pub const APP_ID: &'static str = "sola-shell";

    /// Boot the daemon: initialise state and immediately open all four windows.
    /// Returns `(Self, Task<Msg>)` — the Task opens the windows and maps the
    /// resulting `window::Id` into `Msg::WindowOpened(Kind, id)`.
    pub fn boot() -> (Self, iced::Task<Msg>) {
        let theme = theme::default_theme();

        // Seed Topic::Theme only on a cold first boot (nothing persisted
        // yet), so other kit apps have a sticky value to replay against on
        // connect. On every later boot the bus has already restored the
        // user's selected theme from `theme/current.yaml` and replays it to
        // us — adopted via `on_theme`. Emitting our compile-time default here
        // unconditionally would clobber that selection: `Topic::Theme` is
        // persistent, so the emit overwrites the file, and the
        // most-recently-selected theme would be lost on startup. main()
        // installs BusSetup before iced starts, so the bus lock is safe here.
        let bus_theme = theme::to_bus_theme();
        let persisted = sola_bus::topics::Topic::Theme(bus_theme.clone()).path_for();
        if !persisted.exists() {
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                let _ = bus.emit(sola_bus::topics::Topic::Theme(bus_theme));
            }
        }

        // Pre-allocate window ids and produce open tasks for all four windows.
        let (menubar_id, menubar_task) = menubar::open_window();
        let (menu_id, menu_task) = crate::menu::open_window();
        let (launcher_id, launcher_task) = crate::launcher::open_window();
        let (switcher_id, switcher_task) = crate::switcher::open_window();
        let task = iced::Task::batch([
            menubar_task.map(|id| Msg::WindowOpened(WindowKind::Menubar, id)),
            menu_task.map(|id| Msg::WindowOpened(WindowKind::Menu, id)),
            launcher_task.map(|id| Msg::WindowOpened(WindowKind::Launcher, id)),
            switcher_task.map(|id| Msg::WindowOpened(WindowKind::Switcher, id)),
        ]);

        let state = Self {
            theme,
            style: theme::ShellStyle::default(),
            menubar_window_id: Some(menubar_id),
            menu_window_id: Some(menu_id),
            launcher_window_id: Some(launcher_id),
            switcher_window_id: Some(switcher_id),
            focused_app_id: None,
            focused_window_id: None,
            mru_apps: Vec::new(),
            mru_window_by_app: HashMap::new(),
            known_windows: Vec::new(),
            window_id_by_key: HashMap::new(),
            applications: ApplicationsConfig { apps: crate::builtins::builtin_apps() },
            menus: MenuCache::new(),
            output_size: None,
            menu_open: false,
            menu_anchor_x: 0.0,
            current_open_index: None,
            current_open_is_system: false,
            open_panel: None,
            calendar_month: crate::calendar::first_of_month(chrono::Local::now().date_naive()),
            switcher: SwitcherState::default(),
            launcher: LauncherState::default(),
            zoning: ZoningState::new(),
            menubar: MenubarState::new(),
            stats: std::sync::Arc::new(crate::stats::Snapshot::default()),
            cpu_hist: crate::stats::History::new(60),
            mem_hist: crate::stats::History::new(60),
            net_down_hist: crate::stats::History::new(60),
            net_up_hist: crate::stats::History::new(60),
            gpu_hist: crate::stats::History::new(60),
            pending_focus_generation: 0,
        };

        (state, task)
    }

    // -------------------------------------------------------------------------
    // Window lookup helpers
    // -------------------------------------------------------------------------

    /// Look up a window_id by (app_id, title). sola-river includes shell surfaces
    /// in Topic::Windows with the title set by the iced `title()` callback.
    pub fn lookup_window_id(&self, app_id: &str, title: &str) -> Option<u32> {
        self.window_id_by_key
            .get(&(app_id.to_string(), title.to_string()))
            .copied()
    }

    // -------------------------------------------------------------------------
    // Emit helpers — compute and push bus topics from current state
    // -------------------------------------------------------------------------

    /// Build the composition list (bottom to top) and emit Topic::Composition.
    ///
    /// Stack order (bottom → top):
    ///   1. Shell menubar — always at bottom.
    ///   2. App windows ordered by MRU (least recent first), per-app MRU window on top.
    ///   3. Shell overlays when active (menu, switcher, launcher — launcher on top).
    pub fn emit_composition(&self) {
        let mut entries: Vec<CompositionEntry> = Vec::new();

        // 1. Menubar — always at the bottom.
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menubar") {
            entries.push(CompositionEntry { window_id: wid });
        }

        // 2. App windows ordered by MRU (least recent first = bottom of stack).
        // Within each app, the per-app MRU window sits on top of its siblings.
        let mut seen_app_ids: HashSet<&str> = HashSet::new();
        for app_id in self.mru_apps.iter().rev() {
            if app_id.as_str() == Self::APP_ID {
                continue;
            }
            seen_app_ids.insert(app_id.as_str());
            let top_wid = self.mru_window_by_app.get(app_id).copied();
            for w in &self.known_windows {
                if w.app_id == *app_id && Some(w.window_id) != top_wid {
                    entries.push(CompositionEntry { window_id: w.window_id });
                }
            }
            if let Some(wid) = top_wid {
                if self
                    .known_windows
                    .iter()
                    .any(|w| w.window_id == wid && w.app_id == *app_id)
                {
                    entries.push(CompositionEntry { window_id: wid });
                }
            }
        }
        // Apps not yet in MRU.
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID || seen_app_ids.contains(w.app_id.as_str()) {
                continue;
            }
            entries.push(CompositionEntry { window_id: w.window_id });
        }

        // 3. Shell overlays on top when active.
        if self.menu_open {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menu") {
                entries.push(CompositionEntry { window_id: wid });
            }
        }
        if self.switcher.active {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "switcher") {
                entries.push(CompositionEntry { window_id: wid });
            }
        }
        if self.launcher.active {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "launcher") {
                entries.push(CompositionEntry { window_id: wid });
            }
        }

        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::Composition(entries));
        }
    }

    /// Which top-level menu of `app_id` contains `action_id`, as a position
    /// in its menu list (0 = the app-name slot shown as the title). `None`
    /// if the app has no cached menu or the action isn't found.
    fn menu_index_for_action(&self, app_id: &str, action_id: &str) -> Option<usize> {
        let payload = self.menus.get_menu(app_id)?;
        payload.menus.iter().position(|menu| {
            menu.items.iter().any(|item| {
                matches!(item, MenuItem::Action { id, .. } if id == action_id)
            })
        })
    }

    /// Briefly flash the menubar label that owns `(app_id, action_id)` — the
    /// macOS "command went through the menu" feedback. The shell's own actions
    /// live under the system flower; a focused app's actions map to its title
    /// (index 0) or one of its menu labels. Returns the timer task that ends
    /// the pulse, or `Task::none()` if there's no label to flash.
    fn flash_menu_action(&mut self, app_id: &str, action_id: &str) -> iced::Task<Msg> {
        let target = if app_id == Self::APP_ID {
            FlashTarget { is_system: true, index: 0 }
        } else if let Some(index) = self.menu_index_for_action(app_id, action_id) {
            FlashTarget { is_system: false, index }
        } else {
            return iced::Task::none();
        };
        let generation = self.menubar.begin_flash(target);
        iced::Task::perform(
            tokio::time::sleep(Duration::from_millis(150)),
            move |_| Msg::MenuFlashExpire(generation),
        )
    }

    /// Emit Topic::RegisteredChords based on current overlay state and focused app.
    ///
    /// Base set: shell key chords (Meta+Space, Meta+Tab, Meta+Q, Meta+Grave,
    /// Meta+Numpad{…}), focused-app menu shortcuts (meta-bound only). Bare Super_L
    /// always registered so ChordReleased fires for switcher confirm. Escape
    /// registered only while an overlay is active.
    pub fn emit_registered_chords(&self) {
        let source = self.shell_key_chords();
        let mut chords: Vec<RegisteredChord> = Vec::with_capacity(source.len() * 2 + 2);
        for c in &source {
            chords.push(keys::to_registered(c));
            // Numpad keys have a different keysym when NumLock is off;
            // register both so zoning fires regardless of NumLock state.
            if let Some(alt) = keys::to_registered_alt(c) {
                chords.push(alt);
            }
        }
        // Bare Super_L (no modifiers) so we receive ChordReleased when the user
        // lets the Super key go — used to confirm the app switcher.
        chords.push(RegisteredChord {
            keysym: keys::KEYSYM_SUPER_L,
            modifiers: 0,
        });
        // Global media keys (play/pause, mute, next/prev, volume). Bare
        // keysyms, registered unconditionally so they work regardless of
        // focus or overlay state; `on_chord` runs them via `solactl media`.
        for (keysym, _) in keys::MEDIA_KEYS {
            chords.push(RegisteredChord {
                keysym: *keysym,
                modifiers: 0,
            });
        }
        // While any overlay is active, grab Escape so the user can dismiss it
        // regardless of which surface owns input focus. Deregistered as soon as
        // the overlay closes so terminal apps keep their Escape.
        if self.launcher.active || self.switcher.active || self.menu_open {
            chords.push(RegisteredChord {
                keysym: keys::KEYSYM_ESCAPE,
                modifiers: 0,
            });
        }
        chords.sort_by_key(|c| (c.modifiers, c.keysym));
        chords.dedup();

        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::RegisteredChords(chords));
        }
    }

    /// Build the list of chords the shell wants River to grab.
    pub fn shell_key_chords(&self) -> Vec<KeyChord> {
        // Shell-own menu bindings (e.g. Exit Sola shortcut).
        let mut bindings: Vec<KeyChord> = self
            .menus
            .key_bindings_for(Self::APP_ID)
            .into_iter()
            .filter(|b| b.meta)
            .collect();

        // Focused app's menu shortcuts (meta-bound only, only while focused so
        // River doesn't grab them globally when other clients have focus).
        if let Some(focused) = self.focused_app_id.as_deref() {
            if focused != Self::APP_ID {
                bindings.extend(
                    self.menus
                        .key_bindings_for(focused)
                        .into_iter()
                        .filter(|b| b.meta),
                );
            }
        }

        // Fixed shell chords.
        bindings.push(KeyCode::TAB.meta());   // Meta+Tab → switcher
        bindings.push(KeyCode::GRAVE.meta()); // Meta+` → cycle windows of focused app
        bindings.push(KeyCode::SPACE.meta()); // Meta+Space → launcher
        bindings.push(KeyCode::Q.meta());     // Meta+Q → close focused app
        // Super+Shift+3 full output / Super+Shift+4 focused window (macOS-style).
        bindings.push(KeyCode::KEY_3.meta_shift());
        bindings.push(KeyCode::KEY_4.meta_shift());

        // Meta+Numpad zones a window.
        for &raw in crate::zoning::ZONING_KEYCODES {
            bindings.push(KeyChord {
                keycode: KeyCode::from(raw),
                ..KeyCode::TAB.meta()
            });
        }

        bindings.sort_by_key(|b| (b.keycode.raw(), b.meta, b.alt, b.ctrl, b.shift));
        bindings.dedup();
        bindings
    }

    /// Emit Topic::Frame for all shell windows and any explicitly-zoned app windows.
    ///
    /// Shell overlays are framed eagerly (even when hidden) so show/hide via
    /// Topic::Composition is a pure visibility flip with no resize lag.
    pub fn emit_all_frames(&self) {
        let mut frames: Vec<FrameUpdate> = Vec::new();

        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menubar") {
            if let Some(f) = self.zoning.menubar_frame(wid) { frames.push(f); }
        }
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "launcher") {
            if let Some(f) = self.zoning.default_app_frame(wid) { frames.push(f); }
        }
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menu") {
            if let Some(f) = self.zoning.default_app_frame(wid) { frames.push(f); }
        }
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "switcher") {
            // Full-screen transparent overlay (same as launcher/menu); the
            // switcher view centers a grid that grows to fit the open apps.
            // (Previously a fixed 800x400 frame, which clipped the grid.)
            if let Some(f) = self.zoning.default_app_frame(wid) { frames.push(f); }
        }
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID { continue; }
            // A floating window is sized once when floated; re-framing it here
            // would clobber it — Float's zone rect is 0×0, and the sola-
            // fallback below is full-screen. Leave it at the size
            // handle_key/apply_config_zone gave it.
            if self.zoning.is_floating(w.window_id) {
                continue;
            }
            if let Some(f) = self.zoning.window_frame(w.window_id) {
                frames.push(f);
            } else if w.app_id.starts_with("sola-") {
                if let Some(f) = self.zoning.default_app_frame(w.window_id) {
                    frames.push(f);
                }
            }
        }

        if !frames.is_empty() {
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                for f in frames {
                    let _ = bus.emit(Topic::Frame(f));
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // iced Application interface
    // -------------------------------------------------------------------------

    pub fn title(&self, window: iced::window::Id) -> String {
        if Some(window) == self.menubar_window_id {
            return "menubar".to_string();
        }
        if Some(window) == self.menu_window_id {
            return "menu".to_string();
        }
        if Some(window) == self.launcher_window_id {
            return "launcher".to_string();
        }
        if Some(window) == self.switcher_window_id {
            return "switcher".to_string();
        }
        Self::APP_ID.to_string()
    }

    /// Per-window chrome theme.
    ///
    /// - Menubar: permanently black background; foreground text/icons follow
    ///   the real palette.
    /// - Overlays (menu, launcher, switcher): transparent window fill so the
    ///   OS background shows through, but all non-base palette tiers remain
    ///   opaque so kit components (card, popover, button) render correctly.
    pub fn theme(&self, window: iced::window::Id) -> iced::Theme {
        if Some(window) == self.menubar_window_id {
            return theme::menubar(&self.theme, self.style.menubar_bg);
        }
        theme::overlay(&self.theme)
    }

    /// Estimate the left-edge X of the menubar element identified by
    /// `(index, is_system)`.
    ///
    /// Font-metric math, not a post-layout measurement. Good enough for
    /// anchoring a dropdown until real geometry arrives via
    /// MenuLabelPosition events.
    ///
    /// Layout:
    ///   [system-btn ~34px] [app-title: (chars×7.5)+16] [label[1]: ...] ...
    ///
    /// - `is_system=true`  → 0 (system icon is the leftmost element)
    /// - `index=0`         → SYSTEM_BTN_W (left edge of app title)
    /// - `index=n` (n≥1)   → after app title and labels [1..n]
    pub fn estimate_label_x(&self, index: usize, is_system: bool) -> f32 {
        const CHAR_WIDTH: f32 = 7.5;
        const PAD_H: f32 = 16.0; // 2×8px horizontal padding
        const SYSTEM_BTN_W: f32 = 34.0;

        if is_system {
            return 0.0;
        }

        if index == 0 {
            return SYSTEM_BTN_W;
        }

        // Width of the app title label (index 0 slot).
        let title_label = self
            .focused_app_id
            .as_deref()
            .and_then(|id| self.menus.get_menu(id))
            .and_then(|p| p.menus.first())
            .map(|m| m.label.as_str())
            .unwrap_or("");
        let title_w = title_label.len() as f32 * CHAR_WIDTH + PAD_H;

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

    /// Estimate the left-edge X of the system-stat indicator for `metric`.
    ///
    /// The stat cluster ([CPU] [GPU?] [MEM] [RX] [TX] [clock]) is right-aligned,
    /// so we walk it right-to-left from the screen edge. Font-metric math, not a
    /// post-layout measurement — good enough to drop a panel under its
    /// indicator (the caller clamps so the card never runs off-screen).
    pub fn estimate_stat_x(&self, metric: crate::stats::Metric) -> f32 {
        use crate::stats::Metric;

        // Indicator button widths (content + ITEM_PAD [2,9] = 18px).
        // Keep roughly in sync with STAT_VALUE_W / RATE_VALUE_W / CLUSTER_SPACING
        // in menubar/view.rs (chrome type, not mono).
        const STAT_W: f32 = 80.0; // CPU/GPU/MEM: label + 36px fixed value + pad
        const RATE_W: f32 = 115.0; // TX/RX: label + 78px fixed rate + pad
        const CLOCK_W: f32 = 166.0; // clock: "%H:%M %a %Y-%m-%d" + pad
        const GAP: f32 = 4.0; // cluster spacing (menubar CLUSTER_SPACING)

        let output_w = self.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
        let has_gpu = self.stats.gpu.is_some();

        // Cluster right edge sits at the screen edge; subtract leftward.
        // Order L→R: … MEM · RX · TX · clock
        let clock_left = output_w - CLOCK_W;
        let tx_left = clock_left - GAP - RATE_W;
        let rx_left = tx_left - GAP - RATE_W;
        let mem_left = rx_left - GAP - STAT_W;
        let (gpu_left, cpu_left) = if has_gpu {
            let g = mem_left - GAP - STAT_W;
            (g, g - GAP - STAT_W)
        } else {
            (mem_left, mem_left - GAP - STAT_W)
        };

        match metric {
            Metric::Cpu => cpu_left,
            Metric::Gpu => gpu_left,
            Metric::Mem => mem_left,
            Metric::Rx => rx_left,
            Metric::Tx => tx_left,
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Msg> {
        use iced::time;

        let mut subs = vec![
            sola_kit::app::bus_subscription().map(Msg::Bus),
            time::every(Duration::from_secs(10)).map(|_| Msg::ClockTick),
            crate::stats::subscription().map(Msg::StatsTick),
        ];

        // While the launcher is active, subscribe to keyboard events so
        // ArrowUp/Down, Enter, and Escape route to launcher messages.
        // Printable characters are already handled by the text input's
        // on_input callback (→ Msg::LauncherQuery). Only navigation keys
        // need explicit routing here; they would otherwise be eaten by the
        // chord-dispatch guard in on_chord that eats all chords while
        // launcher.active is true.
        if self.launcher.active {
            let kb = iced::keyboard::listen().map(|event| {
                use iced::keyboard::{Event, Key};
                use iced::keyboard::key::Named;
                match event {
                    Event::KeyPressed { key: Key::Named(Named::ArrowUp), .. } => {
                        Msg::LauncherNav { up: true }
                    }
                    Event::KeyPressed { key: Key::Named(Named::ArrowDown), .. } => {
                        Msg::LauncherNav { up: false }
                    }
                    Event::KeyPressed { key: Key::Named(Named::Enter), .. } => Msg::Launch,
                    Event::KeyPressed { key: Key::Named(Named::Escape), .. } => {
                        Msg::CloseLauncher
                    }
                    _ => Msg::Noop,
                }
            });
            subs.push(kb);
        }

        iced::Subscription::batch(subs)
    }

    pub fn update(&mut self, msg: Msg) -> iced::Task<Msg> {
        match msg {
            Msg::Bus(arc) => self.handle_bus(&arc),
            Msg::WindowOpened(_kind, _id) => {
                // Window id was pre-allocated in boot(); the OS confirmed it.
                iced::Task::none()
            }
            Msg::ClockTick => {
                self.menubar.clock_now = chrono::Local::now();
                iced::Task::none()
            }
            Msg::StatsTick(snap) => {
                self.cpu_hist.push(snap.cpu_pct);
                self.mem_hist.push(snap.mem_pct);
                self.net_down_hist.push(snap.net_down);
                self.net_up_hist.push(snap.net_up);
                if let Some(g) = snap.gpu {
                    self.gpu_hist.push(g.util);
                }
                self.stats = snap;
                iced::Task::none()
            }
            Msg::ToggleCalendar => {
                if self.menu_open && self.open_panel == Some(Panel::Calendar) {
                    // Already showing the calendar — dismiss it.
                    self.menu_open = false;
                    self.open_panel = None;
                } else {
                    // Open (or switch an app menu over to) the calendar,
                    // always starting on the current month.
                    self.menu_open = true;
                    self.open_panel = Some(Panel::Calendar);
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                    crate::stats::set_active_metric(None);
                    self.calendar_month =
                        crate::calendar::first_of_month(self.menubar.clock_now.date_naive());
                }
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::ToggleStatPanel(m) => {
                if self.menu_open && self.open_panel == Some(crate::app::Panel::Stat(m)) {
                    self.menu_open = false;
                    self.open_panel = None;
                    crate::stats::set_active_metric(None);
                } else {
                    self.menu_open = true;
                    self.open_panel = Some(crate::app::Panel::Stat(m));
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                    crate::stats::set_active_metric(Some(m));
                }
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::CalendarPrevMonth => {
                self.calendar_month = crate::calendar::prev_month(self.calendar_month);
                iced::Task::none()
            }
            Msg::CalendarNextMonth => {
                self.calendar_month = crate::calendar::next_month(self.calendar_month);
                iced::Task::none()
            }
            Msg::CalendarToday => {
                self.calendar_month =
                    crate::calendar::first_of_month(self.menubar.clock_now.date_naive());
                iced::Task::none()
            }
            Msg::ToastExpire(toast_gen) => {
                self.menubar.expire_toast(toast_gen);
                iced::Task::none()
            }
            Msg::MenuFlashExpire(flash_gen) => {
                self.menubar.expire_flash(flash_gen);
                iced::Task::none()
            }
            Msg::OpenMenu { index, is_system } => {
                // Toggle: clicking the same menubar element while its menu
                // is open dismisses it (macOS behaviour).
                let same_trigger = self.menu_open
                    && self.current_open_index == Some(index)
                    && self.current_open_is_system == is_system;
                if same_trigger {
                    self.menu_open = false;
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                    self.emit_composition();
                    self.emit_registered_chords();
                    return iced::Task::none();
                }

                self.menu_anchor_x = self
                    .menubar
                    .label_positions
                    .get(index)
                    .copied()
                    .filter(|x| *x > 0.0)
                    .unwrap_or_else(|| self.estimate_label_x(index, is_system));
                self.menu_open = true;
                self.current_open_index = Some(index);
                self.current_open_is_system = is_system;
                self.open_panel = None;
                crate::stats::set_active_metric(None);
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::HoverMenu { index, is_system } => {
                // Only acts when a menu is already open — hover-switches the
                // active dropdown to whichever label the cursor entered.
                // Hovering the same label is a no-op.
                let same = self.current_open_index == Some(index)
                    && self.current_open_is_system == is_system;
                if self.menu_open && !same {
                    self.menu_anchor_x = self
                        .menubar
                        .label_positions
                        .get(index)
                        .copied()
                        .filter(|x| *x > 0.0)
                        .unwrap_or_else(|| self.estimate_label_x(index, is_system));
                    self.current_open_index = Some(index);
                    self.current_open_is_system = is_system;
                    self.open_panel = None;
                    crate::stats::set_active_metric(None);
                }
                iced::Task::none()
            }
            Msg::CloseMenu => {
                self.menu_open = false;
                self.current_open_index = None;
                self.current_open_is_system = false;
                self.open_panel = None;
                crate::stats::set_active_metric(None);
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::MenuAction { app_id, action_id } => {
                // Restart this process only. `sola` (process manager)
                // respawns managed `sola-shell` on exit — used after
                // font/theme changes that don't require a reinstall.
                if app_id == Self::APP_ID && action_id == "restart" {
                    tracing::info!("restart shell requested via menu");
                    return iced::exit();
                }
                if let Ok(mut bus) = sola_kit::app::bus().lock() {
                    if app_id == Self::APP_ID && (action_id == "exit" || action_id == "quit") {
                        let _ = bus.emit(Topic::Shutdown);
                    } else if action_id == "_close" {
                        if let Some(ref focused) = self.focused_app_id.clone() {
                            let _ = bus.emit(Topic::CloseApp(focused.clone()));
                        }
                    } else {
                        let _ = bus.emit(Topic::MenuAction(
                            MenuActionPayload { app_id, action_id },
                        ));
                    }
                }
                self.menu_open = false;
                self.current_open_index = None;
                self.current_open_is_system = false;
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::MenuLabelPosition { index, x } => {
                if self.menubar.label_positions.len() <= index {
                    self.menubar.label_positions.resize(index + 1, 0.0);
                }
                self.menubar.label_positions[index] = x;
                iced::Task::none()
            }
            // --- Launcher ---
            Msg::OpenLauncher => {
                self.launcher.prior_focus = self.focused_window_id;
                self.launcher.active = true;
                let apps = self.applications.clone();
                self.launcher.apply_query(&apps, "");
                self.emit_composition();
                self.emit_registered_chords();
                // Emit Topic::Focus to route keyboard to the launcher surface.
                if let Some(wid) = self.lookup_window_id(Self::APP_ID, "launcher") {
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                    }
                }
                // Also focus the query input inside iced.
                iced::widget::operation::focus::<Msg>(iced::widget::Id::new(
                    crate::launcher::view::QUERY_INPUT_ID,
                ))
            }
            Msg::CloseLauncher => {
                self.launcher.active = false;
                self.emit_composition();
                self.emit_registered_chords();
                // Restore focus to the previously focused app window.
                if let Some(wid) = self.launcher.prior_focus {
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                    }
                }
                iced::Task::none()
            }
            Msg::LauncherQuery(text) => {
                let apps = self.applications.clone();
                self.launcher.apply_query(&apps, &text);
                iced::Task::none()
            }
            Msg::LauncherNav { up } => {
                let len = self.launcher.filtered_ids.len();
                if len == 0 {
                    return iced::Task::none();
                }
                if up {
                    self.launcher.selected = self.launcher.selected.saturating_sub(1);
                } else {
                    self.launcher.selected = (self.launcher.selected + 1).min(len - 1);
                }
                iced::Task::none()
            }
            Msg::Launch => {
                let app_id = self
                    .launcher
                    .filtered_ids
                    .get(self.launcher.selected)
                    .cloned();
                if let Some(ref id) = app_id {
                    if let Some(app) = self.applications.get(id) {
                        if let Ok(mut bus) = sola_kit::app::bus().lock() {
                            let _ = bus.emit(Topic::LaunchApp(
                                sola_bus::topics::LaunchAppPayload {
                                    app_id: id.clone(),
                                    command: app.command.clone(),
                                },
                            ));
                        }
                    }
                }
                self.launcher.active = false;
                self.emit_composition();
                self.emit_registered_chords();
                // Restore focus to the previously focused window.
                if let Some(wid) = self.launcher.prior_focus {
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                    }
                }
                iced::Task::none()
            }
            // --- Switcher ---
            Msg::SwitcherNav { next } => {
                if next {
                    self.switcher.select_next();
                } else {
                    self.switcher.select_prev();
                }
                iced::Task::none()
            }
            Msg::SwitcherHover { index } => {
                self.switcher.selected =
                    index.min(self.switcher.apps.len().saturating_sub(1));
                iced::Task::none()
            }
            Msg::SwitcherConfirm => {
                let app_id = self
                    .switcher
                    .selected_app_id()
                    .map(|s| s.to_string());
                self.switcher.active = false;

                if let Some(ref app_id) = app_id {
                    // Update focus and MRU.
                    self.bus_set_focus(app_id);
                    let wid = self
                        .mru_window_by_app
                        .get(app_id)
                        .copied()
                        .or_else(|| self.lookup_any_window_id(app_id));
                    if let Some(wid) = wid {
                        self.focused_window_id = Some(wid);
                        self.mru_window_by_app.insert(app_id.clone(), wid);
                        if let Ok(mut bus) = sola_kit::app::bus().lock() {
                            let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                        }
                    }
                }
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::SwitcherCancel => {
                self.switcher.active = false;
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::FocusHoverFire { window_id, generation } => {
                // Only act if the generation matches — any mouse-enter or
                // mouse-left bump cancels the pending fire.
                if generation != self.pending_focus_generation {
                    return iced::Task::none();
                }
                // Look up app_id from known_windows; skip shell surfaces.
                let app_id = self
                    .known_windows
                    .iter()
                    .find(|w| w.window_id == window_id && w.app_id != Self::APP_ID)
                    .map(|w| w.app_id.clone());
                if let Some(ref id) = app_id {
                    self.bus_set_focus(id);
                    self.focused_window_id = Some(window_id);
                    self.mru_window_by_app.insert(id.clone(), window_id);
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(sola_bus::topics::Topic::Focus(
                            sola_bus::topics::FocusTarget { window_id },
                        ));
                    }
                    self.emit_composition();
                }
                iced::Task::none()
            }
            Msg::CycleAppWindows => {
                // Find all windows of the focused app and cycle to the next.
                let Some(ref app_id) = self.focused_app_id.clone() else {
                    return iced::Task::none();
                };
                let mut app_windows: Vec<u32> = self
                    .known_windows
                    .iter()
                    .filter(|w| &w.app_id == app_id && w.app_id != Self::APP_ID)
                    .map(|w| w.window_id)
                    .collect();
                app_windows.sort(); // deterministic ordering
                if app_windows.len() <= 1 {
                    // Single window or no windows — nothing to cycle.
                    return iced::Task::none();
                }
                let current_idx = self
                    .focused_window_id
                    .and_then(|wid| app_windows.iter().position(|&w| w == wid))
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % app_windows.len();
                let next_wid = app_windows[next_idx];
                tracing::info!(
                    app_id = %app_id,
                    from = ?self.focused_window_id,
                    to = next_wid,
                    "Meta+` — cycling app window"
                );
                self.focused_window_id = Some(next_wid);
                self.mru_window_by_app.insert(app_id.clone(), next_wid);
                if let Ok(mut bus) = sola_kit::app::bus().lock() {
                    let _ = bus.emit(sola_bus::topics::Topic::Focus(
                        sola_bus::topics::FocusTarget { window_id: next_wid },
                    ));
                }
                self.emit_composition();
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
        if Some(window) == self.launcher_window_id {
            return crate::launcher::view::view(self);
        }
        if Some(window) == self.switcher_window_id {
            return crate::switcher::view::view(self);
        }
        // Fallback — shouldn't happen under normal operation.
        iced::widget::container(iced::widget::text(""))
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into()
    }
}
