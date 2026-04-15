mod menus;
mod switcher;
mod zoning;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_app::{asset_bundle, SolaApp};
use sola_bus::topics::{App, CompositionEntry, FocusTarget, FrameUpdate, Topic};

mod keycode {
    pub const TAB: u32 = 23;
    pub const LEFT: u32 = 113;
    pub const RIGHT: u32 = 114;
    pub const SUPER_L: u32 = 133;
}

struct ShellState {
    focused_app_id: Option<String>,
    mru_apps: Vec<String>,
    known_apps: Vec<App>,
    menus: menus::MenuCache,
    zoning: zoning::ZoningState,
    switcher: switcher::SwitcherState,
    switcher_webview: Option<webkit6::WebView>,
    menubar_webview: Option<webkit6::WebView>,
    menu_webview: Option<webkit6::WebView>,
    menu_open: bool,
    bus: Option<Rc<RefCell<sola_bus::BusClient>>>,
}

impl ShellState {
    fn new() -> Self {
        Self {
            focused_app_id: None,
            mru_apps: Vec::new(),
            known_apps: Vec::new(),
            menus: menus::MenuCache::new(),
            zoning: zoning::ZoningState::new(),
            switcher: switcher::SwitcherState::default(),
            switcher_webview: None,
            menubar_webview: None,
            menu_webview: None,
            menu_open: false,
            bus: None,
        }
    }

    fn set_focus(&mut self, app_id: &str) {
        self.focused_app_id = Some(app_id.to_string());
        self.zoning.set_focused(app_id.to_string());
        self.mru_apps.retain(|m| m != app_id);
        self.mru_apps.insert(0, app_id.to_string());

        let menu = self.menus.get_menu(app_id);
        let app_name = menu
            .and_then(|m| m.menus.first())
            .map(|d| d.label.as_str())
            .unwrap_or(app_id);
        let menu_labels: Vec<String> = menu
            .map(|m| m.menus.iter().map(|d| d.label.clone()).collect())
            .unwrap_or_default();

        if let Some(ref wv) = self.menubar_webview {
            let msg = serde_json::json!({
                "event": "focus",
                "app_name": app_name,
                "menu_labels": menu_labels,
            }).to_string();
            let js_str = serde_json::to_string(&msg).unwrap_or_default();
            eval_js(wv, &format!("window.__solaRecv({js_str})"));
        }
    }

    fn rebuild_switcher_apps(&self) -> Vec<App> {
        let mut apps: Vec<App> = self.mru_apps.iter()
            .filter_map(|id| self.known_apps.iter().find(|a| &a.app_id == id))
            .cloned()
            .collect();
        // Append any known apps not yet in MRU.
        for a in &self.known_apps {
            if a.app_id != "sola-shell" && !self.mru_apps.contains(&a.app_id) {
                apps.push(a.clone());
            }
        }
        apps
    }

