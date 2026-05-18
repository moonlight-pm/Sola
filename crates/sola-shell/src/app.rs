use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde_json::Value;
use sola_kit::{AppCtx, AppRuntimeHandle, BusRegistry, SolaApp, WindowConfig, WindowHandle};
use sola_bus::topics::{
    AppMenuPayload, ApplicationsConfig, CompositionEntry, FocusTarget, FrameUpdate, KeyChord,
    LaunchResultPayload, MenuDefinition, MenuItem, MouseClickedPayload, MouseEnteredPayload,
    RegisteredChord, Topic, TopicKind, UserAppExitedPayload, Window,
};
use sola_core::KeyCode;
use sola_core::applications::{Application, builtin_apps, is_builtin};

/// Initial `applications` list — just the built-ins. User entries
/// arrive later as `Topic::Application` sticky replays from the bus
/// and get appended in `on_application`.
fn initial_applications() -> ApplicationsConfig {
    ApplicationsConfig {
        apps: builtin_apps(),
    }
}

/// Hover dwell before focus-follows-mouse switches focus. Below this,
/// sweeping the cursor across windows leaves focus where it was.
const FOCUS_HOVER_DELAY: Duration = Duration::from_millis(500);

use crate::launcher::{self, LAUNCHER_ASSETS, LauncherState};
use crate::menu::{MENU_ASSETS, MenuCache, SYNTHESIZED_CLOSE_ACTION, synthesized_menu};
use crate::menubar::setup_menubar;
use crate::switcher::{SWITCHER_ASSETS, SwitcherState};
use crate::zoning::{self, ZoningState};

pub struct ShellWindows {
    pub menubar: WindowHandle,
    pub menu: WindowHandle,
    pub switcher: WindowHandle,
    pub launcher: WindowHandle,
}

/// Lightweight app entry for the switcher (grouped by app_id).
#[derive(Clone)]
pub struct SwitcherApp {
    pub app_id: String,
}

pub struct ShellApp {
    pub focused_app_id: Option<String>,
    pub focused_window_id: Option<u32>,
    pub mru_apps: Vec<String>,
    /// Most-recently-focused window per app, for switcher restore.
    pub mru_window_by_app: HashMap<String, u32>,
    /// All known windows from sola-river, keyed by window_id.
    pub known_windows: Vec<Window>,
    /// Maps (app_id, title) to window_id for lookup.
    pub window_id_by_key: HashMap<(String, String), u32>,
    pub applications: ApplicationsConfig,
    pub menus: MenuCache,
    pub zoning: ZoningState,
    pub switcher: SwitcherState,
    pub launcher: LauncherState,
    pub menu_open: bool,
    pub windows: ShellWindows,
    /// Pending focus-follows-mouse timer. `Some(gen)` means a timer with that
    /// generation is in flight; `None` means none is pending.
    /// `cef::post_delayed_task` has no cancel API, so we use a monotonic
    /// generation counter: each new timer increments the counter and captures
    /// its generation; the callback short-circuits if its generation no longer
    /// matches (i.e. a newer timer has been scheduled, or the timer was
    /// cancelled).
    pub pending_focus_source: Option<u64>,
    /// Monotonically-increasing generation counter for focus-hover timers.
    /// Incremented on every `schedule_focus_from_pointer` call so that stale
    /// callbacks can detect they've been superseded.
    pub pending_focus_generation: u64,
    /// Captured in `after_runtime_ready`. Used by hover timers to
    /// re-enter app state via `schedule_after`.
    pub runtime: Option<AppRuntimeHandle<Self>>,
}

impl SolaApp for ShellApp {
    const APP_ID: &'static str = "sola-shell";

