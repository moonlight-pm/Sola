use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde_json::Value;
use sola_app::config::JsonConfigIn;
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle};
use sola_bus::topics::{
    App, AppMenuPayload, CompositionEntry, FocusTarget, FrameUpdate, KeyChord, MenuDefinition,
    MenuItem, MouseEnteredPayload, ShellKeyBindingsPayload, Topic,
};
use sola_core::KeyCode;

use crate::applications::{Application, ApplicationsConfig};
use crate::launcher::{self, LAUNCHER_ASSETS, LauncherState};
use crate::menu::{MENU_ASSETS, MenuCache};
use crate::menubar::setup_menubar;
use crate::switcher::{SWITCHER_ASSETS, SwitcherState};
use crate::zoning::{self, ZoningState};

/// How long after an app disappears do we treat its re-appearance as a
/// "re-map" (e.g. sola-x reconnect after an EGL buffer failure) rather
/// than a fresh launch. Inside the window: keep MRU position, don't
/// steal focus. Outside the window: treat as a brand-new launch.
const REMAP_WINDOW: Duration = Duration::from_secs(5);

pub struct ShellWindows {
    pub menubar: WindowHandle,
    pub menu: WindowHandle,
    pub switcher: WindowHandle,
    pub launcher: WindowHandle,
}