    /// Build the composition list (bottom to top) and emit it.
    fn emit_composition(&self, emit: &dyn Fn(Topic)) {
        let mut entries = Vec::new();

        // 1. Shell menubar — always present at the bottom.
        entries.push(CompositionEntry {
            app_id: "sola-shell".into(),
            title: Some("menubar".into()),
        });

        // 2. App windows ordered by MRU (least recent first = bottom of stack).
        for app_id in self.mru_apps.iter().rev() {
            if app_id == "sola-shell" { continue; }
            entries.push(CompositionEntry {
                app_id: app_id.clone(),
                title: None,
            });
        }

        // Apps not yet in MRU (just appeared).
        for app in &self.known_apps {
            if app.app_id == "sola-shell" { continue; }
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
                app_id: "sola-shell".into(),
                title: Some("menu".into()),
            });
        }
        if self.switcher.active {
            entries.push(CompositionEntry {
                app_id: "sola-shell".into(),
                title: Some("switcher".into()),
            });
        }

        emit(Topic::Composition(entries));
    }

    /// Emit Frame updates for all known apps.
    fn emit_all_frames(&self, emit: &dyn Fn(Topic)) {
        if let Some(frame) = self.zoning.menubar_frame() {
            emit(Topic::Frame(frame));
        }

        for app in &self.known_apps {
            if app.app_id == "sola-shell" { continue; }
            if let Some(frame) = self.zoning.app_frame(&app.app_id) {
                emit(Topic::Frame(frame));
            }
        }
    }

    /// Handle new/removed apps from the compositor's Apps list.
    fn handle_apps_update(&mut self, apps: Vec<App>, emit: &dyn Fn(Topic)) {
        let old_ids: std::collections::HashSet<&str> =
            self.known_apps.iter().map(|a| a.app_id.as_str()).collect();
        let new_ids: std::collections::HashSet<&str> =
            apps.iter().map(|a| a.app_id.as_str()).collect();

        let added: Vec<String> = apps.iter()
            .filter(|a| !old_ids.contains(a.app_id.as_str()) && a.app_id != "sola-shell")
            .map(|a| a.app_id.clone())
            .collect();

        let removed: Vec<String> = self.known_apps.iter()
            .filter(|a| !new_ids.contains(a.app_id.as_str()) && a.app_id != "sola-shell")
            .map(|a| a.app_id.clone())
            .collect();

        self.known_apps = apps.clone();
        self.switcher.apps = apps.into_iter()
            .filter(|a| a.app_id != "sola-shell")
            .collect();

        for id in &removed {
            self.mru_apps.retain(|m| m != id);
        }

        // Emit Frames for new apps.
        for id in &added {
            if let Some(frame) = self.zoning.app_frame(id) {
                emit(Topic::Frame(frame));
            }
        }

        self.emit_composition(emit);

        // Focus the newest app.
        if let Some(id) = added.first() {
            self.set_focus(id);
            emit(Topic::Focus(FocusTarget {
                app_id: id.clone(),
                title: None,
            }));
            self.emit_composition(emit);
        }
    }
}

static MENUBAR_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/menubar.ts" => (include_str!("../web/src/menubar.ts"), TypeScript),
};

static OVERLAY_HTML: &str = include_str!("../web/overlay.html");
static OVERLAY_JS: &str = include_str!("../web/src/overlay.ts");
static MENU_HTML: &str = include_str!("../web/menu.html");