    fn new(ctx: &mut AppCtx) -> Self {
        let menubar_initial = serde_json::json!({ "focused": null });
        let menubar = setup_menubar(ctx, menubar_initial);

        let switcher = ctx.add_window(WindowConfig {
            title: "switcher".into(),
            size: (800, 400),
            position: Some((560, 340)),
            decorated: false,
            transparent: true,
            assets: SWITCHER_ASSETS,
            initial_state: None,
            zoned: false,
            keyboard_target: false,
            root_component: Some("/switcher.tsx"),
        });

        let menu = ctx.add_window(WindowConfig {
            title: "menu".into(),
            size: (220, 300),
            position: Some((0, zoning::MENUBAR_HEIGHT)),
            decorated: false,
            transparent: true,
            assets: MENU_ASSETS,
            initial_state: None,
            zoned: false,
            keyboard_target: false,
            root_component: Some("/menu.tsx"),
        });

        let launcher = ctx.add_window(WindowConfig {
            title: "launcher".into(),
            size: (launcher::WIDTH, launcher::HEIGHT),
            position: Some((700, 340)),
            decorated: false,
            transparent: true,
            assets: LAUNCHER_ASSETS,
            initial_state: None,
            zoned: false,
            keyboard_target: true,
            root_component: Some("/launcher.tsx"),
        });

        let mut menus = MenuCache::new();
        // Register the shell's system menu (for shortcut lookup).
        menus.set_menu(AppMenuPayload {
            app_id: Self::APP_ID.into(),
            menus: vec![MenuDefinition {
                label: "Sola".into(),
                items: vec![MenuItem::Action {
                    id: "exit".into(),
                    label: "Exit Sola".into(),
                    shortcut: Some(KeyCode::BACKSPACE.meta().shift()),
                    disabled: false,
                    checked: false,
                }],
            }],
        });

        let app = Self {
            focused_app_id: None,
            focused_window_id: None,
            mru_apps: Vec::new(),
            mru_window_by_app: HashMap::new(),
            known_windows: Vec::new(),
            window_id_by_key: HashMap::new(),
            applications: initial_applications(),
            menus,
            zoning: ZoningState::new(),
            switcher: SwitcherState::default(),
            launcher: LauncherState::default(),
            menu_open: false,
            windows: ShellWindows {
                menubar,
                menu,
                switcher,
                launcher,
            },
            pending_focus_source: None,
            pending_focus_generation: 0,
            runtime: None,
        };

        app.emit_registered_chords(ctx);

        // Publish the merged kit + shell theme so every kit window receives
        // both the kit component vars and the shell's --sola-menubar-* vars.
        let mut theme = sola_kit::theme::kit_default_theme();
        theme.components.extend(crate::theme::shell_default_bindings());
        ctx.emit(Topic::Theme(theme));

        app
    }

    fn after_runtime_ready(&mut self, handle: AppRuntimeHandle<Self>, _ctx: &mut AppCtx) {
        self.runtime = Some(handle);
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        // Default CloseApp handler is inherited from the trait — don't re-register.
        bus.on(TopicKind::Windows, Self::on_windows);
        bus.on(TopicKind::Zones, Self::on_zones);
        bus.on(TopicKind::SetAppMenu, Self::on_set_app_menu);
        bus.on(TopicKind::OutputGeometry, Self::on_output_geometry);
        bus.on(TopicKind::MouseEntered, Self::on_mouse_entered);
        bus.on(TopicKind::MouseClicked, Self::on_mouse_clicked);
        bus.on(TopicKind::MouseLeft, Self::on_mouse_left);
        bus.on(TopicKind::Chord, Self::on_chord);
        bus.on(TopicKind::ChordReleased, Self::on_chord_released);
        bus.on(TopicKind::LaunchResult, Self::on_launch_result);
        bus.on(TopicKind::UserAppExited, Self::on_user_app_exited);
        bus.on(TopicKind::Application, Self::on_application);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        _id: Option<u64>,
        source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        match (source.title(), cmd) {
            ("menubar", "open_menu") => {
                let src = args.get("source").and_then(|v| v.as_str()).unwrap_or("app");
                let index = args.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let anchor_x = args.get("anchor_x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                self.open_menu(src, index, anchor_x, ctx);
            }
            ("menubar", "close_menu") => self.close_menu(ctx),
            ("menu", "dismiss") => self.close_menu(ctx),
            ("menu", "action") => {
                let app_id = args.get("app_id").and_then(|v| v.as_str()).unwrap_or("");
                let action_id = args.get("action_id").and_then(|v| v.as_str()).unwrap_or("");
                tracing::info!(app_id, action_id, "menu action");
                if app_id == Self::APP_ID && action_id == "exit" {
                    ctx.emit(Topic::Shutdown);
                } else if action_id == SYNTHESIZED_CLOSE_ACTION {
                    ctx.emit(Topic::CloseApp(app_id.to_string()));
                } else {
                    ctx.emit(Topic::MenuAction(sola_bus::topics::MenuActionPayload {
                        app_id: app_id.to_string(),
                        action_id: action_id.to_string(),
                    }));
                }
                self.close_menu(ctx);
            }
            ("switcher", "select") => {
                let index = args.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if self.switcher.active && index < self.switcher.apps.len() {
                    self.switcher.selected = index;
                }
            }
            ("launcher", "query") => {
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
                self.launcher.apply_query(&self.applications, text);
                self.render_launcher();
            }
            ("launcher", "nav") => {
                let max = self.launcher.filtered_ids.len().saturating_sub(1);
                let cur = self.launcher.selected;
                let new_sel = if let Some(idx) = args.get("index").and_then(|v| v.as_u64()) {
                    (idx as usize).min(max)
                } else if let Some(dir) = args.get("dir").and_then(|v| v.as_str()) {
                    match dir {
                        "up" => cur.saturating_sub(1),
                        "down" => (cur + 1).min(max),
                        _ => cur,
                    }
                } else {
                    cur
                };
                if new_sel != cur {
                    self.launcher.selected = new_sel;
                    self.render_launcher();
                }
            }
            ("launcher", "launch") => {
                let explicit = args.get("app_id").and_then(|v| v.as_str());
                let app_id = explicit
                    .map(str::to_string)
                    .or_else(|| {
                        self.launcher
                            .filtered_ids
                            .get(self.launcher.selected)
                            .cloned()
                    })
                    .unwrap_or_default();
                tracing::info!(
                    %app_id,
                    explicit = explicit.is_some(),
                    "launcher cmd 'launch' received"
                );
                self.launch_and_close(&app_id, ctx);
            }
            ("launcher", "close") => {
                self.close_launcher(ctx);
            }
            _ => {}
        }
    }
}

impl ShellApp {
    fn on_windows(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::Windows(windows) = delivery.topic else {
            return;
        };
        self.handle_windows_update(windows.clone(), ctx);
        if self.switcher.active {
            let json = self.switcher_apps_json();
            self.windows.switcher.eval_js(&format!(
                "renderSwitcher({}, {})",
                json, self.switcher.selected
            ));
        }
    }

