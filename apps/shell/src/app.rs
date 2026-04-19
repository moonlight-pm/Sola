use std::collections::{HashMap, HashSet};

use serde_json::Value;
use sola_app::config::JsonConfigIn;
use sola_app::{AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle};
use sola_bus::topics::{
    App, AppMenuPayload, CompositionEntry, FocusTarget, FrameUpdate, KeyChord,
    LaunchResultPayload, MenuDefinition, MenuItem, MouseClickedPayload,
    MouseEnteredPayload, RegisteredChord, Topic, TopicKind, UserAppExitedPayload,
};
use sola_core::KeyCode;

use crate::applications::{Application, ApplicationsConfig};
use crate::launcher::{self, LAUNCHER_ASSETS, LauncherState};
use crate::menu::{MENU_ASSETS, MenuCache};
use crate::menubar::setup_menubar;
use crate::session::{self, SessionEntry};
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
    pub known_windows: Vec<App>,
    /// Maps (app_id, title) to window_id for lookup.
    pub window_id_by_key: HashMap<(String, String), u32>,
    pub applications: ApplicationsConfig,
    pub session_entries: Vec<SessionEntry>,
    pub menus: MenuCache,
    pub zoning: ZoningState,
    pub switcher: SwitcherState,
    pub launcher: LauncherState,
    pub menu_open: bool,
    pub windows: ShellWindows,
}

impl SolaApp for ShellApp {
    const APP_ID: &'static str = "sola-shell";

    fn new(ctx: &mut AppCtx) -> Self {
        let menubar = setup_menubar(ctx);

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
            applications: ApplicationsConfig::load(),
            session_entries: session::load(),
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
        };

        app.emit_registered_chords(ctx);