fn main() {
    let state: Rc<RefCell<ShellState>> = Rc::new(RefCell::new(ShellState::new()));

    SolaApp::builder()
        .app_id("sola-shell")
        .window_size(1920, zoning::MENUBAR_HEIGHT)
        .decorated(false)
        .transparent(true)
        .web_assets(MENUBAR_ASSETS)
        .on_js_command({
            let state = state.clone();
            move |cmd, args| {
                match cmd {
                    "open_menu" => {
                        let index = args.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let mut s = state.borrow_mut();
                        open_menu(&mut s, index);
                    }
                    "close_menu" => {
                        let mut s = state.borrow_mut();
                        if let Some(ref bus) = s.bus {
                            let bus = bus.clone();
                            let emit = move |topic: Topic| { let _ = bus.borrow_mut().emit(topic); };
                            close_menu(&mut s, &emit);
                        }
                    }
                    _ => {}
                }
            }
        })
        .on_bus_event({
            let state = state.clone();
            move |topic, send_to_js, emit| {
                let mut s = state.borrow_mut();
                match topic {
                    Topic::Apps(apps) => {
                        s.handle_apps_update(apps.clone(), emit);

                        if s.switcher.active {
                            let json = serde_json::to_string(&s.switcher.apps).unwrap_or_default();
                            let script = format!(
                                "renderSwitcher({}, {})",
                                json, s.switcher.selected
                            );
                            if let Some(ref wv) = s.switcher_webview {
                                eval_js(wv, &script);
                            }
                        }
                    }
                    Topic::SetAppMenu(payload) => {
                        s.menus.set_menu(payload.clone());

                        if s.focused_app_id.as_deref() == Some(&payload.app_id) {
                            let app_name = payload
                                .menus
                                .first()
                                .map(|d| d.label.as_str())
                                .unwrap_or(&payload.app_id);
                            let menu_labels: Vec<String> =
                                payload.menus.iter().map(|d| d.label.clone()).collect();
                            send_to_js(serde_json::json!({
                                "event": "focus",
                                "app_name": app_name,
                                "menu_labels": menu_labels,
                            }));
                        }
                    }
                    Topic::OutputGeometry(geo) => {
                        s.zoning.set_output_size(geo);
                        s.emit_all_frames(emit);
                        s.emit_composition(emit);
                    }
                    _ => {}
                }
            }
        })
        .on_activate({
            let state = state.clone();
            move |window, _webview, bus| {
                use sola_bus::topics::{
                    AppMenuPayload, MenuDefinition, MenuItem,
                    WindowPolicy, WindowPolicyPayload,
                };

                let _ = bus.borrow_mut().emit_sticky(Topic::SetWindowPolicy(
                    WindowPolicyPayload {
                        app_id: "sola-shell".into(),
                        windows: vec![
                            WindowPolicy {
                                title: "menubar".into(),
                                zoned: false,
                                keyboard_target: true,
                                size: Some((1920, zoning::MENUBAR_HEIGHT)),
                                position: Some((0, 0)),
                            },
                            WindowPolicy {
                                title: "switcher".into(),
                                zoned: false,
                                keyboard_target: false,
                                size: Some((800, 400)),
                                position: Some((560, 340)),
                            },
                            WindowPolicy {
                                title: "menu".into(),
                                zoned: false,
                                keyboard_target: false,
                                size: Some((220, 300)),
                                position: Some((0, zoning::MENUBAR_HEIGHT)),
                            },
                        ],
                    },
                ));

                // Register the shell's system menu (for shortcut lookup).
                state.borrow_mut().menus.set_menu(AppMenuPayload {
                    app_id: "sola-shell".into(),
                    menus: vec![MenuDefinition {
                        label: "Sola".into(),
                        items: vec![MenuItem::Action {
                            id: "exit".into(),
                            label: "Exit Sola".into(),
                            shortcut: Some("Super+Shift+Backspace".into()),
                            disabled: false,
                            checked: false,
                        }],
                    }],
                });

                window.set_title(Some("menubar"));

                let webview_ref = {
                    let child = window.child().unwrap();
                    child.downcast::<webkit6::WebView>().unwrap()
                };
                state.borrow_mut().menubar_webview = Some(webview_ref);

                state.borrow_mut().bus = Some(bus.clone());

                setup_switcher_panel(window, &state, &bus);
                setup_menu_panel(window, &state, &bus);
                setup_key_controller(window, &state, &bus);

                tracing::info!("shell ready");
            }
        })
        .run();
}

fn setup_switcher_panel(
    window: &gtk4::ApplicationWindow,
    state: &Rc<RefCell<ShellState>>,
    _bus: &Rc<RefCell<sola_bus::BusClient>>,
) {
    let app = window.application().unwrap();

    let switcher_window = gtk4::ApplicationWindow::new(&app);
    switcher_window.set_decorated(false);
    switcher_window.set_default_size(800, 400);
    switcher_window.set_title(Some("switcher"));

    let css = gtk4::CssProvider::new();
    css.load_from_data("window.switcher-window, window.switcher-window.background { background: transparent; }");
    gtk4::style_context_add_provider_for_display(
        &gdk4::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    switcher_window.add_css_class("switcher-window");

    let switcher_webview = webkit6::WebView::new();
    switcher_webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&switcher_webview) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_write_console_messages_to_stdout(true);
    }
    switcher_webview.connect_context_menu(|_, _, _| true);

    let html = OVERLAY_HTML.replace("__OVERLAY_JS__", &strip_ts_inline(OVERLAY_JS));
    switcher_webview.load_html(&html, None);

    switcher_window.set_child(Some(&switcher_webview));
    switcher_window.present();

    state.borrow_mut().switcher_webview = Some(switcher_webview);
}