    fn on_zones(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::Zones(zones) = delivery.topic else {
            return;
        };
        tracing::info!(count = zones.len(), "zones updated");
        self.zoning.set_zones(zones.clone());
        // Re-apply config zones to any windows already known. New
        // windows that arrive after this will pick up the mapping via
        // apply_config_zone in handle_windows_update.
        let windows: Vec<(String, u32)> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| (w.app_id.clone(), w.window_id))
            .collect();
        for (app_id, wid) in windows {
            if let Some(frame) = self.zoning.apply_config_zone(&app_id, wid) {
                ctx.emit(Topic::Frame(frame));
            }
        }
    }

    fn on_set_app_menu(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::SetAppMenu(payload) = delivery.topic else {
            return;
        };
        self.menus.set_menu(payload.clone());

        self.emit_registered_chords(ctx);

        if self.focused_app_id.as_deref() == Some(&payload.app_id) {
            let app_name = payload
                .menus
                .first()
                .map(|d| d.label.as_str())
                .unwrap_or(&payload.app_id);
            let menu_labels: Vec<String> = payload.menus.iter().map(|d| d.label.clone()).collect();
            self.windows.menubar.send_to_js(&serde_json::json!({
                "event": "focus",
                "app_name": app_name,
                "menu_labels": menu_labels,
            }));
        }
    }

    fn on_output_geometry(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::OutputGeometry(geo) = delivery.topic else {
            return;
        };
        self.zoning.set_output_size(geo);
        self.emit_all_frames(ctx);
        self.emit_composition(ctx);
    }

    fn on_mouse_entered(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::MouseEntered(MouseEnteredPayload { window_id }) = delivery.topic else {
            return;
        };
        self.schedule_focus_from_pointer(*window_id);
    }

    fn on_mouse_clicked(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::MouseClicked(MouseClickedPayload { window_id }) = delivery.topic else {
            return;
        };
        // A click is an explicit signal — bypass the hover dwell and
        // focus immediately, dropping any pending hover timer.
        self.cancel_pending_focus();
        // While a menubar menu is open we run in macOS-style menu mode:
        // a click on any non-shell window (i.e. outside the menubar and
        // dropdown surfaces) dismisses the menu. Clicks landing on the
        // menu/menubar surfaces are handled by their own JS.
        if self.menu_open {
            let on_shell = self
                .known_windows
                .iter()
                .find(|w| w.window_id == *window_id)
                .map(|w| w.app_id == Self::APP_ID)
                .unwrap_or(false);
            if !on_shell {
                self.close_menu(ctx);
            }
            return;
        }
        self.focus_window_from_pointer(*window_id, ctx);
    }

    fn on_mouse_left(&mut self, _delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        // Cursor left all known windows — drop any pending hover focus
        // so we don't switch into an app the user has moved away from.
        self.cancel_pending_focus();
    }

    fn on_chord(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::Chord(evt) = delivery.topic else { return };
        crate::keys::handle_chord(self, ctx, evt.clone());
    }

    fn on_chord_released(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::ChordReleased(evt) = delivery.topic else {
            return;
        };
        crate::keys::handle_chord_released(self, ctx, evt.clone());
    }

    fn on_launch_result(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::LaunchResult(LaunchResultPayload {
            app_id: _,
            command,
            ok,
            error,
        }) = delivery.topic
        else {
            return;
        };
        if *ok {
            tracing::info!(command = %command, "LaunchResult ok");
        } else {
            tracing::warn!(
                command = %command,
                error = error.as_deref().unwrap_or(""),
                "LaunchResult failed"
            );
            let err_msg = error.as_deref().unwrap_or("launch failed");
            self.push_toast(&format!("{command}: {err_msg}"));
        }
    }

