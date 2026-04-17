use std::collections::{HashMap, HashSet};

use serde_json::Value;
use sola_app::config::JsonConfigIn;
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle};
use sola_bus::topics::{
    App, AppMenuPayload, CompositionEntry, FocusTarget, FrameUpdate, KeyChord,
    MenuDefinition, MenuItem, MouseClickedPayload, MouseEnteredPayload,
    RegisteredChord, Topic, XkbProfilePayload,
};
use sola_core::KeyCode;

use crate::applications::{Application, ApplicationsConfig};
use crate::launcher::{self, LAUNCHER_ASSETS, LauncherState};
use crate::menu::{MENU_ASSETS, MenuCache};
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
    pub known_windows: Vec<App>,
    /// Maps (app_id, title) to window_id for lookup.
    pub window_id_by_key: HashMap<(String, String), u32>,
    pub applications: ApplicationsConfig,
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

    fn on_bus_event(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        match topic {
            Topic::Apps(apps) => {
                self.handle_apps_update(apps.clone(), ctx);
                if self.switcher.active {
                    let json = self.switcher_apps_json();
                    self.windows.switcher.eval_js(&format!(
                        "renderSwitcher({}, {})",
                        json, self.switcher.selected
                    ));
                }
            }
            Topic::SetAppMenu(payload) => {
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
            Topic::OutputGeometry(geo) => {
                self.zoning.set_output_size(geo);
                self.emit_all_frames(ctx);
                self.emit_composition(ctx);
            }
            Topic::MouseEntered(MouseEnteredPayload { window_id }) => {
                self.focus_window_from_pointer(*window_id, ctx);
            }
            Topic::MouseClicked(MouseClickedPayload { window_id }) => {
                self.focus_window_from_pointer(*window_id, ctx);
            }
            Topic::MouseLeft => {}
            Topic::Chord(evt) => {
                crate::keys::handle_chord(self, ctx, evt.clone());
            }
            Topic::ChordReleased(evt) => {
                crate::keys::handle_chord_released(self, ctx, evt.clone());
            }
            _ => {}
        }
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
            ("launcher", "launch") => {
                let app_id = args.get("app_id").and_then(|v| v.as_str()).unwrap_or("");
                self.launch_and_close(app_id, ctx);
            }
            ("launcher", "close") => {
                self.close_launcher(ctx);
            }
            _ => {}
        }
    }
}

impl ShellApp {
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
        self.emit_xkb_profile_for_focus(ctx);
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

    /// Emit `Topic::XkbProfile` reflecting the current focused app:
    /// `"meta-as-ctrl"` when focus is on a non-Sola client (so Meta+letter
    /// arrives as Ctrl+letter), `"default"` otherwise. Sola apps are
    /// identified by the `sola-` app_id prefix. sola-river dedupes
    /// redundant switches, so calling this on every focus update is fine.
    pub fn emit_xkb_profile_for_focus(&self, ctx: &mut AppCtx) {
        let profile = match self.focused_app_id.as_deref() {
            Some(id) if id.starts_with("sola-") => "default",
            Some(_) => "meta-as-ctrl",
            None => "default",
        };
        ctx.emit_sticky(Topic::XkbProfile(XkbProfilePayload {
            profile: profile.to_string(),
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

        let mut focus_lost = false;
        for id in &removed {
            self.mru_apps.retain(|m| m != id);
            if self.focused_app_id.as_deref() == Some(id.as_str()) {
                self.focused_app_id = None;
                self.focused_window_id = None;
                focus_lost = true;
            }
        }
        if focus_lost {
            self.emit_xkb_profile_for_focus(ctx);
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
            self.emit_xkb_profile_for_focus(ctx);
            self.emit_composition(ctx);
        }
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
        if self.launcher.active {
            return;
        }
        tracing::info!("activating launcher");

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
        if !self.launcher.active {
            return;
        }
        tracing::info!("deactivating launcher");
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
        if let Some(app) = self.applications.get(app_id) {
            tracing::info!(app_id, command = %app.command, "launching application");
            ctx.emit(Topic::LaunchApp(app.command.clone()));
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
}