fn setup_menu_panel(
    window: &gtk4::ApplicationWindow,
    state: &Rc<RefCell<ShellState>>,
    bus: &Rc<RefCell<sola_bus::BusClient>>,
) {
    let app = window.application().unwrap();

    let menu_window = gtk4::ApplicationWindow::new(&app);
    menu_window.set_decorated(false);
    menu_window.set_default_size(220, 300);
    menu_window.set_title(Some("menu"));

    let css = gtk4::CssProvider::new();
    css.load_from_data("window.menu-window, window.menu-window.background { background: transparent; }");
    gtk4::style_context_add_provider_for_display(
        &gdk4::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    menu_window.add_css_class("menu-window");

    let menu_webview = webkit6::WebView::new();
    menu_webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&menu_webview) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_write_console_messages_to_stdout(true);
    }
    menu_webview.connect_context_menu(|_, _, _| true);
    menu_webview.load_html(MENU_HTML, None);

    // Menu item clicks come back via title change: "action:app_id:action_id"
    menu_webview.connect_notify_local(Some("title"), {
        let state = state.clone();
        let bus = bus.clone();
        move |webview, _| {
            let Some(title) = webview.title() else { return };
            let title = title.to_string();
            if let Some(rest) = title.strip_prefix("action:") {
                if let Some((app_id, action_id)) = rest.split_once(':') {
                    tracing::info!(app_id, action_id, "menu action");

                    if app_id == "sola-shell" && action_id == "exit" {
                        let _ = bus.borrow_mut().emit(Topic::Shutdown);
                    } else {
                        let _ = bus.borrow_mut().emit(Topic::MenuAction(
                            sola_bus::topics::MenuActionPayload {
                                app_id: app_id.to_string(),
                                action_id: action_id.to_string(),
                            },
                        ));
                    }

                    let mut s = state.borrow_mut();
                    close_menu(&mut s, &|topic: Topic| {
                        let _ = bus.borrow_mut().emit(topic);
                    });
                }
            }
        }
    });

    menu_window.set_child(Some(&menu_webview));
    menu_window.present();

    state.borrow_mut().menu_webview = Some(menu_webview);
}

fn open_menu(s: &mut ShellState, menu_index: usize) {
    use sola_bus::topics::MenuItem;

    let app_id = if menu_index == 0 {
        "sola-shell".to_string()
    } else {
        s.focused_app_id.clone().unwrap_or_default()
    };

    let menu = s.menus.get_menu(&app_id);
    let menu_def = menu.and_then(|m| m.menus.get(menu_index));
    let Some(menu_def) = menu_def else { return };

    let items: Vec<serde_json::Value> = menu_def.items.iter().map(|item| {
        match item {
            MenuItem::Action { id, label, shortcut, disabled, .. } => {
                serde_json::json!({
                    "type": "action",
                    "id": id,
                    "app_id": app_id,
                    "label": label,
                    "shortcut": shortcut,
                    "disabled": disabled,
                })
            }
            MenuItem::Divider => serde_json::json!({ "type": "divider" }),
        }
    }).collect();

    if let Some(ref wv) = s.menu_webview {
        let json = serde_json::to_string(&items).unwrap_or_default();
        eval_js(wv, &format!("showMenu({})", json));
    }

    // Position the menu panel below the menubar, roughly aligned to the clicked label.
    let anchor_x = menu_index as i32 * 80;
    if let Some(ref bus) = s.bus {
        let _ = bus.borrow_mut().emit(Topic::Frame(FrameUpdate {
            app_id: "sola-shell".into(),
            title: Some("menu".into()),
            x: anchor_x,
            y: zoning::MENUBAR_HEIGHT,
            width: 220,
            height: 300,
        }));
    }

    s.menu_open = true;
    if let Some(ref bus) = s.bus {
        let emit = |topic: Topic| { let _ = bus.borrow_mut().emit(topic); };
        s.emit_composition(&emit);
    }
}

fn close_menu(s: &mut ShellState, emit: &dyn Fn(Topic)) {
    if !s.menu_open { return; }
    s.menu_open = false;
    if let Some(ref wv) = s.menu_webview {
        eval_js(wv, "clearMenu()");
    }
    if let Some(ref wv) = s.menubar_webview {
        let msg = serde_json::json!({"event": "close_menu"}).to_string();
        let js_str = serde_json::to_string(&msg).unwrap_or_default();
        eval_js(wv, &format!("window.__solaRecv({js_str})"));
    }
    s.emit_composition(emit);
}