    fn on_user_app_exited(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::UserAppExited(UserAppExitedPayload {
            app_id: _,
            command,
            code,
            signal,
        }) = delivery.topic
        else {
            return;
        };
        let detail = match (code, signal) {
            (Some(c), _) => format!("exit {c}"),
            (_, Some(s)) => format!("signal {s}"),
            _ => "exited".to_string(),
        };
        tracing::warn!(
            command = %command,
            code = ?code,
            signal = ?signal,
            "user app exited"
        );
        self.push_toast(&format!("{command} — {detail}"));
    }

    fn on_application(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Application(app) = delivery.topic else {
            return;
        };
        // Built-ins live in code; the bus must not be able to shadow
        // them or remove them with a stray emit/retract.
        if is_builtin(&app.app_id) {
            return;
        }
        self.applications.remove(&app.app_id);
        if !delivery.retracted {
            self.applications.apps.push(app.clone());
        }
        // Re-render the launcher if it's currently open so a newly
        // added app shows up without the user having to retype.
        if self.launcher.active {
            let query = self.launcher.query.clone();
            self.launcher.apply_query(&self.applications, &query);
            self.render_launcher();
        }
    }

    /// Look up a configured application by its `app_id`.
    pub fn application(&self, app_id: &str) -> Option<&Application> {
        self.applications.get(app_id)
    }

    /// Icon reference (`"<pack>/<name>"`) for an application, if configured.
    pub fn icon_for(&self, app_id: &str) -> Option<&str> {
        self.application(app_id).map(|a| a.icon.as_str())
    }

    /// JSON payload of the switcher's apps, with `icon` resolved against the
    /// `applications` registry. Used in place of raw `switcher.apps` JSON so
    /// the overlay can render real icons.
    pub fn switcher_apps_json(&self) -> String {
        let entries: Vec<Value> = self
            .switcher
            .apps
            .iter()
            .map(|app| {
                let icon = self
                    .icon_for(&app.app_id)
                    .map(String::from)
                    .unwrap_or_else(|| "app".to_string());
                let window_count = self
                    .known_windows
                    .iter()
                    .filter(|w| w.app_id == app.app_id)
                    .count() as u32;
                serde_json::json!({
                    "app_id": app.app_id,
                    "name": self.display_label(&app.app_id),
                    "icon": icon,
                    "window_count": window_count,
                })
            })
            .collect();
        serde_json::to_string(&entries).unwrap_or_default()
    }

    pub fn emit_registered_chords(&self, ctx: &mut AppCtx) {
        let source = self.shell_key_chords();
        let mut chords: Vec<RegisteredChord> = Vec::with_capacity(source.len() * 2 + 2);
        for c in &source {
            chords.push(crate::keys::to_registered(c));
            // Numpad keys have a different keysym when NumLock is off;
            // register both so zoning fires regardless of state.
            if let Some(alt) = crate::keys::to_registered_alt(c) {
                chords.push(alt);
            }
        }
        // Register bare Super_L with no modifiers so we get a released
        // event when the user lets the Super key go. Used to confirm the
        // app switcher (Meta+Tab opens, Meta release selects).
        chords.push(RegisteredChord {
            keysym: crate::keys::KEYSYM_SUPER_L,
            modifiers: 0,
        });
        // While a shell overlay is active, grab Escape so the user can
        // dismiss with one key regardless of which WebView owns DOM focus.
        // Deregistered as soon as the overlay closes so terminal apps
        // (vim, less, etc.) keep their Escape.
        if self.launcher.active || self.switcher.active || self.menu_open {
            chords.push(RegisteredChord {
                keysym: crate::keys::KEYSYM_ESCAPE,
                modifiers: 0,
            });
        }
        chords.sort_by_key(|c| (c.modifiers, c.keysym));
        chords.dedup();
        ctx.emit(Topic::RegisteredChords(chords));
    }

    fn shell_key_chords(&self) -> Vec<KeyChord> {
        // Always include the shell's own meta-bound shortcuts (e.g. Exit
        // Sola). Then add the focused Sola app's meta shortcuts — and ONLY
        // that app's. Registering every cached app's chords globally
        // means River grabs them no matter who has focus, swallowing
        // chords like Cmd+W when a non-Sola client (e.g. Zed) is focused.
        let mut bindings: Vec<KeyChord> = self
            .menus
            .key_bindings_for(Self::APP_ID)
            .into_iter()
            .filter(|b| b.meta)
            .collect();
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

        // Meta+Tab activates switcher.
        bindings.push(KeyCode::TAB.meta());

        // Meta+` cycles through the focused app's own windows.
        bindings.push(KeyCode::GRAVE.meta());

        // Meta+Space toggles launcher.
        bindings.push(KeyCode::SPACE.meta());

        // Meta+Q: close focused app.
        bindings.push(KeyCode::Q.meta());

        // Meta+Numpad zones a window.
        for &keycode in zoning::ZONING_KEYCODES {
            bindings.push(KeyChord {
                keycode: keycode.into(),
                ..KeyCode::TAB.meta()
            });
        }

        bindings.sort_by_key(|b| (b.keycode, b.meta, b.alt, b.ctrl, b.shift));
        bindings.dedup();

        bindings
    }