pub struct ShellApp {
    pub focused_app_id: Option<String>,
    pub mru_apps: Vec<String>,
    pub known_apps: Vec<App>,
    pub applications: ApplicationsConfig,
    pub menus: MenuCache,
    pub zoning: ZoningState,
    pub switcher: SwitcherState,
    pub launcher: LauncherState,
    pub menu_open: bool,
    pub windows: ShellWindows,
    /// Timestamps of recent removals, used to distinguish a genuine
    /// re-launch from an app that just briefly vanished and came back.
    pub recently_removed: HashMap<String, Instant>,
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
            mru_apps: Vec::new(),
            known_apps: Vec::new(),
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
            recently_removed: HashMap::new(),
        };

        app.emit_shell_key_bindings(ctx);

        app
    }

    fn after_runtime_ready(
        &mut self,
        runtime: std::rc::Weak<std::cell::RefCell<sola_app::AppRuntime<Self>>>,
        _ctx: &mut AppCtx,
    ) {
        crate::keys::install(self.windows.menubar.clone(), runtime);
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

                self.emit_shell_key_bindings(ctx);

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
            Topic::MouseEntered(MouseEnteredPayload { app_id, title }) => {
                // Shell-owned surfaces should not steal app focus.
                if app_id == Self::APP_ID {
                    return;
                }

                // Keep menu/switcher interactions stable while overlays are active.
                if self.menu_open || self.switcher.active || self.launcher.active {
                    return;
                }

                self.set_focus(app_id);
                ctx.emit(Topic::Focus(FocusTarget {
                    app_id: app_id.clone(),
                    title: title.clone(),
                }));
                self.emit_composition(ctx);
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
                    .unwrap_or_else(|| app.icon.clone());
                serde_json::json!({
                    "app_id": app.app_id,
                    "name": app.name,
                    "icon": icon,
                    "window_count": app.window_count,
                })
            })
            .collect();
        serde_json::to_string(&entries).unwrap_or_default()
    }

    fn emit_shell_key_bindings(&self, ctx: &mut AppCtx) {
        ctx.emit_sticky(Topic::ShellKeyBindings(ShellKeyBindingsPayload {
            app_id: Self::APP_ID.into(),
            bindings: self.shell_key_bindings(),
        }));
    }

    fn shell_key_bindings(&self) -> Vec<KeyChord> {
        let mut bindings: Vec<KeyChord> = self
            .menus
            .key_bindings()
            .into_iter()
            .filter(|b| b.meta)
            .collect();

        // Ensure compositor always routes Meta+Tab for switcher activation.
        bindings.push(KeyCode::TAB.meta());

        // Ensure compositor always routes Meta+Space for launcher activation.
        bindings.push(KeyCode::SPACE.meta());

        // Ensure compositor routes Meta+Numpad zoning keys.
        for &keycode in zoning::ZONING_KEYCODES {
            bindings.push(KeyChord {
                keycode: keycode.into(),
                ..KeyCode::TAB.meta()
            });
        }

        // Keep list de-duplicated and stable.
        bindings.sort_by_key(|b| (b.keycode, b.meta, b.alt, b.ctrl, b.shift));
        bindings.dedup();

        bindings
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

    pub fn rebuild_switcher_apps(&self) -> Vec<App> {
        let mut apps: Vec<App> = self
            .mru_apps
            .iter()
            .filter_map(|id| self.known_apps.iter().find(|a| &a.app_id == id))
            .cloned()
            .collect();
        // Append any known apps not yet in MRU.
        for a in &self.known_apps {
            if a.app_id != Self::APP_ID && !self.mru_apps.contains(&a.app_id) {
                apps.push(a.clone());
            }
        }
        apps
    }

    /// Build the composition list (bottom to top) and emit it.
    pub fn emit_composition(&self, ctx: &mut AppCtx) {
        let mut entries = Vec::new();

        // 1. Shell menubar — always at the bottom.
        entries.push(CompositionEntry {
            app_id: Self::APP_ID.into(),
            title: Some("menubar".into()),
        });

        // 2. App windows ordered by MRU (least recent first = bottom of stack).
        for app_id in self.mru_apps.iter().rev() {
            if app_id == Self::APP_ID {
                continue;
            }
            entries.push(CompositionEntry {
                app_id: app_id.clone(),
                title: None,
            });
        }

        // Apps not yet in MRU.
        for app in &self.known_apps {
            if app.app_id == Self::APP_ID {
                continue;
            }
            if !self.mru_apps.contains(&app.app_id) {
                entries.push(CompositionEntry {
                    app_id: app.app_id.clone(),
                    title: None,
                });
            }
        }

        // 3. Shell panels on top when active.
        if self.menu_open {
            entries.push(CompositionEntry {
                app_id: Self::APP_ID.into(),
                title: Some("menu".into()),
            });
        }
        if self.switcher.active {
            entries.push(CompositionEntry {
                app_id: Self::APP_ID.into(),
                title: Some("switcher".into()),
            });
        }
        if self.launcher.active {
            entries.push(CompositionEntry {
                app_id: Self::APP_ID.into(),
                title: Some("launcher".into()),
            });
        }

        ctx.emit(Topic::Composition(entries));
    }

    /// Emit Frame updates for all known apps.
    pub fn emit_all_frames(&self, ctx: &mut AppCtx) {
        if let Some(frame) = self.zoning.menubar_frame() {
            ctx.emit(Topic::Frame(frame));
        }
        for app in &self.known_apps {
            if app.app_id == Self::APP_ID {
                continue;
            }
            if let Some(frame) = self.zoning.app_frame(&app.app_id) {
                ctx.emit(Topic::Frame(frame));
            }
        }
    }

    /// Handle new/removed apps from the compositor's Apps list.
    pub fn handle_apps_update(&mut self, apps: Vec<App>, ctx: &mut AppCtx) {
        let old_ids: HashSet<&str> = self.known_apps.iter().map(|a| a.app_id.as_str()).collect();
        let new_ids: HashSet<&str> = apps.iter().map(|a| a.app_id.as_str()).collect();

        let added: Vec<String> = apps
            .iter()
            .filter(|a| !old_ids.contains(a.app_id.as_str()) && a.app_id != Self::APP_ID)
            .map(|a| a.app_id.clone())
            .collect();

        let removed: Vec<String> = self
            .known_apps
            .iter()
            .filter(|a| !new_ids.contains(a.app_id.as_str()) && a.app_id != Self::APP_ID)
            .map(|a| a.app_id.clone())
            .collect();

        self.known_apps = apps.clone();
        self.switcher.apps = apps
            .into_iter()
            .filter(|a| a.app_id != Self::APP_ID)
            .collect();

        // Preserve MRU entries for removed apps so a brief disappearance
        // (sola-x reconnect, etc) can restore them in place. Stamp the
        // removal time so handle_apps_update below can distinguish a
        // quick re-map from a true re-launch.
        let now = Instant::now();
        for id in &removed {
            self.recently_removed.insert(id.clone(), now);
            if self.focused_app_id.as_deref() == Some(id.as_str()) {
                self.focused_app_id = None;
            }
        }
        self.recently_removed
            .retain(|_, ts| now.duration_since(*ts) < Duration::from_secs(60));

        // Classify each added app as a re-map or a fresh launch.
        let mut truly_new: Vec<String> = Vec::new();
        for id in &added {
            let is_remap = self
                .recently_removed
                .get(id)
                .is_some_and(|ts| now.duration_since(*ts) < REMAP_WINDOW);
            self.recently_removed.remove(id);

            if is_remap {
                // Keep existing MRU position; if we've somehow lost it,
                // drop the app at the back so it doesn't visually raise.
                if !self.mru_apps.iter().any(|m| m == id) {
                    self.mru_apps.push(id.clone());
                }
            } else {
                // Fresh launch — will land at the front of MRU via
                // set_focus below, and should receive keyboard focus.
                truly_new.push(id.clone());
            }
        }

        // Emit Frames for all added apps (new and re-mapped alike).
        for id in &added {
            if let Some(frame) = self.zoning.app_frame(id) {
                ctx.emit(Topic::Frame(frame));
            }
        }

        self.emit_composition(ctx);

        // Focus the first fresh launch so the user can start using it
        // immediately. set_focus inserts it at the front of MRU.
        if let Some(id) = truly_new.first() {
            self.set_focus(id);
            ctx.emit(Topic::Focus(FocusTarget {
                app_id: id.clone(),
                title: None,
            }));
            self.emit_composition(ctx);
        }
        // Any additional fresh launches in the same batch go near the
        // front of MRU but behind the focused one.
        for id in truly_new.iter().skip(1) {
            if !self.mru_apps.iter().any(|m| m == id) {
                self.mru_apps.insert(1.min(self.mru_apps.len()), id.clone());
            }
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
        if let Some((ow, oh)) = self.zoning.output_size {
            ctx.emit(Topic::Frame(FrameUpdate {
                app_id: Self::APP_ID.into(),
                title: Some("menu".into()),
                x: 0,
                y: zoning::MENUBAR_HEIGHT,
                width: ow,
                height: oh - zoning::MENUBAR_HEIGHT,
            }));
        }

        self.menu_open = true;
        self.emit_composition(ctx);
    }

    pub fn close_menu(&mut self, ctx: &mut AppCtx) {
        if !self.menu_open {
            return;
        }
        self.menu_open = false;
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

        // Snapshot the focus target we'll restore on close.
        self.launcher.prior_focus = self.focused_app_id.as_ref().map(|id| FocusTarget {
            app_id: id.clone(),
            title: None,
        });

        self.launcher.active = true;
        self.launcher.apply_query(&self.applications, "");

        if let Some((ow, oh)) = self.zoning.output_size {
            ctx.emit(Topic::Frame(FrameUpdate {
                app_id: Self::APP_ID.into(),
                title: Some("launcher".into()),
                x: (ow - launcher::WIDTH) / 2,
                y: (oh - launcher::HEIGHT) / 3,
                width: launcher::WIDTH,
                height: launcher::HEIGHT,
            }));
        }

        self.emit_composition(ctx);

        // Route keyboard to the launcher window.
        ctx.emit(Topic::Focus(FocusTarget {
            app_id: Self::APP_ID.into(),
            title: Some("launcher".into()),
        }));

        self.windows.launcher.eval_js("resetForOpen()");
        self.render_launcher();
    }

    pub fn close_launcher(&mut self, ctx: &mut AppCtx) {
        if !self.launcher.active {
            return;
        }
        tracing::info!("deactivating launcher");
        let prior = self.launcher.prior_focus.take();
        self.launcher.active = false;
        self.launcher.query.clear();
        self.launcher.filtered_ids.clear();
        self.launcher.selected = 0;

        self.emit_composition(ctx);

        if let Some(target) = prior {
            ctx.emit(Topic::Focus(target));
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

