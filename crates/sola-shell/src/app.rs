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
use crate::selection::state::SelectionState;
use crate::switcher::state::SwitcherState;
use crate::zoning::ZoningState;

pub mod bus;

#[derive(Clone, Debug)]
pub enum WindowKind {
    Menubar,
    Menu,
    Launcher,
    Switcher,
    Selection,
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
    /// Also clears a matching pending launch (opening feedback timeout).
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
    /// Launch a specific app by id (row click — not the keyboard selection).
    LaunchApp(String),
    /// Menubar chip: unhide a composition-hidden app (AppHidden retract) and raise it.
    UnhideApp(String),
    // --- Switcher messages ---
    /// Cycle switcher selection forward (next=true) or backward (next=false).
    SwitcherNav { next: bool },
    /// Hover-select: mouse entered card at `index`.
    SwitcherHover { index: usize },
    /// Confirm selection: focus the MRU window of the selected app, deactivate.
    SwitcherConfirm,
    /// Cancel without focus change: deactivate.
    SwitcherCancel,
    /// Focus-follows-mouse delay fired: focus `window_id` (no raise) if
    /// `generation` still matches.
    FocusHoverFire { window_id: u32, generation: u64 },
    /// Cycle to the next window of the currently focused app (Meta+`).
    CycleAppWindows,
    /// Super+Shift+4: open the selection marquee overlay.
    OpenSelection,
    /// Escape / cancel selection without capturing.
    CloseSelection,
    /// Pointer down on the selection overlay (compositor-space coords).
    SelectionPress { x: f32, y: f32 },
    /// Pointer move while dragging a selection.
    SelectionMove { x: f32, y: f32 },
    /// Pointer up — finish region capture if large enough.
    SelectionRelease { x: f32, y: f32 },
    Noop,
}

/// Which non-menu panel the Menu window is hosting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Calendar,
    Stat(crate::stats::Metric),
}

/// In-flight launcher spawn waiting for a first matching window (or timeout).
///
/// Shows a menubar toast (`Opening {label}…`) so slow cold starts are not
/// silent. Cleared when a new window with a matching `app_id` appears, on
/// launch failure / early exit, or when the toast generation expires.
#[derive(Debug, Clone)]
pub struct PendingLaunch {
    /// Catalog `app_id` from the launcher entry.
    pub app_id: String,
    /// `MenubarState::toast_generation` at the moment the opening toast was
    /// pushed — used to ignore stale timeouts and clear only our toast.
    pub toast_generation: u64,
    /// Window ids already present for this app at launch time. A resolve
    /// requires a **new** window so re-launching an already-open app does not
    /// dismiss against the existing surface.
    pub existing_wids: HashSet<u32>,
}