    /// Drop any pending hover-focus timer.
    fn cancel_pending_focus(&mut self) {
        // Clear the flag. Any already-scheduled callback checks its captured
        // generation against pending_focus_source on arrival and becomes a
        // no-op if they differ.
        // NOTE: We do NOT reset pending_focus_generation here — it must keep
        // monotonically increasing so old captured generations stay stale.
        self.pending_focus_source = None;
    }

    /// Schedule focus-follows-mouse for `window_id` after `FOCUS_HOVER_DELAY`.
    /// Re-entries cancel the previous timer; the cursor must dwell on a
    /// single window for the full delay before focus actually moves.
    fn schedule_focus_from_pointer(&mut self, window_id: u32) {
        self.cancel_pending_focus();

        // Skip surfaces that wouldn't focus anyway: shell chrome, our own
        // overlays' active state, or the already-focused window.
        let Some(info) = self.known_windows.iter().find(|w| w.window_id == window_id) else {
            return;
        };
        if info.app_id == Self::APP_ID {
            return;
        }
        if self.menu_open || self.switcher.active || self.launcher.active {
            return;
        }
        if self.focused_window_id == Some(window_id) {
            return;
        }

        let Some(handle) = self.runtime.clone() else {
            return;
        };

        // Increment and capture the generation BEFORE posting the closure so
        // the closure captures the value that was current when this timer was
        // scheduled. cef::post_delayed_task has no cancel API, so we rely on
        // generation matching to detect stale callbacks.
        self.pending_focus_generation = self.pending_focus_generation.wrapping_add(1);
        let timer_gen = self.pending_focus_generation;
        self.pending_focus_source = Some(timer_gen);

        let delay_ms = FOCUS_HOVER_DELAY.as_millis() as u64;
        handle.schedule_after(delay_ms, move |app, ctx| {
            // Stale-callback guard: bail if a newer timer has been scheduled
            // (pending_focus_source holds a different generation) or if the
            // timer was cancelled (pending_focus_source is None).
            if app.pending_focus_source != Some(timer_gen) {
                return;
            }
            app.pending_focus_source = None;
            app.focus_window_from_pointer(window_id, ctx);
        });
    }

    fn focus_window_from_pointer(&mut self, window_id: u32, ctx: &mut AppCtx) {
        let Some(info) = self.known_windows.iter().find(|w| w.window_id == window_id) else {
            return;
        };
        if info.app_id == Self::APP_ID {
            return;
        }
        if self.menu_open || self.switcher.active || self.launcher.active {
            return;
        }
        let app_id = info.app_id.clone();
        self.set_focus(&app_id, ctx);
        self.focused_window_id = Some(window_id);
        self.mru_window_by_app.insert(app_id, window_id);
        ctx.emit(Topic::Focus(FocusTarget { window_id }));
        self.emit_composition(ctx);
    }