        app
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        // Default CloseApp handler is inherited from the trait — don't re-register.
        bus.on(TopicKind::Apps, Self::on_apps);
        bus.on(TopicKind::SetAppMenu, Self::on_set_app_menu);
        bus.on(TopicKind::OutputGeometry, Self::on_output_geometry);
        bus.on(TopicKind::MouseEntered, Self::on_mouse_entered);
        bus.on(TopicKind::MouseClicked, Self::on_mouse_clicked);
        bus.on(TopicKind::MouseLeft, Self::on_mouse_left);
        bus.on(TopicKind::Chord, Self::on_chord);
        bus.on(TopicKind::ChordReleased, Self::on_chord_released);
        bus.on(TopicKind::LaunchResult, Self::on_launch_result);
        bus.on(TopicKind::UserAppExited, Self::on_user_app_exited);
        bus.on(TopicKind::ClientConnected, Self::on_client_connected);
        bus.on(TopicKind::ClientDisconnected, Self::on_client_disconnected);
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
                } else {
                    ctx.emit(Topic::MenuAction(sola_bus::topics::MenuActionPayload {
                        app_id: app_id.to_string(),
                        action_id: action_id.to_string(),
                    }));
                }
                self.close_menu(ctx);
            }
            ("launcher", "query") => {
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
                self.launcher.apply_query(&self.applications, text);
                self.render_launcher();
            }
            ("launcher", "nav") => {
                let max = self.launcher.filtered_ids.len().saturating_sub(1);
                let cur = self.launcher.selected;
                let new_sel = if let Some(idx) =
                    args.get("index").and_then(|v| v.as_u64())
                {
                    (idx as usize).min(max)
                } else if let Some(dir) =
                    args.get("dir").and_then(|v| v.as_str())
                {
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
    fn on_apps(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::Apps(apps) = topic else { return };
        self.handle_apps_update(apps.clone(), ctx);
        if self.switcher.active {
            let json = self.switcher_apps_json();
            self.windows.switcher.eval_js(&format!(
                "renderSwitcher({}, {})",
                json, self.switcher.selected
            ));
        }
    }

    fn on_set_app_menu(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::SetAppMenu(payload) = topic else { return };
        self.menus.set_menu(payload.clone());

        self.emit_registered_chords(ctx);

        if self.focused_app_id.as_deref() == Some(&payload.app_id) {
            let app_name = payload
                .menus
                .first()
                .map(|d| d.label.as_str())
                .unwrap_or(&payload.app_id);
            let menu_labels: Vec<String> =
                payload.menus.iter().map(|d| d.label.clone()).collect();
            self.windows.menubar.send_to_js(&serde_json::json!({
                "event": "focus",
                "app_name": app_name,
                "menu_labels": menu_labels,
            }));
        }
    }

    fn on_output_geometry(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::OutputGeometry(geo) = topic else { return };
        self.zoning.set_output_size(geo);
        self.emit_all_frames(ctx);
        self.emit_composition(ctx);
    }

    fn on_mouse_entered(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::MouseEntered(MouseEnteredPayload { window_id }) = topic else { return };
        self.focus_window_from_pointer(*window_id, ctx);
    }

    fn on_mouse_clicked(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::MouseClicked(MouseClickedPayload { window_id }) = topic else { return };
        self.focus_window_from_pointer(*window_id, ctx);
    }

    fn on_mouse_left(&mut self, _topic: &Topic, _ctx: &mut AppCtx) {}

    fn on_chord(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::Chord(evt) = topic else { return };
        crate::keys::handle_chord(self, ctx, evt.clone());
    }

    fn on_chord_released(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::ChordReleased(evt) = topic else { return };
        crate::keys::handle_chord_released(self, ctx, evt.clone());
    }

    fn on_launch_result(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::LaunchResult(LaunchResultPayload { app_id: _, command, ok, error }) = topic
            else { return };
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

    fn on_user_app_exited(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::UserAppExited(UserAppExitedPayload {
            app_id,
            command,
            code,
            signal,
        }) = topic else { return };
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

        // Authoritative "entry gone" signal: remove first matching entry.
        // Prefer a live one (window already closed); fall back to pending.
        let idx = self
            .session_entries
            .iter()
            .position(|e| e.app_id == *app_id && e.window_id.is_some())
            .or_else(|| {
                self.session_entries
                    .iter()
                    .position(|e| e.app_id == *app_id)
            });
        if let Some(i) = idx {
            self.session_entries.remove(i);
            session::save(&self.session_entries);
        }
    }

    fn on_client_connected(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::ClientConnected(app_id) = topic else { return };
        if app_id != "sola-session" {
            return;
        }

        // Relaunch pending entries only. Live entries (window_id: Some) stay live.
        let mut launches: Vec<(String, String)> = Vec::new();
        self.session_entries.retain(|e| {
            if e.window_id.is_some() {
                return true;
            }
            match self.applications.get(&e.app_id) {
                Some(app) => {
                    launches.push((e.app_id.clone(), app.command.clone()));
                    true
                }
                None => {
                    tracing::warn!(
                        app_id = %e.app_id,
                        "session entry not in applications.json; pruning"
                    );
                    false
                }
            }
        });

        for (app_id, command) in launches {
            tracing::info!(%app_id, "restoring session app");
            let _ = ctx.emit(Topic::LaunchApp(sola_bus::topics::LaunchAppPayload {
                app_id,
                command,
            }));
        }
        session::save(&self.session_entries);
    }

    fn on_client_disconnected(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::ClientDisconnected(app_id) = topic else { return };
        if app_id != "sola-session" {
            return;
        }
        tracing::warn!("sola-session disconnected; demoting live entries to pending");
        for e in self.session_entries.iter_mut() {
            e.window_id = None;
        }
        session::save(&self.session_entries);
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
                    "name": app.app_id,
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
        ctx.emit_sticky(Topic::RegisteredChords(chords));
    }

    fn shell_key_chords(&self) -> Vec<KeyChord> {
        let mut bindings: Vec<KeyChord> = self
            .menus
            .key_bindings()
            .into_iter()
            .filter(|b| b.meta)
            .collect();

        // Meta+Tab activates switcher.
        bindings.push(KeyCode::TAB.meta());

        // Meta+Space toggles launcher.
        bindings.push(KeyCode::SPACE.meta());

        // Meta+C / Meta+V: global copy/paste. Routed to the focused
        // window's owning Sola app via Topic::Copy / Topic::Paste.
        bindings.push(KeyCode::C.meta());
        bindings.push(KeyCode::V.meta());

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

    fn focus_window_from_pointer(&mut self, window_id: u32, ctx: &mut AppCtx) {
        let Some(info) = self
            .known_windows
            .iter()
            .find(|w| w.window_id == window_id)
        else {
            return;
        };
        if info.app_id == Self::APP_ID {
            return;
        }
        if self.menu_open || self.switcher.active || self.launcher.active {
            return;
        }
        let app_id = info.app_id.clone();
        self.set_focus(&app_id);
        self.focused_window_id = Some(window_id);
        self.mru_window_by_app.insert(app_id, window_id);
        ctx.emit(Topic::Focus(FocusTarget { window_id }));
        self.emit_composition(ctx);
    }

    pub fn set_focus(&mut self, app_id: &str) {
        self.focused_app_id = Some(app_id.to_string());
        self.zoning.set_focused(app_id.to_string());
        self.mru_apps.retain(|m| m != app_id);
        self.mru_apps.insert(0, app_id.to_string());

        // Close any open menu — the focus event to JS handles the menubar UI.
        if self.menu_open {
            self.menu_open = false;
            self.windows.menu.eval_js("clearMenu()");
        }

        let menu = self.menus.get_menu(app_id);
        let app_name = menu
            .and_then(|m| m.menus.first())
            .map(|d| d.label.as_str())
            .unwrap_or(app_id);
        let menu_labels: Vec<String> = menu
            .map(|m| m.menus.iter().map(|d| d.label.clone()).collect())
            .unwrap_or_default();

        self.windows.menubar.send_to_js(&serde_json::json!({
            "event": "focus",
            "app_name": app_name,
            "menu_labels": menu_labels,
        }));
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
            .map(|id| SwitcherApp {
                app_id: id.clone(),
            })
            .collect();
        // Append any known apps not yet in MRU.
        for id in &unique_app_ids {
            if !self.mru_apps.contains(id) {
                apps.push(SwitcherApp {
                    app_id: id.clone(),
                });
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
        let mut seen_app_ids = HashSet::new();
        for app_id in self.mru_apps.iter().rev() {
            if app_id == Self::APP_ID {
                continue;
            }
            seen_app_ids.insert(app_id.clone());
            // Include all windows for this app.
            for w in &self.known_windows {
                if w.app_id == *app_id {
                    entries.push(CompositionEntry {
                        window_id: w.window_id,
                    });
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

    /// Handle new/removed windows from sola-river's Apps list.
    pub fn handle_apps_update(&mut self, apps: Vec<App>, ctx: &mut AppCtx) {
        tracing::info!(count = apps.len(), "shell received Apps");
        let old_app_ids: HashSet<String> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| w.app_id.clone())
            .collect();
        let new_app_ids: HashSet<String> = apps
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
        for w in &apps {
            self.window_id_by_key
                .insert((w.app_id.clone(), w.title.clone()), w.window_id);
        }

        self.known_windows = apps;

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

        for id in &removed {
            self.mru_apps.retain(|m| m != id);
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
        if let Some(id) = added.first() {
            self.set_focus(id);
            // Focus the first window of this app.
            if let Some(wid) = self.lookup_any_window_id(id) {
                self.focused_window_id = Some(wid);
                self.mru_window_by_app.insert(id.clone(), wid);
                ctx.emit(Topic::Focus(FocusTarget { window_id: wid }));
            }
            self.emit_composition(ctx);
        }

        self.reconcile_session_entries(ctx);
    }

    pub fn open_menu(&mut self, source: &str, menu_index: usize, anchor_x: f64, ctx: &mut AppCtx) {
        let app_id = if source == "system" {
            Self::APP_ID.to_string()
        } else {
            self.focused_app_id.clone().unwrap_or_default()
        };

        let menu = self.menus.get_menu(&app_id);
        let menu_def = menu.and_then(|m| m.menus.get(menu_index));
        let Some(menu_def) = menu_def else { return };

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
        if let (Some((ow, oh)), Some(wid)) =
            (self.zoning.output_size, self.lookup_window_id(Self::APP_ID, "menu"))
        {
            ctx.emit(Topic::Frame(FrameUpdate {
                window_id: wid,
                x: 0,
                y: zoning::MENUBAR_HEIGHT,
                width: ow,
                height: oh - zoning::MENUBAR_HEIGHT,
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

        // Reload apps from disk so edits take effect without restarting.
        self.applications = ApplicationsConfig::load();

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

    /// Reconcile session entries against the current window set.
    ///
    /// - Demotes (does not remove) live entries whose window has vanished.
    /// - Claims pending entries for newly mapped windows and applies saved zones.
    /// - Creates new entries for windows with no matching pending entry.
    fn reconcile_session_entries(&mut self, ctx: &mut AppCtx) {
        use std::collections::HashSet;
        use sola_bus::topics::Zone;

        let current: HashSet<(String, u32)> = self
            .known_windows
            .iter()
            .map(|w| (w.app_id.clone(), w.window_id))
            .collect();

        // Demote (don't remove) live entries whose window has vanished.
        // Removal is driven by UserAppExited; see on_user_app_exited.
        for e in self.session_entries.iter_mut() {
            if let Some(wid) = e.window_id {
                if !current.contains(&(e.app_id.clone(), wid)) {
                    e.window_id = None;
                }
            }
        }

        // For each window, claim a pending entry or create a new one.
        let windows: Vec<(String, u32)> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| (w.app_id.clone(), w.window_id))
            .collect();

        let mut frames: Vec<sola_bus::topics::FrameUpdate> = Vec::new();

        for (app_id, window_id) in windows {
            let already = self
                .session_entries
                .iter()
                .any(|e| e.window_id == Some(window_id));
            if already {
                continue;
            }
            let pending_idx = self
                .session_entries
                .iter()
                .position(|e| e.app_id == app_id && e.window_id.is_none());
            match pending_idx {
                Some(i) => {
                    self.session_entries[i].window_id = Some(window_id);
                    let zone = self.session_entries[i].zone;
                    tracing::info!(%app_id, window_id, ?zone, "session: claimed pending entry");
                    if let Some(frame) = self.zoning.snap(window_id, zone) {
                        frames.push(frame);
                    }
                }
                None => {
                    // No pending entry — new window. Record it with whatever
                    // zone the existing zoning logic assigned, or a sane default.
                    let zone = self
                        .zoning
                        .current_zone_for_window(window_id)
                        .unwrap_or(Zone::Top);
                    self.session_entries.push(SessionEntry {
                        app_id: app_id.clone(),
                        zone,
                        window_id: Some(window_id),
                    });
                }
            }
        }

        for frame in frames {
            ctx.emit(Topic::Frame(frame));
        }

        session::save(&self.session_entries);
    }

    /// Update a session entry's zone when the user snaps a live window.
    pub fn update_entry_zone(&mut self, window_id: u32, zone: sola_bus::topics::Zone) {
        if let Some(e) = self
            .session_entries
            .iter_mut()
            .find(|e| e.window_id == Some(window_id))
        {
            if e.zone != zone {
                e.zone = zone;
                session::save(&self.session_entries);
            }
        }
    }

    /// Emit `CloseApp` for the currently focused window, unless a shell overlay
    /// is active or the focused surface is the shell itself.
    pub fn close_focused_app(&mut self, ctx: &mut AppCtx) {
        if self.launcher.active || self.switcher.active || self.menu_open {
            return;
        }
        let Some(wid) = self.focused_window_id else { return };
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

