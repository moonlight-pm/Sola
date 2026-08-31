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

/// Emit/retract on the kit bus if it is installed. No-op in unit tests.
fn with_bus(f: impl FnOnce(&mut sola_bus::BusClient)) {
    let Some(slot) = sola_kit::app::try_bus() else {
        return;
    };
    match slot.lock() {
        Ok(mut bus) => f(&mut bus),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Menubar,
    Menu,
    Launcher,
    Switcher,
    Selection,
    Notify,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Bus(Arc<sola_bus::Message>),
    /// Fired by `iced::window::open`'s Task when a window's OS handle is ready.
    WindowOpened(WindowKind, iced::window::Id),
    /// Overlay iced swapchain size (after Wayland configure). Used to delay
    /// Composition until the buffer is live, not the parked 2×2.
    OverlayIcedResized {
        id: iced::window::Id,
        size: iced::Size,
    },
    /// Next-tick hop after a live overlay resize so view/present run first.
    CommitOverlayShow,
    /// Open the menu at the given app-menu index (0 = app-name slot).
    /// `is_system` true means the system-menu button was pressed.
    OpenMenu {
        index: usize,
        is_system: bool,
    },
    /// Hover over a menu label — only re-opens if a different menu is already open.
    HoverMenu {
        index: usize,
        is_system: bool,
    },
    /// Close the currently open menu (backdrop click, focus change, Escape, etc.)
    CloseMenu,
    /// User selected a menu action: route to bus and close menu.
    MenuAction {
        app_id: String,
        action_id: String,
    },
    /// Menubar view reports the laid-out X position of a label at `index`.
    MenuLabelPosition {
        index: usize,
        x: f32,
    },
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
    LauncherNav {
        up: bool,
    },
    /// Launch the selected application and close the launcher.
    Launch,
    /// Launch a specific app by id (row click — not the keyboard selection).
    LaunchApp(String),
    /// Menubar unread chip: raise (and unhide) sola-mail.
    RaiseMail,
    /// Expire a live notification banner (`NotifyState` generation).
    NotifyExpire(u64),
    /// Animation tick while a banner is entering or leaving.
    NotifyTick,
    /// Click a card / pile row: raise source, drop from live+pile.
    NotifyActivate(String),
    /// × without raising.
    NotifyDismiss(String),
    /// Open / close the missed-pile panel.
    ToggleNotifyPile,
    /// Empty the missed pile.
    NotifyClearPile,
    /// Open / close the volume popover.
    ToggleAudio,
    /// Snapshot from the PipeWire worker.
    Audio(crate::audio::Event),
    /// Volume / mute / default-device controls.
    AudioUi(crate::audio::UiMsg),
    /// Open / close the Bluetooth popover.
    ToggleBluetooth,
    /// Snapshot / agent events from the BlueZ worker.
    Bluetooth(crate::bluetooth::Event),
    /// Panel controls (power, add, pair, disconnect, agent reply).
    BluetoothUi(crate::bluetooth::UiMsg),
    // --- Switcher messages ---
    /// Cycle switcher selection forward (next=true) or backward (next=false).
    SwitcherNav {
        next: bool,
    },
    /// Hover-select: mouse entered card at `index`.
    SwitcherHover {
        index: usize,
    },
    /// Confirm selection: focus the MRU window of the selected app, deactivate.
    SwitcherConfirm,
    /// Cancel without focus change: deactivate.
    SwitcherCancel,
    /// Focus-follows-mouse delay fired: focus `window_id` (no raise) if
    /// `generation` still matches.
    FocusHoverFire {
        window_id: u32,
        generation: u64,
    },
    /// Cycle to the next window of the currently focused app (Meta+`).
    CycleAppWindows,
    /// Super+Shift+4: freeze the output, then open the selection marquee.
    OpenSelection,
    /// RGBA freeze for Super+Shift+4 finished (call plane). Overlay opens
    /// only after this so menus/selections stay in the still.
    SelectionFreeze {
        generation: u64,
        result: Result<FreezeImage, String>,
    },
    /// Freeze texture is on the GPU; overlay may join composition.
    SelectionTextureReady,
    /// Escape / cancel selection without capturing.
    CloseSelection,
    /// Pointer down on the selection overlay (compositor-space coords).
    SelectionPress {
        x: f32,
        y: f32,
    },
    /// Pointer move while dragging a selection.
    SelectionMove {
        x: f32,
        y: f32,
    },
    /// Pointer up — finish region capture if large enough.
    SelectionRelease {
        x: f32,
        y: f32,
    },
    /// `compositor.screenshot` finished (call plane, not the bus).
    ScreenshotDone(Result<std::path::PathBuf, String>),
    Noop,
}

/// Frozen full-output RGBA for the selection overlay. Handle clone is cheap
/// (refcount); Debug omits the pixel buffer.
#[derive(Clone)]
pub struct FreezeImage {
    pub handle: iced::widget::image::Handle,
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Debug for FreezeImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FreezeImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

/// Which non-menu panel the Menu window is hosting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Calendar,
    Stat(crate::stats::Metric),
    NotifyPile,
    Bluetooth,
    Audio,
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
    pub notify_window_id: Option<iced::window::Id>,

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
    /// Last composition stack (window_ids bottom→top) we emitted on the bus.
    /// Used to skip redundant `Topic::Composition` when only titles changed.
    pub last_composition: Vec<u32>,
    /// Last registered-chord set we emitted; skip identical re-emits that
    /// still force a River manage cycle (title-only Windows storms).
    pub last_registered_chords: Vec<RegisteredChord>,

    // Application catalog (built-ins + user entries from Topic::Application)
    pub applications: ApplicationsConfig,

    /// Apps omitted from composition (River `hide`). Keyed lowercased for
    /// case-insensitive match; value is the original app_id for AppHidden
    /// retract. Filled from sticky `Topic::AppHidden`.
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
    pub notify: crate::notify::NotifyState,
    /// Menubar Bluetooth chip + popover (in-process BlueZ).
    pub bluetooth: crate::bluetooth::Ui,
    /// Menubar volume chip + popover (PipeWire / wpctl).
    pub audio: crate::audio::Ui,
    /// When true, the next `Msg::ScreenshotDone` Ok should open/raise
    /// sola-preview. Set only by shell hotkey / selection paths.
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
    /// Last `Topic::MailStatus` inbox unread. Chip shows only when mail
    /// is mapped and this is `Some(n)` with `n > 0`.
    pub inbox_unread: Option<u32>,
    /// Iced-reported live swapchain for menu / launcher / switcher /
    /// selection / notify. Composition waits on this so River does not
    /// show a stretched 2×2.
    overlay_iced_live: [bool; 5],
}

impl Shell {
    /// Wayland app_id / bus app_id for the shell's own surfaces.
    pub const APP_ID: &'static str = "sola-shell";

    /// Boot the daemon: menubar first. Menu / launcher / switcher / selection
    /// park at 2×2 after the menubar maps so show is Frame + iced resize,
    /// not a new map — without keeping 5K swapchains while dismissed.
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

        let (menubar_id, menubar_task) = menubar::open_window();
        let task = menubar_task.map(|id| Msg::WindowOpened(WindowKind::Menubar, id));

        let state = Self {
            theme,
            style: theme::ShellStyle::default(),
            menubar_window_id: Some(menubar_id),
            menu_window_id: None,
            launcher_window_id: None,
            switcher_window_id: None,
            selection_window_id: None,
            notify_window_id: None,
            focused_app_id: None,
            focused_window_id: None,
            pointer_window_id: None,
            pending_focus_generation: 0,
            mru_apps: Vec::new(),
            mru_window_by_app: HashMap::new(),
            known_windows: Vec::new(),
            window_id_by_key: HashMap::new(),
            last_composition: Vec::new(),
            last_registered_chords: Vec::new(),
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
            notify: crate::notify::NotifyState::default(),
            bluetooth: crate::bluetooth::Ui::default(),
            audio: crate::audio::Ui::default(),
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
            inbox_unread: None,
            overlay_iced_live: [false; 5],
        };

        (state, task)
    }

    // -------------------------------------------------------------------------
    // Window lookup helpers
    // -------------------------------------------------------------------------

    /// Look up a window_id by (app_id, title). sola-river includes shell surfaces
    /// in Topic::Windows with the title set by the iced `title()` callback.
    pub fn lookup_window_id(&self, app_id: &str, title: &str) -> Option<u32> {
        if app_id == Self::APP_ID {
            let me = std::process::id();
            if let Some(w) = self.known_windows.iter().find(|w| {
                w.app_id == app_id && w.title == title && w.pid == Some(me)
            }) {
                return Some(w.window_id);
            }
        }
        self.window_id_by_key
            .get(&(app_id.to_string(), title.to_string()))
            .copied()
    }

    /// True when this app_id is under sticky `Topic::AppHidden` (case-insensitive).
    pub fn is_app_hidden(&self, app_id: &str) -> bool {
        self.hidden_apps.contains_key(&app_id.to_ascii_lowercase())
    }

    /// Retract sticky AppHidden without raising. Used when the app's last
    /// surface is gone so a later map of the same app_id is not stuck hidden.
    pub fn retract_app_hidden(&mut self, app_id: &str) {
        let key = app_id.to_ascii_lowercase();
        let Some(original) = self.hidden_apps.remove(&key) else {
            return;
        };
        with_bus(|bus| {
            let _ = bus.retract(Topic::AppHidden(sola_bus::topics::AppHidden {
                app_id: original,
            }));
        });
    }

    /// Retract AppHidden and raise a live window of that app (composition +
    /// seat). No-op raise when no surface exists; the sticky is still cleared.
    pub fn unhide_app(&mut self, app_id: &str) {
        let key = app_id.to_ascii_lowercase();
        let original = self
            .hidden_apps
            .remove(&key)
            .unwrap_or_else(|| app_id.to_string());
        with_bus(|bus| {
            let _ = bus.retract(Topic::AppHidden(sola_bus::topics::AppHidden {
                app_id: original.clone(),
            }));
        });
        let raise_id = self
            .known_windows
            .iter()
            .find(|w| w.app_id.eq_ignore_ascii_case(&original))
            .map(|w| w.app_id.clone())
            .unwrap_or(original);
        self.raise_app(&raise_id);
    }

    /// Super+H: omit the focused app from composition (River `hide`).
    /// Focuses the next visible MRU app. Does not hide the shell.
    pub fn hide_focused_app(&mut self) {
        let Some(app_id) = self.focused_app_id.clone() else {
            return;
        };
        if app_id == Self::APP_ID || self.is_app_hidden(&app_id) {
            return;
        }
        tracing::info!(app_id = %app_id, "Super+H — hide focused app");
        self.pending_focus_generation = self.pending_focus_generation.wrapping_add(1);
        self.hidden_apps
            .insert(app_id.to_ascii_lowercase(), app_id.clone());
        with_bus(|bus| {
            let _ = bus.emit(Topic::AppHidden(sola_bus::topics::AppHidden {
                app_id: app_id.clone(),
            }));
        });
        if self.pointer_window_id.is_some_and(|wid| {
            self.known_windows
                .iter()
                .any(|w| w.window_id == wid && w.app_id.eq_ignore_ascii_case(&app_id))
        }) {
            self.pointer_window_id = None;
        }
        let next = self
            .mru_apps
            .iter()
            .find(|id| id.as_str() != Self::APP_ID && !self.is_app_hidden(id))
            .cloned();
        if let Some(next) = next {
            self.raise_app(&next);
        } else {
            self.focused_app_id = None;
            self.focused_window_id = None;
            self.emit_registered_chords();
            self.emit_composition();
        }
    }

    /// Catalog / window app_id of a composition-hidden mapped app matching
    /// `catalog_id`, if any. Launcher uses this to unhide instead of spawn.
    pub fn mapped_hidden_app_id(&self, catalog_id: &str) -> Option<String> {
        if self.is_app_hidden(catalog_id) {
            return Some(
                self.hidden_apps
                    .get(&catalog_id.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_else(|| catalog_id.to_string()),
            );
        }
        self.known_windows.iter().find_map(|w| {
            if self.matches_pending_app(&w.app_id, catalog_id) && self.is_app_hidden(&w.app_id) {
                Some(w.app_id.clone())
            } else {
                None
            }
        })
    }

    pub fn mail_is_mapped(&self) -> bool {
        self.known_windows
            .iter()
            .any(|w| w.app_id.eq_ignore_ascii_case("sola-mail"))
    }

    /// Unread to show in the menubar, if any.
    pub fn mail_unread_badge(&self) -> Option<u32> {
        let n = self.inbox_unread.filter(|n| *n > 0)?;
        if self.mail_is_mapped() { Some(n) } else { None }
    }

    /// Raise sola-mail (unhide first if it is composition-hidden).
    pub fn activate_mail(&mut self) {
        if self.is_app_hidden("sola-mail") {
            self.unhide_app("sola-mail");
        }
        self.raise_app("sola-mail");
    }

    fn activate_notification(&mut self, id: &str) -> iced::Task<Msg> {
        let Some(n) = self.notify.take(id) else {
            return iced::Task::none();
        };
        if self.is_app_hidden(&n.app_id) {
            self.unhide_app(&n.app_id);
        }
        self.raise_app(&n.app_id);
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::NotificationActivate(
                sola_bus::topics::NotificationActivate {
                    id: n.id.clone(),
                    app_id: n.app_id.clone(),
                    tab_id: n.tab_id,
                    url: n.url.clone(),
                },
            ));
        }
        if self.notify.pile.is_empty() && self.open_panel == Some(Panel::NotifyPile) {
            self.menu_open = false;
            self.set_open_panel(None);
        }
        self.emit_overlay_frames();
        self.emit_composition();
        iced::Task::none()
    }

    fn notify_followup(&mut self, now: std::time::Instant) -> iced::Task<Msg> {
        self.emit_overlay_frames();
        self.emit_composition();
        if self.notify.needs_tick(now) {
            iced::Task::perform(tokio::time::sleep(crate::notify::TICK), |_| Msg::NotifyTick)
        } else {
            iced::Task::none()
        }
    }

    fn push_notification(&mut self, n: sola_bus::topics::AppNotification) -> iced::Task<Msg> {
        let now = std::time::Instant::now();
        let generation = self.notify.push(n, now);
        let expire = iced::Task::perform(tokio::time::sleep(crate::notify::HOLD), move |_| {
            Msg::NotifyExpire(generation)
        });
        iced::Task::batch([expire, self.notify_followup(now)])
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
            self.set_open_panel(None);
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

    fn on_selection_freeze(
        &mut self,
        generation: u64,
        result: Result<FreezeImage, String>,
    ) -> iced::Task<Msg> {
        if generation != self.selection.freeze_generation || !self.selection.pending {
            return iced::Task::none();
        }
        match result {
            Err(e) => {
                let prior = self.selection.cancel();
                self.emit_registered_chords();
                let keep = prior.or(self.screenshot_return_focus.take());
                self.restore_app_focus(keep);
                self.open_preview_on_next = false;
                tracing::warn!(%e, "selection freeze failed");
                self.menubar.push_toast(format!("Screenshot failed: {e}"));
                let toast_gen = self.menubar.toast_generation;
                iced::Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
                    Msg::ToastExpire(toast_gen)
                })
            }
            Ok(img) => {
                // Dismiss shell menus *after* the still is in hand, and
                // *before* `apply_freeze` marks the overlay active (dismiss
                // would otherwise cancel the selection).
                self.dismiss_transient_overlays();
                if !self.selection.apply_freeze(generation, img.handle) {
                    return iced::Task::none();
                }
                tracing::info!(
                    width = img.width,
                    height = img.height,
                    "selection freeze ready"
                );
                // Frame the overlay to the output while still hidden. It
                // joins composition on SelectionTextureReady (after GPU
                // upload) so the first visible frame is the still.
                self.emit_registered_chords();
                iced::Task::none()
            }
        }
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
            self.mru_window_by_app.insert(w.app_id.clone(), window_id);
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
    /// A composition-hidden running app is unhidden and raised instead of
    /// spawned again.
    fn launch_from_launcher(&mut self, app_id: Option<&str>) -> iced::Task<Msg> {
        let mut opening = iced::Task::none();
        if let Some(id) = app_id {
            if let Some(hidden) = self.mapped_hidden_app_id(id) {
                self.launcher.active = false;
                self.unhide_app(&hidden);
                self.emit_registered_chords();
                return iced::Task::none();
            }
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
        self.menubar.push_toast(format!("Opening {label}…"));
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

    /// Build the composition list (bottom to top). See [`Self::emit_composition`].
    fn build_composition_entries(&self) -> Vec<CompositionEntry> {
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
            entries.push(CompositionEntry {
                window_id: w.window_id,
            });
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
                    entries.push(CompositionEntry {
                        window_id: w.window_id,
                    });
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

        // 4. Shell overlays on top when active *and* already live-sized.
        //    Framing to the output happens while still hidden; joining the
        //    stack at 2×2 is the first-show flash.
        //    Notify sits above apps but below menu/launcher/switcher/selection.
        if self.overlay_should_compose("notify", self.notify.visible()) {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "notify") {
                entries.push(CompositionEntry { window_id: wid });
            }
        }
        if self.overlay_should_compose("menu", self.menu_open) {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menu") {
                entries.push(CompositionEntry { window_id: wid });
            }
        }
        if self.overlay_should_compose("switcher", self.switcher.active) {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "switcher") {
                entries.push(CompositionEntry { window_id: wid });
            }
        }
        if self.overlay_should_compose("launcher", self.launcher.active) {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "launcher") {
                entries.push(CompositionEntry { window_id: wid });
            }
        }
        if self.overlay_should_compose(
            "selection",
            self.selection.active && self.selection.presentable,
        ) {
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, "selection") {
                entries.push(CompositionEntry { window_id: wid });
            }
        }

        entries
    }

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
    ///
    /// No-op when the stack order is identical to the last emit — title-only
    /// `Windows` storms used to re-`place_top` every surface every time.
    pub fn emit_composition(&mut self) {
        let entries = self.build_composition_entries();
        let order: Vec<u32> = entries.iter().map(|e| e.window_id).collect();
        if order == self.last_composition {
            tracing::debug!(count = order.len(), "skip Composition (unchanged)");
            return;
        }
        self.last_composition = order;
        with_bus(|bus| {
            let _ = bus.emit(Topic::Composition(entries));
        });
    }

    /// Which top-level menu of `app_id` contains `action_id`, as a position
    /// in its menu list (0 = the app-name slot shown as the title). `None`
    /// if the app has no cached menu or the action isn't found.
    fn menu_index_for_action(&self, app_id: &str, action_id: &str) -> Option<usize> {
        let payload = self.menus.get_menu(app_id)?;
        payload.menus.iter().position(|menu| {
            menu.items
                .iter()
                .any(|item| matches!(item, MenuItem::Action { id, .. } if id == action_id))
        })
    }

    /// Briefly flash the menubar label that owns `(app_id, action_id)` — the
    /// macOS "command went through the menu" feedback. The shell's own actions
    /// live under the system flower; a focused app's actions map to its title
    /// (index 0) or one of its menu labels. Returns the timer task that ends
    /// the pulse, or `Task::none()` if there's no label to flash.
    fn flash_menu_action(&mut self, app_id: &str, action_id: &str) -> iced::Task<Msg> {
        let target = if app_id == Self::APP_ID {
            FlashTarget {
                is_system: true,
                index: 0,
            }
        } else if let Some(index) = self.menu_index_for_action(app_id, action_id) {
            FlashTarget {
                is_system: false,
                index,
            }
        } else {
            return iced::Task::none();
        };
        let generation = self.menubar.begin_flash(target);
        iced::Task::perform(tokio::time::sleep(Duration::from_millis(150)), move |_| {
            Msg::MenuFlashExpire(generation)
        })
    }

    /// Build the RegisteredChords payload for the current overlay + focused app.
    fn build_registered_chords(&self) -> Vec<RegisteredChord> {
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
            || self.selection.pending
        {
            chords.push(RegisteredChord {
                keysym: keys::KEYSYM_ESCAPE,
                modifiers: 0,
            });
        }
        // While the switcher is up, Super is held (Meta+Tab). Grab Meta+←/→ so
        // River delivers them; `on_chord` already maps Left/Right to SwitcherNav.
        // Deregistered on dismiss so bare apps keep their own arrow bindings.
        if self.switcher.active {
            chords.push(keys::to_registered(&KeyCode::LEFT.meta()));
            chords.push(keys::to_registered(&KeyCode::RIGHT.meta()));
        }
        chords.sort_by_key(|c| (c.modifiers, c.keysym));
        chords.dedup();
        chords
    }

    /// Emit Topic::RegisteredChords based on current overlay state and focused app.
    ///
    /// Base set: shell key chords (Meta+Space, Meta+Tab, Meta+Q, Meta+H, Meta+Grave,
    /// Meta+Numpad{…}), focused-app menu shortcuts (meta-bound only). Bare Super_L
    /// always registered so ChordReleased fires for switcher confirm. Escape
    /// registered only while an overlay is active. Meta+Left/Right registered
    /// only while the switcher is active.
    ///
    /// No-op when the chord set is identical to the last emit — avoids a River
    /// manage cycle on every title-only `Windows` rebroadcast.
    pub fn emit_registered_chords(&mut self) {
        let chords = self.build_registered_chords();
        if chords == self.last_registered_chords {
            tracing::debug!(count = chords.len(), "skip RegisteredChords (unchanged)");
            return;
        }
        self.last_registered_chords = chords.clone();
        tracing::info!(
            count = chords.len(),
            launcher = self.launcher.active,
            switcher = self.switcher.active,
            "emitting RegisteredChords"
        );
        with_bus(|bus| {
            let _ = bus.emit(Topic::RegisteredChords(chords));
        });
    }

    /// Build the list of chords the shell wants River to grab.
    pub fn shell_key_chords(&self) -> Vec<KeyChord> {
        // Shell-own menu bindings (meta-bound items only; Quit Sola has none).
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
        bindings.push(KeyCode::TAB.meta()); // Meta+Tab → switcher
        bindings.push(KeyCode::GRAVE.meta()); // Meta+` → cycle windows of focused app
        bindings.push(KeyCode::SPACE.meta()); // Meta+Space → launcher
        bindings.push(KeyCode::Q.meta()); // Meta+Q → close focused app
        bindings.push(KeyCode::H.meta()); // Meta+H → hide focused app
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

    /// Emit Topic::Frame for mapped shell windows and zoned app windows.
    ///
    /// Overlays stay mapped: live size while visible, 2×2 while dismissed
    /// (see [`crate::zoning::overlay_frame`]). Lookup misses until River
    /// publishes the surface — a later `Windows` / `WindowOpened` fills in.
    pub fn emit_all_frames(&self) {
        let mut frames: Vec<FrameUpdate> = Vec::new();

        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menubar") {
            if let Some(f) = self.zoning.menubar_frame(wid) {
                frames.push(f);
            }
        }
        self.collect_overlay_frames(&mut frames);
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID {
                continue;
            }
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

        Self::emit_frames(frames);
    }

    fn collect_overlay_frames(&self, frames: &mut Vec<FrameUpdate>) {
        let output = self.zoning.output_size;
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menu") {
            if let Some(f) = crate::zoning::menu_overlay_frame(
                wid,
                self.menu_open,
                output,
                self.menu_overlay_spec(),
            ) {
                frames.push(f);
            }
        }
        for (title, visible, cover_menubar) in [
            ("launcher", self.launcher.active, false),
            ("switcher", self.switcher.active, false),
            ("selection", self.selection.active, true),
        ] {
            let Some(wid) = self.lookup_window_id(Self::APP_ID, title) else {
                continue;
            };
            if let Some(f) = crate::zoning::overlay_frame(wid, visible, output, cover_menubar) {
                frames.push(f);
            }
        }
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "notify") {
            let enter = self.notify.enter_t(std::time::Instant::now());
            if let Some(f) = crate::zoning::notify_overlay_frame(
                wid,
                self.notify.visible(),
                output,
                self.notify.stack_height(),
                enter,
            ) {
                frames.push(f);
            }
        }
    }

    /// Frame only overlays (park ↔ live). Do not re-Frame zoned apps — a
    /// launcher toggle must not re-propose gamescope / Electron sizes.
    fn emit_overlay_frames(&self) {
        let mut frames = Vec::new();
        self.collect_overlay_frames(&mut frames);
        Self::emit_frames(frames);
    }

    fn overlay_live_index(kind: WindowKind) -> Option<usize> {
        match kind {
            WindowKind::Menu => Some(0),
            WindowKind::Launcher => Some(1),
            WindowKind::Switcher => Some(2),
            WindowKind::Selection => Some(3),
            WindowKind::Notify => Some(4),
            WindowKind::Menubar => None,
        }
    }

    fn overlay_kind_of_iced(&self, id: iced::window::Id) -> Option<WindowKind> {
        [
            WindowKind::Menu,
            WindowKind::Launcher,
            WindowKind::Switcher,
            WindowKind::Selection,
            WindowKind::Notify,
        ]
        .into_iter()
        .find(|&kind| self.overlay_id(kind) == Some(id))
    }

    fn overlay_iced_live_for_title(&self, title: &str) -> bool {
        let idx = match title {
            "menu" => 0,
            "launcher" => 1,
            "switcher" => 2,
            "selection" => 3,
            "notify" => 4,
            _ => return false,
        };
        self.overlay_iced_live[idx]
    }

    fn overlay_should_compose(&self, title: &str, active: bool) -> bool {
        active && self.overlay_iced_live_for_title(title)
    }

    fn note_overlay_iced_size(&mut self, kind: WindowKind, size: iced::Size) -> bool {
        let Some(idx) = Self::overlay_live_index(kind) else {
            return false;
        };
        let live = crate::zoning::overlay_geometry_is_live(size.width as i32, size.height as i32);
        self.overlay_iced_live[idx] = live;
        live
    }

    fn commit_overlay_show(&mut self) {
        self.emit_composition();
        for (title, want) in [
            ("launcher", self.launcher.active),
            ("switcher", self.switcher.active),
            ("selection", self.selection.active && self.selection.presentable),
        ] {
            if !self.overlay_should_compose(title, want) {
                continue;
            }
            if let Some(wid) = self.lookup_window_id(Self::APP_ID, title) {
                if let Ok(mut bus) = sola_kit::app::bus().lock() {
                    let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                }
                if title == "switcher" {
                    self.focused_window_id = Some(wid);
                }
            }
        }
    }

    fn emit_frames(frames: Vec<FrameUpdate>) {
        if frames.is_empty() {
            return;
        }
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            for f in frames {
                let _ = bus.emit(Topic::Frame(f));
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
        if Some(window) == self.notify_window_id {
            return "notify".to_string();
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

    /// Left edge of the Bluetooth chip (immediately left of CPU in the
    /// right-aligned cluster). Hide-if-no-adapter does not affect this
    /// estimate while the chip is shown.
    pub fn estimate_bluetooth_x(&self) -> f32 {
        const BT_W: f32 = 32.0;
        const GAP: f32 = 4.0;
        self.estimate_stat_x(crate::stats::Metric::Cpu) - GAP - BT_W
    }

    /// Card-sized live frame for the menu overlay (not the full output).
    fn menu_overlay_spec(&self) -> crate::zoning::MenuOverlaySpec {
        const GUTTER: f32 = 8.0;
        let ow = self.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
        let clamp_x = |x: f32, w: f32| x.min((ow - w - GUTTER).max(0.0)).max(0.0);
        let (x, width, height) = match self.open_panel {
            Some(Panel::Calendar) => {
                let w = crate::calendar::CARD_WIDTH;
                (clamp_x(ow - w - GUTTER, w), w, crate::calendar::CARD_HEIGHT)
            }
            Some(Panel::Stat(m)) => {
                let w = crate::stats::view::CARD_WIDTH;
                (
                    clamp_x(self.estimate_stat_x(m), w),
                    w,
                    crate::stats::view::CARD_HEIGHT,
                )
            }
            Some(Panel::NotifyPile) => {
                let w = crate::notify::view::PILE_WIDTH;
                (
                    clamp_x(ow - w - GUTTER, w),
                    w,
                    crate::notify::view::PILE_HEIGHT,
                )
            }
            Some(Panel::Bluetooth) => {
                let w = crate::bluetooth::view::CARD_WIDTH;
                (
                    clamp_x(self.estimate_bluetooth_x(), w),
                    w,
                    crate::bluetooth::view::CARD_HEIGHT,
                )
            }
            Some(Panel::Audio) => {
                let w = crate::audio::view::CARD_WIDTH;
                (
                    clamp_x(self.estimate_audio_x(), w),
                    w,
                    crate::audio::view::CARD_HEIGHT,
                )
            }
            None => {
                let w = crate::menu::view::MENU_WIDTH;
                (
                    clamp_x(self.menu_anchor_x, w),
                    w,
                    crate::menu::view::MENU_HEIGHT,
                )
            }
        };
        crate::zoning::MenuOverlaySpec {
            x: x.round() as i32,
            width: width.round() as i32,
            height: height.round() as i32,
        }
    }

    /// Left edge of the volume chip (left of Bluetooth when that chip is
    /// shown, otherwise immediately left of CPU).
    pub fn estimate_audio_x(&self) -> f32 {
        const CHIP_W: f32 = 32.0;
        const GAP: f32 = 4.0;
        let right = if crate::bluetooth::bar_icon(&self.bluetooth.snapshot).is_some() {
            self.estimate_bluetooth_x()
        } else {
            self.estimate_stat_x(crate::stats::Metric::Cpu)
        };
        right - GAP - CHIP_W
    }

    /// Change `open_panel`, stopping Bluetooth discovery and clearing the
    /// stats sampler when leaving those surfaces.
    pub(crate) fn set_open_panel(&mut self, panel: Option<Panel>) {
        let was_bt = self.open_panel == Some(Panel::Bluetooth);
        let now_bt = panel == Some(Panel::Bluetooth);
        let was_audio = self.open_panel == Some(Panel::Audio);
        let now_audio = panel == Some(Panel::Audio);
        if was_bt && !now_bt {
            self.bluetooth.on_close();
            crate::bluetooth::send(crate::bluetooth::Command::SetDiscovering(false));
        }
        match panel {
            Some(Panel::Stat(m)) => crate::stats::set_active_metric(Some(m)),
            _ => crate::stats::set_active_metric(None),
        }
        self.open_panel = panel;
        if now_bt && !was_bt {
            crate::bluetooth::send(crate::bluetooth::Command::Refresh);
        }
        if now_audio && !was_audio {
            crate::audio::send(crate::audio::Command::Refresh);
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
            crate::bluetooth::subscription().map(Msg::Bluetooth),
            crate::audio::subscription().map(Msg::Audio),
            iced::window::resize_events().map(|(id, size)| Msg::OverlayIcedResized { id, size }),
            kb,
        ])
    }

    /// Map parked overlay iced windows if they are missing. Called after
    /// the first Composition (menubar) so River hides them on first map.
    fn ensure_overlay_windows(&mut self) -> iced::Task<Msg> {
        iced::Task::batch([
            self.open_overlay(WindowKind::Menu),
            self.open_overlay(WindowKind::Launcher),
            self.open_overlay(WindowKind::Switcher),
            self.open_overlay(WindowKind::Selection),
            self.open_overlay(WindowKind::Notify),
        ])
    }

    fn overlay_id(&self, kind: WindowKind) -> Option<iced::window::Id> {
        match kind {
            WindowKind::Menubar => self.menubar_window_id,
            WindowKind::Menu => self.menu_window_id,
            WindowKind::Launcher => self.launcher_window_id,
            WindowKind::Switcher => self.switcher_window_id,
            WindowKind::Selection => self.selection_window_id,
            WindowKind::Notify => self.notify_window_id,
        }
    }

    fn overlay_id_mut(&mut self, kind: WindowKind) -> &mut Option<iced::window::Id> {
        match kind {
            WindowKind::Menubar => &mut self.menubar_window_id,
            WindowKind::Menu => &mut self.menu_window_id,
            WindowKind::Launcher => &mut self.launcher_window_id,
            WindowKind::Switcher => &mut self.switcher_window_id,
            WindowKind::Selection => &mut self.selection_window_id,
            WindowKind::Notify => &mut self.notify_window_id,
        }
    }

    fn overlay_want(&self, kind: WindowKind) -> bool {
        match kind {
            WindowKind::Menubar => true,
            WindowKind::Menu => self.menu_open,
            WindowKind::Launcher => self.launcher.active,
            WindowKind::Switcher => self.switcher.active,
            WindowKind::Selection => self.selection.active,
            WindowKind::Notify => self.notify.visible(),
        }
    }

    fn overlay_visibility(&self) -> [bool; 5] {
        [
            self.menu_open,
            self.launcher.active,
            self.switcher.active,
            self.selection.active,
            self.notify.visible(),
        ]
    }

    fn open_overlay(&mut self, kind: WindowKind) -> iced::Task<Msg> {
        if matches!(kind, WindowKind::Menubar) || self.overlay_id(kind).is_some() {
            return iced::Task::none();
        }
        let (id, task) = match kind {
            WindowKind::Menu => crate::menu::open_window(),
            WindowKind::Launcher => crate::launcher::open_window(),
            WindowKind::Switcher => crate::switcher::open_window(),
            WindowKind::Selection => crate::selection::open_window(),
            WindowKind::Notify => crate::notify::open_window(),
            WindowKind::Menubar => return iced::Task::none(),
        };
        *self.overlay_id_mut(kind) = Some(id);
        task.map(move |opened| Msg::WindowOpened(kind, opened))
    }

    fn on_overlay_mapped(&mut self, kind: WindowKind) -> iced::Task<Msg> {
        self.emit_all_frames();
        self.emit_composition();
        if kind == WindowKind::Menubar {
            return self.ensure_overlay_windows();
        }
        if kind == WindowKind::Launcher && self.launcher.active {
            return iced::widget::operation::focus::<Msg>(iced::widget::Id::new(
                crate::launcher::view::QUERY_INPUT_ID,
            ));
        }
        iced::Task::none()
    }

    pub fn update(&mut self, msg: Msg) -> iced::Task<Msg> {
        let before = self.overlay_visibility();
        let task = self.dispatch_msg(msg);
        let after = self.overlay_visibility();
        let mut tasks = vec![task];
        if !self.last_composition.is_empty()
            && (self.menu_window_id.is_none()
                || self.launcher_window_id.is_none()
                || self.switcher_window_id.is_none()
                || self.selection_window_id.is_none()
                || self.notify_window_id.is_none())
        {
            tasks.push(self.ensure_overlay_windows());
        }
        if before != after {
            for i in 0..5 {
                if before[i] && !after[i] {
                    self.overlay_iced_live[i] = false;
                }
            }
            self.emit_overlay_frames();
        }
        iced::Task::batch(tasks)
    }

    fn dispatch_msg(&mut self, msg: Msg) -> iced::Task<Msg> {
        match msg {
            Msg::Bus(arc) => self.handle_bus(&arc),
            Msg::WindowOpened(kind, id) => {
                *self.overlay_id_mut(kind) = Some(id);
                self.on_overlay_mapped(kind)
            }
            Msg::OverlayIcedResized { id, size } => {
                let Some(kind) = self.overlay_kind_of_iced(id) else {
                    return iced::Task::none();
                };
                let live = self.note_overlay_iced_size(kind, size);
                if live && self.overlay_want(kind) {
                    if kind == WindowKind::Selection && !self.selection.presentable {
                        return iced::Task::none();
                    }
                    return iced::Task::done(Msg::CommitOverlayShow);
                }
                iced::Task::none()
            }
            Msg::SelectionTextureReady => {
                if !self.selection.active || self.selection.presentable {
                    return iced::Task::none();
                }
                self.selection.presentable = true;
                if self.overlay_iced_live_for_title("selection") {
                    return iced::Task::done(Msg::CommitOverlayShow);
                }
                iced::Task::none()
            }
            Msg::CommitOverlayShow => {
                self.commit_overlay_show();
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
                    self.set_open_panel(None);
                } else {
                    // Open (or switch an app menu over to) the calendar,
                    // always starting on the current month.
                    self.menu_open = true;
                    self.set_open_panel(Some(Panel::Calendar));
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                    self.calendar_month =
                        crate::calendar::first_of_month(self.menubar.clock_now.date_naive());
                }
                self.emit_overlay_frames();
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::ToggleStatPanel(m) => {
                if self.menu_open && self.open_panel == Some(crate::app::Panel::Stat(m)) {
                    self.menu_open = false;
                    self.set_open_panel(None);
                } else {
                    self.menu_open = true;
                    self.set_open_panel(Some(crate::app::Panel::Stat(m)));
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                }
                self.emit_overlay_frames();
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
                    self.emit_overlay_frames();
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
                self.set_open_panel(None);
                self.emit_overlay_frames();
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
                    self.set_open_panel(None);
                    self.emit_overlay_frames();
                }
                iced::Task::none()
            }
            Msg::CloseMenu => {
                self.menu_open = false;
                self.current_open_index = None;
                self.current_open_is_system = false;
                self.set_open_panel(None);
                self.emit_overlay_frames();
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
                        let _ =
                            bus.emit(Topic::MenuAction(MenuActionPayload { app_id, action_id }));
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
                // Keyboard routes after iced reports a live swapchain
                // (`CommitOverlayShow`). Focusing the parked 2×2 can unhide it.
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
            Msg::RaiseMail => {
                self.activate_mail();
                iced::Task::none()
            }
            Msg::NotifyExpire(generation) => {
                let now = std::time::Instant::now();
                if self.notify.begin_leave(generation, now) {
                    return self.notify_followup(now);
                }
                iced::Task::none()
            }
            Msg::NotifyTick => {
                let now = std::time::Instant::now();
                self.notify.finish_leave(now);
                self.emit_overlay_frames();
                self.emit_composition();
                self.notify_followup(now)
            }
            Msg::NotifyActivate(id) => self.activate_notification(&id),
            Msg::NotifyDismiss(id) => {
                self.notify.dismiss(&id);
                if self.notify.pile.is_empty() && self.open_panel == Some(Panel::NotifyPile) {
                    self.menu_open = false;
                    self.set_open_panel(None);
                }
                self.emit_overlay_frames();
                self.emit_composition();
                iced::Task::none()
            }
            Msg::ToggleNotifyPile => {
                if self.menu_open && self.open_panel == Some(Panel::NotifyPile) {
                    self.menu_open = false;
                    self.set_open_panel(None);
                } else {
                    self.menu_open = true;
                    self.set_open_panel(Some(Panel::NotifyPile));
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                }
                self.emit_overlay_frames();
                self.emit_composition();
                iced::Task::none()
            }
            Msg::NotifyClearPile => {
                self.notify.clear_pile();
                self.menu_open = false;
                self.set_open_panel(None);
                self.emit_composition();
                iced::Task::none()
            }
            Msg::ToggleAudio => {
                if !self.audio.snapshot.available {
                    return iced::Task::none();
                }
                if self.menu_open && self.open_panel == Some(Panel::Audio) {
                    self.menu_open = false;
                    self.set_open_panel(None);
                } else {
                    self.menu_open = true;
                    self.set_open_panel(Some(Panel::Audio));
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                }
                self.emit_overlay_frames();
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::Audio(ev) => {
                self.audio.on_event(ev);
                if !self.audio.snapshot.available && self.open_panel == Some(Panel::Audio) {
                    self.menu_open = false;
                    self.set_open_panel(None);
                    self.emit_composition();
                    self.emit_registered_chords();
                }
                iced::Task::none()
            }
            Msg::AudioUi(m) => {
                if let Some(cmd) = self.audio.update(m) {
                    crate::audio::send(cmd);
                }
                iced::Task::none()
            }
            Msg::ToggleBluetooth => {
                if self.bluetooth.snapshot.adapter.is_none() {
                    return iced::Task::none();
                }
                if self.menu_open && self.open_panel == Some(Panel::Bluetooth) {
                    self.menu_open = false;
                    self.set_open_panel(None);
                } else {
                    self.menu_open = true;
                    self.set_open_panel(Some(Panel::Bluetooth));
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                }
                self.emit_overlay_frames();
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
            Msg::Bluetooth(ev) => {
                self.bluetooth.on_event(ev);
                if self.bluetooth.snapshot.adapter.is_none()
                    && self.open_panel == Some(Panel::Bluetooth)
                {
                    self.menu_open = false;
                    self.set_open_panel(None);
                    self.emit_composition();
                    self.emit_registered_chords();
                }
                iced::Task::none()
            }
            Msg::BluetoothUi(m) => {
                if let Some(cmd) = self.bluetooth.update(m) {
                    crate::bluetooth::send(cmd);
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
                self.switcher.selected = index.min(self.switcher.apps.len().saturating_sub(1));
                iced::Task::none()
            }
            Msg::SwitcherConfirm => {
                let app_id = self.switcher.selected_app_id().map(|s| s.to_string());
                self.switcher.active = false;

                if let Some(ref app_id) = app_id {
                    // Unhide if Super+H parked this app, then raise (MRU + seat).
                    self.raise_app(app_id);
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
            Msg::FocusHoverFire {
                window_id,
                generation,
            } => {
                // Superseded by a later enter/leave (e.g. crossed toward menubar).
                if generation != self.pending_focus_generation {
                    tracing::debug!(
                        window_id,
                        generation,
                        current = self.pending_focus_generation,
                        "FFM fire superseded"
                    );
                    return iced::Task::none();
                }
                // Pointer may have moved on without a leave we care about.
                if self.pointer_window_id != Some(window_id) {
                    tracing::debug!(
                        window_id,
                        pointer = ?self.pointer_window_id,
                        "FFM fire pointer moved on"
                    );
                    return iced::Task::none();
                }
                tracing::debug!(window_id, generation, "FFM fire — applying pointer focus");
                self.focus_window_from_pointer(window_id);
                iced::Task::none()
            }
            Msg::OpenSelection => {
                if self.selection.active || self.selection.pending {
                    return iced::Task::none();
                }
                // Snapshot the live app under the keyboard *before* the
                // freeze capture. Do not dismiss overlays or steal focus
                // yet — that would drop menus/text selections from the still.
                let prior = self.focused_window_id.filter(|wid| {
                    self.known_windows
                        .iter()
                        .any(|w| w.window_id == *wid && w.app_id != Self::APP_ID)
                });
                let generation = self.selection.start_freeze(prior);
                self.screenshot_return_focus = prior;
                self.emit_registered_chords();
                crate::screenshot::freeze(generation)
            }
            Msg::SelectionFreeze { generation, result } => {
                self.on_selection_freeze(generation, result)
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
                let freeze = self.selection.freeze.clone();
                let (region, prior) = self.selection.finish_region();
                // Hide overlay; crop the freeze in-process (no second capture,
                // so the still keeps menus/selections the live screen lost).
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
                let Some(handle) = freeze else {
                    tracing::warn!("selection capture missing freeze frame");
                    self.screenshot_return_focus = None;
                    self.open_preview_on_next = false;
                    self.menubar
                        .push_toast("Screenshot failed: no freeze frame");
                    let toast_gen = self.menubar.toast_generation;
                    return iced::Task::perform(
                        tokio::time::sleep(Duration::from_secs(5)),
                        move |_| Msg::ToastExpire(toast_gen),
                    );
                };
                tracing::info!(x = rx, y = ry, w = rw, h = rh, "selection crop from freeze");
                self.open_preview_on_next = true;
                crate::screenshot::crop_freeze(handle, rx, ry, rw, rh)
            }
            Msg::ScreenshotDone(result) => self.on_screenshot_done(result),
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
                        sola_bus::topics::FocusTarget {
                            window_id: next_wid,
                        },
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
        if Some(window) == self.notify_window_id {
            return crate::notify::view::view(self);
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
        assert!(Shell::window_matches_pending_app(
            "sola-terminal",
            "sola-terminal"
        ));
        assert!(Shell::window_matches_pending_app("Orca", "orca"));
        assert!(Shell::window_matches_pending_app("orca", "Orca"));
        assert!(!Shell::window_matches_pending_app(
            "sola-browser",
            "sola-terminal"
        ));
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
            notify_window_id: None,
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
            last_composition: Vec::new(),
            last_registered_chords: Vec::new(),
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
            notify: crate::notify::NotifyState::default(),
            bluetooth: crate::bluetooth::Ui::default(),
            audio: crate::audio::Ui::default(),
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
            inbox_unread: None,
            overlay_iced_live: [false; 5],
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

#[cfg(test)]
mod hide_tests {
    use super::*;

    fn win(id: u32, app: &str, title: &str) -> Window {
        Window {
            window_id: id,
            app_id: app.into(),
            title: title.into(),
            pid: None,
        }
    }

    fn test_shell(windows: Vec<Window>, focused: Option<&str>, mru: &[&str]) -> Shell {
        let mut window_id_by_key = HashMap::new();
        let mut mru_window_by_app = HashMap::new();
        for w in &windows {
            window_id_by_key.insert((w.app_id.clone(), w.title.clone()), w.window_id);
            if w.app_id != Shell::APP_ID {
                mru_window_by_app
                    .entry(w.app_id.clone())
                    .or_insert(w.window_id);
            }
        }
        let focused_window_id = focused.and_then(|app| {
            windows
                .iter()
                .find(|w| w.app_id == app)
                .map(|w| w.window_id)
        });
        Shell {
            theme: theme::default_theme(),
            style: theme::ShellStyle::default(),
            menubar_window_id: None,
            menu_window_id: None,
            launcher_window_id: None,
            switcher_window_id: None,
            selection_window_id: None,
            notify_window_id: None,
            focused_app_id: focused.map(str::to_string),
            focused_window_id,
            pointer_window_id: focused_window_id,
            pending_focus_generation: 0,
            mru_apps: mru.iter().map(|s| s.to_string()).collect(),
            mru_window_by_app,
            known_windows: windows,
            window_id_by_key,
            last_composition: Vec::new(),
            last_registered_chords: Vec::new(),
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
            notify: crate::notify::NotifyState::default(),
            bluetooth: crate::bluetooth::Ui::default(),
            audio: crate::audio::Ui::default(),
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
            inbox_unread: None,
            overlay_iced_live: [false; 5],
        }
    }

    fn desktop() -> Shell {
        test_shell(
            vec![
                win(1, Shell::APP_ID, "menubar"),
                win(2, "sola-terminal", "Terminal"),
                win(3, "sola-browser", "Browser"),
            ],
            Some("sola-terminal"),
            &["sola-terminal", "sola-browser"],
        )
    }

    fn composed_apps(shell: &Shell) -> Vec<u32> {
        shell
            .build_composition_entries()
            .into_iter()
            .map(|e| e.window_id)
            .collect()
    }

    #[test]
    fn shell_key_chords_include_super_h() {
        let shell = desktop();
        assert!(
            shell
                .shell_key_chords()
                .iter()
                .any(|c| c.keycode == KeyCode::H && c.meta && !c.shift && !c.ctrl && !c.alt),
            "Super+H must be a registered shell chord"
        );
    }

    #[test]
    fn hide_omits_app_from_composition_and_focuses_next() {
        let mut shell = desktop();
        assert_eq!(composed_apps(&shell), vec![1, 3, 2]); // menubar, browser, terminal (MRU top)

        shell.hide_focused_app();

        assert!(shell.is_app_hidden("sola-terminal"));
        assert_eq!(shell.focused_app_id.as_deref(), Some("sola-browser"));
        assert_eq!(shell.focused_window_id, Some(3));
        assert_eq!(composed_apps(&shell), vec![1, 3]); // terminal omitted
        assert_eq!(
            shell.mapped_hidden_app_id("sola-terminal").as_deref(),
            Some("sola-terminal")
        );
    }

    #[test]
    fn hide_does_not_hide_shell() {
        let mut shell = test_shell(
            vec![win(1, Shell::APP_ID, "menubar")],
            Some(Shell::APP_ID),
            &[],
        );
        shell.hide_focused_app();
        assert!(!shell.is_app_hidden(Shell::APP_ID));
        assert_eq!(shell.focused_app_id.as_deref(), Some(Shell::APP_ID));
        assert_eq!(composed_apps(&shell), vec![1]);
    }

    #[test]
    fn hide_last_app_clears_focus() {
        let mut shell = test_shell(
            vec![
                win(1, Shell::APP_ID, "menubar"),
                win(2, "sola-terminal", "Terminal"),
            ],
            Some("sola-terminal"),
            &["sola-terminal"],
        );
        shell.hide_focused_app();
        assert!(shell.is_app_hidden("sola-terminal"));
        assert!(shell.focused_app_id.is_none());
        assert!(shell.focused_window_id.is_none());
        assert_eq!(composed_apps(&shell), vec![1]);
    }

    #[test]
    fn unhide_restores_composition_and_focus() {
        let mut shell = desktop();
        shell.hide_focused_app();
        assert!(shell.is_app_hidden("sola-terminal"));

        shell.unhide_app("sola-terminal");

        assert!(!shell.is_app_hidden("sola-terminal"));
        assert_eq!(shell.focused_app_id.as_deref(), Some("sola-terminal"));
        let composed = composed_apps(&shell);
        assert!(composed.contains(&2), "terminal must be in composition");
        assert_eq!(*composed.last().unwrap(), 2, "unhide raises to top");
    }

    #[test]
    fn raise_app_unhides() {
        let mut shell = desktop();
        shell.hide_focused_app();
        assert!(shell.is_app_hidden("sola-terminal"));

        shell.raise_app("sola-terminal");

        assert!(!shell.is_app_hidden("sola-terminal"));
        assert_eq!(shell.focused_app_id.as_deref(), Some("sola-terminal"));
    }
}