    /// Advance keyboard focus to the next window of the currently
    /// focused app. No-op if there are fewer than two such windows.
    /// Wraps around in `known_windows` order.
    pub fn cycle_focused_app_windows(&mut self, ctx: &mut AppCtx) {
        let Some(app_id) = self.focused_app_id.clone() else {
            return;
        };
        let windows: Vec<u32> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id == app_id)
            .map(|w| w.window_id)
            .collect();
        if windows.len() < 2 {
            return;
        }
        let cur_idx = self
            .focused_window_id
            .and_then(|cur| windows.iter().position(|w| *w == cur))
            .unwrap_or(0);
        let next_wid = windows[(cur_idx + 1) % windows.len()];
        self.focused_window_id = Some(next_wid);
        self.mru_window_by_app.insert(app_id, next_wid);
        ctx.emit(Topic::Focus(FocusTarget {
            window_id: next_wid,
        }));
        self.emit_composition(ctx);
    }

    pub fn set_focus(&mut self, app_id: &str, ctx: &mut AppCtx) {
        let app_changed = self.focused_app_id.as_deref() != Some(app_id);
        self.focused_app_id = Some(app_id.to_string());
        self.zoning.set_focused(app_id.to_string());
        self.mru_apps.retain(|m| m != app_id);
        self.mru_apps.insert(0, app_id.to_string());

        // Close any open menu — the focus event to JS handles the menubar UI.
        if self.menu_open {
            self.menu_open = false;
            self.windows.menu.eval_js("clearMenu()");
        }

        // Per-app menu chords are only registered with River while their
        // app is focused — otherwise (e.g.) sola-browser's Meta+W would
        // be intercepted while a non-Sola client like Zed is focused.
        if app_changed {
            self.emit_registered_chords(ctx);
        }

        let synthesized;
        let menu = match self.menus.get_menu(app_id) {
            Some(m) => m,
            None => {
                synthesized = synthesized_menu(app_id, &self.display_label(app_id));
                &synthesized
            }
        };
        let app_name = menu
            .menus
            .first()
            .map(|d| d.label.as_str())
            .unwrap_or(app_id);
        let menu_labels: Vec<String> = menu.menus.iter().map(|d| d.label.clone()).collect();

        self.windows.menubar.send_to_js(&serde_json::json!({
            "event": "focus",
            "app_name": app_name,
            "menu_labels": menu_labels,
        }));
    }

    /// Resolve a human-readable label for an app_id. Falls back to the
    /// app_id itself title-cased if no `applications` entry exists.
    pub fn display_label(&self, app_id: &str) -> String {
        if let Some(app) = self.applications.get(app_id) {
            return app.label.clone();
        }
        let mut chars = app_id.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => String::new(),
        }
    }

    /// Build a deduplicated list of app_ids for the switcher, ordered by MRU.
    pub fn rebuild_switcher_apps(&self) -> Vec<SwitcherApp> {
        let unique_app_ids: Vec<String> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| w.app_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let mut apps: Vec<SwitcherApp> = self
            .mru_apps
            .iter()
            .filter(|id| unique_app_ids.contains(id))
            .map(|id| SwitcherApp { app_id: id.clone() })
            .collect();
        // Append any known apps not yet in MRU.
        for id in &unique_app_ids {
            if !self.mru_apps.contains(id) {
                apps.push(SwitcherApp { app_id: id.clone() });
            }
        }
        apps
    }

    /// Look up a window_id from the known windows list by (app_id, title).
    pub fn lookup_window_id(&self, app_id: &str, title: &str) -> Option<u32> {
        self.window_id_by_key
            .get(&(app_id.to_string(), title.to_string()))
            .copied()
    }

    /// Look up any window_id for an app_id (first match).
    pub fn lookup_any_window_id(&self, app_id: &str) -> Option<u32> {
        self.known_windows
            .iter()
            .find(|w| w.app_id == app_id)
            .map(|w| w.window_id)
    }

    /// Build the composition list (bottom to top) and emit it.
    pub fn emit_composition(&self, ctx: &mut AppCtx) {
        let mut entries = Vec::new();

        // 1. Shell menubar — always at the bottom.
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menubar") {
            entries.push(CompositionEntry { window_id: wid });
        }

        // 2. App windows ordered by MRU (least recent first = bottom of stack).
        // Within each app, the per-app MRU window stacks last so it sits on
        // top of its siblings — important for Meta+` cycling and any time
        // the same app has multiple overlapping windows.
        let mut seen_app_ids = HashSet::new();
        for app_id in self.mru_apps.iter().rev() {
            if app_id == Self::APP_ID {
                continue;
            }
            seen_app_ids.insert(app_id.clone());
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

        // Apps not yet in MRU.
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID || seen_app_ids.contains(&w.app_id) {
                continue;
            }
            entries.push(CompositionEntry {
                window_id: w.window_id,
            });
        }

        // 3. Shell panels on top when active.
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

        ctx.emit(Topic::Composition(entries));
    }

    /// Emit Frame updates for all managed windows.
    ///
    /// - Menubar: full width, fixed height.
    /// - Sola apps (sola-*): zoned frame, or full-screen-below-menubar default.
    /// - External apps: zoned frame only. No frame if unzoned (self-positioning).
    pub fn emit_all_frames(&self, ctx: &mut AppCtx) {
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menubar") {
            if let Some(frame) = self.zoning.menubar_frame(wid) {
                ctx.emit(Topic::Frame(frame));
            }
        }
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID {
                continue;
            }
            if let Some(frame) = self.zoning.window_frame(w.window_id) {
                ctx.emit(Topic::Frame(frame));
            } else if w.app_id.starts_with("sola-") {
                // Sola apps get full-screen-below-menubar by default.
                if let Some(frame) = self.zoning.default_app_frame(w.window_id) {
                    ctx.emit(Topic::Frame(frame));
                }
            }
        }
    }

    /// Handle new/removed windows from sola-river.
    pub fn handle_windows_update(&mut self, windows: Vec<Window>, ctx: &mut AppCtx) {
        tracing::info!(count = windows.len(), "shell received Windows");
        let old_app_ids: HashSet<String> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| w.app_id.clone())
            .collect();
        let new_app_ids: HashSet<String> = windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| w.app_id.clone())
            .collect();

        let added: Vec<String> = new_app_ids
            .iter()
            .filter(|id| !old_app_ids.contains(id.as_str()))
            .cloned()
            .collect();

        let removed: Vec<String> = old_app_ids
            .iter()
            .filter(|id| !new_app_ids.contains(id.as_str()))
            .cloned()
            .collect();

        // Rebuild lookup map.
        self.window_id_by_key.clear();
        for w in &windows {
            self.window_id_by_key
                .insert((w.app_id.clone(), w.title.clone()), w.window_id);
        }

        self.known_windows = windows;

        // Rebuild switcher apps (unique app_ids, excluding shell).
        self.switcher.apps = {
            let mut seen = HashSet::new();
            self.known_windows
                .iter()
                .filter(|w| w.app_id != Self::APP_ID && seen.insert(w.app_id.clone()))
                .map(|w| SwitcherApp {
                    app_id: w.app_id.clone(),
                })
                .collect()
        };

        let focused_app_was_removed = self
            .focused_app_id
            .as_deref()
            .map(|f| removed.iter().any(|r| r == f))
            .unwrap_or(false);
        for id in &removed {
            self.mru_apps.retain(|m| m != id);
            self.mru_window_by_app.remove(id);
            self.zoning.forget_app(id);
            if self.focused_app_id.as_deref() == Some(id.as_str()) {
                self.focused_app_id = None;
                self.focused_window_id = None;
            }
        }

        // Clean up zone tracking for removed windows.
        let current_wids: HashSet<u32> = self.known_windows.iter().map(|w| w.window_id).collect();
        self.zoning
            .window_zones
            .retain(|wid, _| current_wids.contains(wid));

        // For newly appeared apps, apply their saved zone config to the
        // first window. Subsequent windows of the same app are unzoned.
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID {
                continue;
            }
            if let Some(frame) = self.zoning.apply_config_zone(&w.app_id, w.window_id) {
                ctx.emit(Topic::Frame(frame));
            }
        }

        // Emit frames for menubar and all explicitly-zoned windows.
        self.emit_all_frames(ctx);
        self.emit_composition(ctx);

        // Focus the newest app so the user can start using it immediately.
        // If no new app appeared but the focused app was just closed, fall
        // back to the next MRU app — or clear the menubar entirely if there
        // are no apps left.
        if let Some(id) = added.first() {
            self.set_focus(id, ctx);
            if let Some(wid) = self.lookup_any_window_id(id) {
                self.focused_window_id = Some(wid);
                self.mru_window_by_app.insert(id.clone(), wid);
                ctx.emit(Topic::Focus(FocusTarget { window_id: wid }));
            }
            self.emit_composition(ctx);
        } else if focused_app_was_removed {
            if let Some(next) = self.mru_apps.first().cloned() {
                self.set_focus(&next, ctx);
                let wid = self
                    .mru_window_by_app
                    .get(&next)
                    .copied()
                    .or_else(|| self.lookup_any_window_id(&next));
                if let Some(wid) = wid {
                    self.focused_window_id = Some(wid);
                    self.mru_window_by_app.insert(next.clone(), wid);
                    ctx.emit(Topic::Focus(FocusTarget { window_id: wid }));
                }
                self.emit_composition(ctx);
            } else {
                self.clear_menubar_focus();
                // No focused app left — drop any per-app chords that were
                // still registered with River.
                self.emit_registered_chords(ctx);
            }
        }
    }

    /// Tell the menubar there's no focused app — clears the app name and
    /// menu labels. Used when the last app closes.
    fn clear_menubar_focus(&self) {
        self.windows.menubar.send_to_js(&serde_json::json!({
            "event": "focus",
            "app_name": "",
            "menu_labels": [],
        }));
    }

    pub fn open_menu(&mut self, source: &str, menu_index: usize, anchor_x: f64, ctx: &mut AppCtx) {
        let app_id = if source == "system" {
            Self::APP_ID.to_string()
        } else {
            self.focused_app_id.clone().unwrap_or_default()
        };

        let synthesized;
        let menu = match self.menus.get_menu(&app_id) {
            Some(m) => m,
            None if app_id != Self::APP_ID && !app_id.is_empty() => {
                synthesized = synthesized_menu(&app_id, &self.display_label(&app_id));
                &synthesized
            }
            None => return,
        };
        let Some(menu_def) = menu.menus.get(menu_index) else {
            return;
        };

        let items: Vec<Value> = menu_def
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Action {
                    id,
                    label,
                    shortcut,
                    disabled,
                    ..
                } => serde_json::json!({
                    "type": "action",
                    "id": id,
                    "app_id": app_id,
                    "label": label,
                    "shortcut": shortcut.as_ref().map(|c| c.display()),
                    "disabled": disabled,
                }),
                MenuItem::Divider => serde_json::json!({ "type": "divider" }),
            })
            .collect();

        let json = serde_json::to_string(&items).unwrap_or_default();
        self.windows
            .menu
            .eval_js(&format!("showMenu({}, {})", json, anchor_x));

        // Full-screen overlay below the menubar — transparent except the dropdown.
        if let (Some((ow, oh)), Some(wid)) = (
            self.zoning.output_size,
            self.lookup_window_id(Self::APP_ID, "menu"),
        ) {
            ctx.emit(Topic::Frame(FrameUpdate {
                window_id: wid,
                x: 0,
                y: zoning::MENUBAR_HEIGHT,
                width: ow,
                height: oh - zoning::MENUBAR_HEIGHT,
                fullscreen: false,
            }));
        }

        self.menu_open = true;
        self.emit_registered_chords(ctx);
        self.emit_composition(ctx);
    }

    pub fn close_menu(&mut self, ctx: &mut AppCtx) {
        if !self.menu_open {
            return;
        }
        self.menu_open = false;
        self.emit_registered_chords(ctx);
        self.windows.menu.eval_js("clearMenu()");
        self.windows
            .menubar
            .send_to_js(&serde_json::json!({"event": "close_menu"}));
        self.emit_composition(ctx);
    }

    pub fn open_launcher(&mut self, ctx: &mut AppCtx) {
        tracing::info!(
            already_active = self.launcher.active,
            prior_focus = ?self.focused_window_id,
            "open_launcher"
        );
        if self.launcher.active {
            return;
        }

        // Snapshot the focus target we'll restore on close.
        self.launcher.prior_focus = self.focused_window_id;

        self.launcher.active = true;
        self.emit_registered_chords(ctx);
        self.launcher.apply_query(&self.applications, "");

        // Launcher is a fullscreen-below-menubar overlay. The visible
        // panel is centered by the CSS; the rest is transparent and
        // absorbs pointer/scroll events so nothing beneath the launcher
        // can be interacted with while it's open.
        if let (Some((ow, oh)), Some(wid)) = (
            self.zoning.output_size,
            self.lookup_window_id(Self::APP_ID, "launcher"),
        ) {
            ctx.emit(Topic::Frame(FrameUpdate {
                window_id: wid,
                x: 0,
                y: zoning::MENUBAR_HEIGHT,
                width: ow,
                height: oh - zoning::MENUBAR_HEIGHT,
                fullscreen: false,
            }));
        }

        self.emit_composition(ctx);

        // Route keyboard to the launcher window.
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "launcher") {
            ctx.emit(Topic::Focus(FocusTarget { window_id: wid }));
        }

        self.windows.launcher.eval_js("resetForOpen()");
        self.render_launcher();
    }

    pub fn close_launcher(&mut self, ctx: &mut AppCtx) {
        tracing::info!(
            was_active = self.launcher.active,
            prior_focus = ?self.launcher.prior_focus,
            "close_launcher"
        );
        if !self.launcher.active {
            return;
        }
        let prior_wid = self.launcher.prior_focus.take();
        self.launcher.active = false;
        self.emit_registered_chords(ctx);
        self.launcher.query.clear();
        self.launcher.filtered_ids.clear();
        self.launcher.selected = 0;

        self.emit_composition(ctx);

        if let Some(wid) = prior_wid {
            ctx.emit(Topic::Focus(FocusTarget { window_id: wid }));
        }
    }

    pub fn launch_and_close(&mut self, app_id: &str, ctx: &mut AppCtx) {
        tracing::info!(app_id, "launch_and_close");
        if let Some(app) = self.applications.get(app_id) {
            tracing::info!(app_id, command = %app.command, "emitting LaunchApp");
            ctx.emit(Topic::LaunchApp(sola_bus::topics::LaunchAppPayload {
                app_id: app_id.to_string(),
                command: app.command.clone(),
            }));
        } else {
            tracing::warn!(app_id, "launch requested for unknown application");
        }
        self.close_launcher(ctx);
    }

    fn render_launcher(&self) {
        let json = launcher::state::render_json(&self.applications, &self.launcher.filtered_ids);
        let js = format!("renderApps({}, {})", json, self.launcher.selected);
        self.windows.launcher.eval_js(&js);
    }

    /// Emit `CloseApp` for the currently focused window, unless a shell overlay
    /// is active or the focused surface is the shell itself.
    pub fn close_focused_app(&mut self, ctx: &mut AppCtx) {
        if self.launcher.active || self.switcher.active || self.menu_open {
            return;
        }
        let Some(wid) = self.focused_window_id else {
            return;
        };
        let Some(win) = self.known_windows.iter().find(|w| w.window_id == wid) else {
            tracing::warn!(wid, "Meta+Q: focused_window_id not in known_windows");
            return;
        };
        let app_id = win.app_id.clone();
        if app_id == Self::APP_ID {
            return;
        }
        tracing::info!(%app_id, "Meta+Q — emitting CloseApp");
        let _ = ctx.emit(Topic::CloseApp(app_id));
    }

    /// Show a transient toast message in the menubar.
    fn push_toast(&self, message: &str) {
        self.windows.menubar.send_to_js(&serde_json::json!({
            "event": "toast",
            "message": message,
        }));
    }
}