fn setup_key_controller(
    window: &gtk4::ApplicationWindow,
    state: &Rc<RefCell<ShellState>>,
    bus: &Rc<RefCell<sola_bus::BusClient>>,
) {
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

    key_ctrl.connect_key_pressed({
        let state = state.clone();
        let bus = bus.clone();
        move |_, _keyval, keycode, gtk_modifiers| {
            let mut s = state.borrow_mut();
            let shift_held = gtk_modifiers.contains(gdk4::ModifierType::SHIFT_MASK);
            let emit = |topic: Topic| {
                let _ = bus.borrow_mut().emit(topic);
            };

            // Shell system shortcuts (highest priority).
            if let Some(action) = s.menus.lookup_shortcut(keycode, shift_held, "sola-shell") {
                tracing::info!(action_id = %action.action_id, "shell shortcut");
                if action.action_id == "exit" {
                    emit(Topic::Shutdown);
                }
                return glib::Propagation::Stop;
            }

            // Super+Tab: activate switcher.
            if keycode == keycode::TAB && !s.switcher.active {
                tracing::info!("activating switcher");
                s.switcher.apps = s.rebuild_switcher_apps();
                s.switcher.active = true;
                s.switcher.selected = if s.switcher.apps.len() > 1 { 1 } else { 0 };
                let json = serde_json::to_string(&s.switcher.apps).unwrap_or_default();
                let script = format!("renderSwitcher({}, {})", json, s.switcher.selected);
                if let Some(ref wv) = s.switcher_webview {
                    eval_js(wv, &script);
                }

                // Emit switcher frame (centered on screen).
                if let Some((ow, oh)) = s.zoning.output_size {
                    emit(Topic::Frame(FrameUpdate {
                        app_id: "sola-shell".into(),
                        title: Some("switcher".into()),
                        x: (ow - 800) / 2,
                        y: (oh - 400) / 2,
                        width: 800,
                        height: 400,
                    }));
                }
                s.emit_composition(&emit);
                return glib::Propagation::Stop;
            }

            // Switcher navigation.
            if s.switcher.active {
                match keycode {
                    keycode::TAB | keycode::RIGHT => {
                        s.switcher.select_next();
                        let sel = s.switcher.selected;
                        if let Some(ref wv) = s.switcher_webview {
                            eval_js(wv, &format!("setSelection({sel})"));
                        }
                        return glib::Propagation::Stop;
                    }
                    keycode::LEFT => {
                        s.switcher.select_prev();
                        let sel = s.switcher.selected;
                        if let Some(ref wv) = s.switcher_webview {
                            eval_js(wv, &format!("setSelection({sel})"));
                        }
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }

            // Zone snapping (Super+Numpad).
            if let Some(frame) = s.zoning.handle_key(keycode) {
                emit(Topic::Frame(frame));
                return glib::Propagation::Stop;
            }

            // Focused app menu shortcut lookup.
            if let Some(focused) = s.focused_app_id.clone() {
                if let Some(action) = s.menus.lookup_shortcut(keycode, shift_held, &focused) {
                    tracing::info!(
                        app_id = %action.app_id,
                        action_id = %action.action_id,
                        "menu shortcut matched"
                    );
                    emit(Topic::MenuAction(action));
                    return glib::Propagation::Stop;
                }
            }

            glib::Propagation::Proceed
        }
    });

    key_ctrl.connect_key_released({
        let state = state.clone();
        let bus = bus.clone();
        move |_, _keyval, keycode, _modifiers| {
            if keycode != keycode::SUPER_L {
                return;
            }

            let emit = |topic: Topic| {
                let _ = bus.borrow_mut().emit(topic);
            };

            let mut s = state.borrow_mut();
            if !s.switcher.active {
                return;
            }

            let app_id = s.switcher.selected_app_id().map(String::from);
            tracing::info!(app_id = ?app_id, "deactivating switcher");

            s.switcher.active = false;
            if let Some(ref wv) = s.switcher_webview {
                eval_js(wv, "clear()");
            }

            if let Some(ref app_id) = app_id {
                s.set_focus(app_id);
                emit(Topic::Focus(FocusTarget {
                    app_id: app_id.clone(),
                    title: None,
                }));
            }
            s.emit_composition(&emit);
        }
    });

    window.add_controller(key_ctrl);
}

fn eval_js(webview: &webkit6::WebView, script: &str) {
    webview.evaluate_javascript(script, None, None, None::<&gio::Cancellable>, |_| {});
}

fn strip_ts_inline(ts: &str) -> String {
    ts.to_string()
}