/// How long the "Opening …" toast stays up if no matching window appears.
const OPENING_TOAST_SECS: u64 = 20;

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
    pub selection_window_id: Option<iced::window::Id>,

    // Focus
    pub focused_app_id: Option<String>,
    pub focused_window_id: Option<u32>,
    /// Window currently under the pointer (`Topic::MouseEntered` /
    /// `MouseLeft`). Used to re-apply focus-follows-mouse after programmatic
    /// focus steals (new map, close fallback) — River does not re-send
    /// `pointer_enter` if the cursor never left the old surface.
    pub pointer_window_id: Option<u32>,
    /// Generation counter for the focus-follows-mouse dwell timer. Bumped on
    /// every enter/leave so a superseded `FocusHoverFire` is a no-op — gives
    /// a short grace period when mousing across apps toward the menubar.
    pub pending_focus_generation: u64,

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

    /// Apps omitted from composition (River `hide`). Keyed lowercased for
    /// case-insensitive match; value is the original app_id for menubar labels
    /// and AppHidden retract. Filled from sticky `Topic::AppHidden`.
    pub hidden_apps: HashMap<String, String>,

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
    pub selection: SelectionState,
    /// When true, the next `Topic::Screenshot` from sola-river should
    /// open/raise sola-preview. Set only by shell hotkey / selection paths.
    pub open_preview_on_next: bool,
    /// Window that should keep keyboard after a shell-initiated capture
    /// finishes (preview is raised without stealing focus). Set when a
    /// Super+Shift+3/4/5 capture starts; applied after open/raise preview.
    pub screenshot_return_focus: Option<u32>,
    /// When `Some(app_id)`, the next `on_windows` "new app mapped" focus
    /// steal for that app is skipped (screenshot cold-launch of preview
    /// must not yank the keyboard off the pre-capture app).
    pub suppress_map_focus_for: Option<String>,
    pub zoning: ZoningState,

    // Menubar state (clock, toast, label positions)
    pub menubar: MenubarState,

    /// Launcher spawn waiting for a matching window, if any.
    pub pending_launch: Option<PendingLaunch>,

    /// Latest system-stats sample for the menubar indicators + panels.
    pub stats: std::sync::Arc<crate::stats::Snapshot>,
    /// Per-metric history for the dropdown graphs (cpu, mem, net-down, net-up).
    pub cpu_hist: crate::stats::History,
    pub mem_hist: crate::stats::History,
    pub net_down_hist: crate::stats::History,
    pub net_up_hist: crate::stats::History,
    pub gpu_hist: crate::stats::History,
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

        // Pre-allocate window ids and produce open tasks for all shell surfaces.
        let (menubar_id, menubar_task) = menubar::open_window();
        let (menu_id, menu_task) = crate::menu::open_window();
        let (launcher_id, launcher_task) = crate::launcher::open_window();
        let (switcher_id, switcher_task) = crate::switcher::open_window();
        let (selection_id, selection_task) = crate::selection::open_window();
        let task = iced::Task::batch([
            menubar_task.map(|id| Msg::WindowOpened(WindowKind::Menubar, id)),
            menu_task.map(|id| Msg::WindowOpened(WindowKind::Menu, id)),
            launcher_task.map(|id| Msg::WindowOpened(WindowKind::Launcher, id)),
            switcher_task.map(|id| Msg::WindowOpened(WindowKind::Switcher, id)),
            selection_task.map(|id| Msg::WindowOpened(WindowKind::Selection, id)),
        ]);

        let state = Self {
            theme,
            style: theme::ShellStyle::default(),
            menubar_window_id: Some(menubar_id),
            menu_window_id: Some(menu_id),
            launcher_window_id: Some(launcher_id),
            switcher_window_id: Some(switcher_id),
            selection_window_id: Some(selection_id),
            focused_app_id: None,
            focused_window_id: None,
            pointer_window_id: None,
            pending_focus_generation: 0,
            mru_apps: Vec::new(),
            mru_window_by_app: HashMap::new(),
            known_windows: Vec::new(),
            window_id_by_key: HashMap::new(),
            applications: ApplicationsConfig { apps: crate::builtins::builtin_apps() },
            hidden_apps: HashMap::new(),
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
            selection: SelectionState::default(),
            open_preview_on_next: false,
            screenshot_return_focus: None,
            suppress_map_focus_for: None,
            zoning: ZoningState::new(),
            menubar: MenubarState::new(),
            pending_launch: None,
            stats: std::sync::Arc::new(crate::stats::Snapshot::default()),
            cpu_hist: crate::stats::History::new(60),
            mem_hist: crate::stats::History::new(60),
            net_down_hist: crate::stats::History::new(60),
            net_up_hist: crate::stats::History::new(60),
            gpu_hist: crate::stats::History::new(60),
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

    /// True when this app_id is under sticky `Topic::AppHidden` (case-insensitive).
    pub fn is_app_hidden(&self, app_id: &str) -> bool {
        self.hidden_apps
            .contains_key(&app_id.to_ascii_lowercase())
    }

    /// Menubar labels for hidden apps (original app_id casing), sorted.
    pub fn hidden_app_labels(&self) -> Vec<String> {
        let mut v: Vec<String> = self.hidden_apps.values().cloned().collect();
        v.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
        v
    }

    /// Retract AppHidden, optionally focus a live window of that app, recompose.
    pub fn unhide_app(&mut self, app_id: &str) {
        let key = app_id.to_ascii_lowercase();
        let original = self
            .hidden_apps
            .remove(&key)
            .unwrap_or_else(|| app_id.to_string());
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.retract(Topic::AppHidden(sola_bus::topics::AppHidden {
                app_id: original.clone(),
            }));
        }
        // Raise / focus if a surface exists.
        if let Some(wid) = self
            .mru_window_by_app
            .get(&original)
            .copied()
            .or_else(|| {
                self.known_windows
                    .iter()
                    .find(|w| w.app_id.eq_ignore_ascii_case(&original))
                    .map(|w| w.window_id)
            })
        {
            self.mru_apps.retain(|id| !id.eq_ignore_ascii_case(&original));
            self.mru_apps.insert(0, original.clone());
            self.mru_window_by_app.insert(original.clone(), wid);
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
            }
        }
        self.emit_composition();
    }

    /// Dismiss transient shell overlays so a capture doesn't leave the
    /// switcher/launcher/selection holding the scene (and keyboard routing).
    pub fn dismiss_transient_overlays(&mut self) {
        let mut changed = false;
        if self.launcher.active {
            self.launcher.active = false;
            changed = true;
        }
        if self.switcher.active {
            self.switcher.active = false;
            changed = true;
        }
        if self.menu_open {
            self.menu_open = false;
            self.open_panel = None;
            self.current_open_index = None;
            changed = true;
        }
        if self.selection.active {
            let _ = self.selection.cancel();
            changed = true;
        }
        if changed {
            self.emit_composition();
            self.emit_registered_chords();
        }
    }

    /// Mark the next Screenshot result as shell-initiated (preview handoff)
    /// and remember which app window should keep the keyboard afterward.
    pub fn arm_screenshot_handoff(&mut self) {
        self.dismiss_transient_overlays();
        self.open_preview_on_next = true;
        // Prefer the app that currently has keyboard focus; never the shell.
        self.screenshot_return_focus = self.focused_window_id.filter(|wid| {
            self.known_windows
                .iter()
                .any(|w| w.window_id == *wid && w.app_id != Self::APP_ID)
        });
    }

    /// Put keyboard focus on `window_id` if it is still a live non-shell
    /// window. Used after selection ends and after preview handoff so we
    /// never leave focus on a hidden shell surface.
    pub fn restore_app_focus(&mut self, prior: Option<u32>) {
        let target = prior
            .filter(|wid| {
                self.known_windows
                    .iter()
                    .any(|w| w.window_id == *wid && w.app_id != Self::APP_ID)
            })
            .or_else(|| {
                self.focused_window_id.filter(|wid| {
                    self.known_windows
                        .iter()
                        .any(|w| w.window_id == *wid && w.app_id != Self::APP_ID)
                })
            });
        let Some(window_id) = target else {
            tracing::warn!("no app window available to restore keyboard focus");
            return;
        };
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::Focus(FocusTarget { window_id }));
        }
        if let Some(w) = self.known_windows.iter().find(|w| w.window_id == window_id) {
            self.focused_window_id = Some(window_id);
            self.focused_app_id = Some(w.app_id.clone());
            self.mru_window_by_app
                .insert(w.app_id.clone(), window_id);
        }
    }

    // -------------------------------------------------------------------------
    // Opening-app toast (launcher feedback)
    // -------------------------------------------------------------------------

    /// Whether a Wayland / catalog `window_app_id` belongs to `pending_app_id`.
    ///
    /// Exact match, ASCII case-insensitive match, or catalog
    /// [`ApplicationsConfig::get_for_window`] hit whose key equals the
    /// pending catalog id.
    pub fn window_matches_pending_app(window_app_id: &str, pending_app_id: &str) -> bool {
        window_app_id == pending_app_id || window_app_id.eq_ignore_ascii_case(pending_app_id)
    }

    /// Catalog-aware match: also accepts windows whose Wayland id resolves to
    /// the pending catalog entry via case-insensitive lookup.
    pub fn matches_pending_app(&self, window_app_id: &str, pending_app_id: &str) -> bool {
        if Self::window_matches_pending_app(window_app_id, pending_app_id) {
            return true;
        }
        self.applications
            .get_for_window(window_app_id)
            .is_some_and(|a| a.app_id == pending_app_id)
    }

    /// Emit LaunchApp for `app_id`, show opening toast, close launcher.
    fn launch_from_launcher(&mut self, app_id: Option<&str>) -> iced::Task<Msg> {
        let mut opening = iced::Task::none();
        if let Some(id) = app_id {
            if let Some(app) = self.applications.get(id) {
                let command = app.command.clone();
                let label = app.label.clone();
                if let Ok(mut bus) = sola_kit::app::bus().lock() {
                    let _ = bus.emit(Topic::LaunchApp(sola_bus::topics::LaunchAppPayload {
                        app_id: id.to_string(),
                        command,
                    }));
                }
                opening = self.begin_opening(id, &label);
            }
        }
        self.launcher.active = false;
        self.emit_composition();
        self.emit_registered_chords();
        if let Some(wid) = self.launcher.prior_focus {
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
            }
        }
        opening
    }

    /// Push `Opening {label}…`, record pending launch state, schedule a 20s
    /// timeout that clears both toast and pending if still current.
    pub fn begin_opening(&mut self, app_id: &str, label: &str) -> iced::Task<Msg> {
        let existing_wids: HashSet<u32> = self
            .known_windows
            .iter()
            .filter(|w| self.matches_pending_app(&w.app_id, app_id))
            .map(|w| w.window_id)
            .collect();
        self.menubar
            .push_toast(format!("Opening {label}…"));
        let toast_generation = self.menubar.toast_generation;
        self.pending_launch = Some(PendingLaunch {
            app_id: app_id.to_string(),
            toast_generation,
            existing_wids,
        });
        iced::Task::perform(
            tokio::time::sleep(Duration::from_secs(OPENING_TOAST_SECS)),
            move |_| Msg::ToastExpire(toast_generation),
        )
    }

    /// Drop pending launch (and its toast, if still current) when a matching
    /// **new** window appears in `known_windows`.
    pub fn resolve_pending_launch_if_window(&mut self) {
        let Some(pending) = self.pending_launch.as_ref() else {
            return;
        };
        let app_id = pending.app_id.clone();
        let existing = pending.existing_wids.clone();
        let toast_gen = pending.toast_generation;
        let appeared = self.known_windows.iter().any(|w| {
            !existing.contains(&w.window_id) && self.matches_pending_app(&w.app_id, &app_id)
        });
        if appeared {
            self.clear_pending_launch(Some(toast_gen));
        }
    }

    /// Clear `pending_launch`. When `toast_generation` is `Some` and still
    /// matches the menubar toast, expire that toast too. When `None`, clear
    /// pending without touching a toast that may already have been replaced
    /// (caller will push a failure/exit toast next).
    pub fn clear_pending_launch(&mut self, toast_generation: Option<u64>) {
        let Some(pending) = self.pending_launch.take() else {
            return;
        };
        if let Some(toast_gen) = toast_generation {
            if pending.toast_generation == toast_gen {
                self.menubar.expire_toast(toast_gen);
            }
        }
    }

    /// If there is a pending launch for `app_id` (case-insensitive), take it
    /// without expiring its toast — the caller will push a replacement toast
    /// or clear the opening toast via the returned generation.
    pub fn take_pending_for_app(&mut self, app_id: &str) -> Option<PendingLaunch> {
        let matches = self
            .pending_launch
            .as_ref()
            .is_some_and(|p| Self::window_matches_pending_app(&p.app_id, app_id));
        if matches {
            self.pending_launch.take()
        } else {
            None
        }
    }

    // -------------------------------------------------------------------------
    // Emit helpers — compute and push bus topics from current state
    // -------------------------------------------------------------------------

    /// Build the composition list (bottom to top) and emit Topic::Composition.
    ///
    /// Stack order (bottom → top):
    ///   1. Shell menubar — always at bottom.
    ///   2. App windows not yet in MRU (never raised) — under everything raised,
    ///      so focus-follows-mouse without a raise cannot leave an external app
    ///      permanently stuck on top of activated windows.
    ///   3. App windows ordered by MRU (least recent first), per-app MRU window
    ///      on top of its siblings. Only click / switcher / map activation bumps
    ///      MRU (raise).
    ///   4. Shell overlays when active (menu, switcher, launcher, selection —
    ///      selection on top while capturing).
    pub fn emit_composition(&self) {
        let mut entries: Vec<CompositionEntry> = Vec::new();

        // 1. Menubar — always at the bottom.
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menubar") {
            entries.push(CompositionEntry { window_id: wid });
        }

        let mru_set: HashSet<&str> = self.mru_apps.iter().map(String::as_str).collect();

        // 2. Apps not yet in MRU — bottom of the app stack (never auto-raised).
        //    Skip composition-hidden apps (AppHidden sticky → River hide).
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID
                || mru_set.contains(w.app_id.as_str())
                || self.is_app_hidden(&w.app_id)
            {
                continue;
            }
            entries.push(CompositionEntry { window_id: w.window_id });
        }

        // 3. App windows ordered by MRU (least recent first = bottom of raised stack).
        // Within each app, the per-app MRU window sits on top of its siblings.
        for app_id in self.mru_apps.iter().rev() {
            if app_id.as_str() == Self::APP_ID || self.is_app_hidden(app_id) {
                continue;
            }
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

        // 4. Shell overlays on top when active.
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
        if self.selection.active {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "selection") {
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
        if self.launcher.active
            || self.switcher.active
            || self.menu_open
            || self.selection.active
        {
            chords.push(RegisteredChord {
                keysym: keys::KEYSYM_ESCAPE,
                modifiers: 0,
            });
        }
        chords.sort_by_key(|c| (c.modifiers, c.keysym));
        chords.dedup();

        tracing::info!(
            count = chords.len(),
            launcher = self.launcher.active,
            switcher = self.switcher.active,
            "emitting RegisteredChords"
        );
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
        // Super+Shift+3 full / +4 selection / +5 focused window (macOS order).
        bindings.push(KeyCode::KEY_3.meta_shift());
        bindings.push(KeyCode::KEY_4.meta_shift());
        bindings.push(KeyCode::KEY_5.meta_shift());

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
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "selection") {
            // Full output at 0,0 so marquee coords match compositor space.
            if let Some((w, h)) = self.zoning.output_size {
                frames.push(FrameUpdate {
                    window_id: wid,
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                    fullscreen: false,
                });
            }
        }
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID { continue; }
            // Floating windows (assigned Float or default-float with no zone)
            // keep client-requested / restore size — never re-frame them.
            // Unassigned sola-* apps used to get default_app_frame (full usable
            // area), which treated them like a zone; they now default-float.
            if self.zoning.is_floating(w.window_id) {
                continue;
            }
            if let Some(f) = self.zoning.window_frame(w.window_id) {
                frames.push(f);
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
        if Some(window) == self.selection_window_id {
            return "selection".to_string();
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

        // IMPORTANT: keep this recipe **stable**. Toggling optional
        // subscriptions (e.g. only listening to keyboard while the launcher
        // is open) rebuilds the whole batch and restarts `bus_subscription`.
        // A race in the bus poller handoff used to return an *empty* stream
        // forever after that — shell looked frozen (no FFM, no chords, stale
        // menubar). Always register the same set; gate behaviour in update.
        let kb = iced::keyboard::listen().map(|event| {
            use iced::keyboard::key::Named;
            use iced::keyboard::{Event, Key};
            match event {
                Event::KeyPressed {
                    key: Key::Named(Named::ArrowUp),
                    ..
                } => Msg::LauncherNav { up: true },
                Event::KeyPressed {
                    key: Key::Named(Named::ArrowDown),
                    ..
                } => Msg::LauncherNav { up: false },
                Event::KeyPressed {
                    key: Key::Named(Named::Enter),
                    ..
                } => Msg::Launch,
                Event::KeyPressed {
                    key: Key::Named(Named::Escape),
                    ..
                } => Msg::CloseLauncher,
                _ => Msg::Noop,
            }
        });

        iced::Subscription::batch([
            sola_kit::app::bus_subscription().map(Msg::Bus),
            time::every(Duration::from_secs(10)).map(|_| Msg::ClockTick),
            crate::stats::subscription().map(Msg::StatsTick),
            kb,
        ])
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
                if self
                    .pending_launch
                    .as_ref()
                    .is_some_and(|p| p.toast_generation == toast_gen)
                {
                    self.pending_launch = None;
                }
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
                // Flower menu: open the app launcher (close menu first).
                if app_id == Self::APP_ID && action_id == "launch" {
                    self.menu_open = false;
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                    self.emit_composition();
                    self.emit_registered_chords();
                    return iced::Task::done(Msg::OpenLauncher);
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
                if !self.launcher.active {
                    return iced::Task::none();
                }
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
                // Keyboard sub is always live (stable subscription recipe);
                // ignore nav when the launcher is closed.
                if !self.launcher.active {
                    return iced::Task::none();
                }
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
                if !self.launcher.active {
                    return iced::Task::none();
                }
                let app_id = self
                    .launcher
                    .filtered_ids
                    .get(self.launcher.selected)
                    .cloned();
                self.launch_from_launcher(app_id.as_deref())
            }
            Msg::LaunchApp(app_id) => {
                if !self.launcher.active {
                    return iced::Task::none();
                }
                // Sync keyboard selection to the clicked row for consistency.
                if let Some(i) = self
                    .launcher
                    .filtered_ids
                    .iter()
                    .position(|id| id == &app_id)
                {
                    self.launcher.selected = i;
                }
                self.launch_from_launcher(Some(&app_id))
            }
            Msg::UnhideApp(app_id) => {
                self.unhide_app(&app_id);
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
                // Superseded by a later enter/leave (e.g. crossed toward menubar).
                if generation != self.pending_focus_generation {
                    return iced::Task::none();
                }
                // Pointer may have moved on without a leave we care about.
                if self.pointer_window_id != Some(window_id) {
                    return iced::Task::none();
                }
                self.focus_window_from_pointer(window_id);
                iced::Task::none()
            }
            Msg::OpenSelection => {
                // Snapshot the live app under the keyboard *before* overlays
                // are dismissed / marquee steals focus.
                let prior = self.focused_window_id.filter(|wid| {
                    self.known_windows
                        .iter()
                        .any(|w| w.window_id == *wid && w.app_id != Self::APP_ID)
                });
                self.dismiss_transient_overlays();
                self.selection.begin(prior);
                self.screenshot_return_focus = prior;
                self.emit_composition();
                self.emit_registered_chords();
                // Focus the selection surface so canvas gets pointer/keyboard.
                if let Some(wid) = self.lookup_window_id(Self::APP_ID, "selection") {
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                    }
                }
                iced::Task::none()
            }
            Msg::CloseSelection => {
                let prior = self.selection.cancel();
                self.emit_composition();
                self.emit_registered_chords();
                self.restore_app_focus(prior.or(self.screenshot_return_focus));
                self.screenshot_return_focus = None;
                self.open_preview_on_next = false;
                iced::Task::none()
            }
            Msg::SelectionPress { x, y } => {
                self.selection.press(x, y);
                iced::Task::none()
            }
            Msg::SelectionMove { x, y } => {
                self.selection.move_to(x, y);
                iced::Task::none()
            }
            Msg::SelectionRelease { x, y } => {
                self.selection.move_to(x, y);
                let (region, prior) = self.selection.finish_region();
                // Hide overlay before capture so the marquee/scrim is not
                // in the PNG (Composition precedes CaptureScreen on the bus).
                self.emit_composition();
                self.emit_registered_chords();
                // Always return keyboard to the pre-marquee window — even on
                // cancel / tiny drag — so focus isn't left on the hidden
                // selection surface. Preview handoff later must not steal it.
                let keep = prior.or(self.screenshot_return_focus);
                self.screenshot_return_focus = keep;
                self.restore_app_focus(keep);
                let Some((rx, ry, rw, rh)) = region else {
                    tracing::info!("selection cancelled (too small or empty)");
                    self.screenshot_return_focus = None;
                    self.open_preview_on_next = false;
                    return iced::Task::none();
                };
                tracing::info!(x = rx, y = ry, w = rw, h = rh, "selection capture");
                self.open_preview_on_next = true;
                if let Ok(mut bus) = sola_kit::app::bus().lock() {
                    use sola_bus::topics::{CaptureScreenPayload, CaptureTarget};
                    let _ = bus.emit(Topic::CaptureScreen(CaptureScreenPayload {
                        path: None,
                        target: CaptureTarget::Region {
                            x: rx,
                            y: ry,
                            width: rw,
                            height: rh,
                        },
                    }));
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
        if Some(window) == self.selection_window_id {
            return crate::selection::view::view(self);
        }
        // Fallback — shouldn't happen under normal operation.
        iced::widget::container(iced::widget::text(""))
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod pending_launch_tests {
    use super::*;

    #[test]
    fn window_matches_pending_app_exact_and_case_insensitive() {
        assert!(Shell::window_matches_pending_app("sola-terminal", "sola-terminal"));
        assert!(Shell::window_matches_pending_app("Orca", "orca"));
        assert!(Shell::window_matches_pending_app("orca", "Orca"));
        assert!(!Shell::window_matches_pending_app("sola-browser", "sola-terminal"));
    }

    #[test]
    fn resolve_requires_new_window_id() {
        let mut shell = Shell {
            theme: theme::default_theme(),
            style: theme::ShellStyle::default(),
            menubar_window_id: None,
            menu_window_id: None,
            launcher_window_id: None,
            switcher_window_id: None,
            selection_window_id: None,
            focused_app_id: None,
            focused_window_id: None,
            pointer_window_id: None,
            pending_focus_generation: 0,
            mru_apps: Vec::new(),
            mru_window_by_app: HashMap::new(),
            known_windows: vec![Window {
                window_id: 1,
                app_id: "sola-terminal".into(),
                title: "Terminal".into(),
                pid: None,
            }],
            window_id_by_key: HashMap::new(),
            applications: ApplicationsConfig {
                apps: crate::builtins::builtin_apps(),
            },
            hidden_apps: HashMap::new(),
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
            selection: SelectionState::default(),
            open_preview_on_next: false,
            screenshot_return_focus: None,
            suppress_map_focus_for: None,
            zoning: ZoningState::new(),
            menubar: MenubarState::new(),
            pending_launch: Some(PendingLaunch {
                app_id: "sola-terminal".into(),
                toast_generation: 1,
                existing_wids: HashSet::from([1]),
            }),
            stats: std::sync::Arc::new(crate::stats::Snapshot::default()),
            cpu_hist: crate::stats::History::new(60),
            mem_hist: crate::stats::History::new(60),
            net_down_hist: crate::stats::History::new(60),
            net_up_hist: crate::stats::History::new(60),
            gpu_hist: crate::stats::History::new(60),
        };
        shell.menubar.toast_generation = 1;
        shell.menubar.toast = Some("Opening Terminal…".into());

        // Existing window only — still pending.
        shell.resolve_pending_launch_if_window();
        assert!(shell.pending_launch.is_some());
        assert_eq!(shell.menubar.toast.as_deref(), Some("Opening Terminal…"));

        // New matching window resolves.
        shell.known_windows.push(Window {
            window_id: 2,
            app_id: "sola-terminal".into(),
            title: "Terminal".into(),
            pid: None,
        });
        shell.resolve_pending_launch_if_window();
        assert!(shell.pending_launch.is_none());
        assert!(shell.menubar.toast.is_none());
    }
}
