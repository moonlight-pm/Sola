mod menus;
mod switcher;
mod zoning;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_app::{asset_bundle, SolaApp};
use sola_bus::topics::Topic;

mod keycode {
    pub const TAB: u32 = 23;
    pub const LEFT: u32 = 113;
    pub const RIGHT: u32 = 114;
    pub const SUPER_L: u32 = 133;
}

struct ShellState {
    focused_app_id: Option<String>,
    menus: menus::MenuCache,
    zoning: zoning::ZoningState,
    switcher: switcher::SwitcherState,
    overlay_webview: Option<webkit6::WebView>,
    overlay_window: Option<gtk4::ApplicationWindow>,
}

impl ShellState {
    fn new() -> Self {
        Self {
            focused_app_id: None,
            menus: menus::MenuCache::new(),
            zoning: zoning::ZoningState::new(),
            switcher: switcher::SwitcherState::default(),
            overlay_webview: None,
            overlay_window: None,
        }
    }
}

static MENUBAR_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/menubar.ts" => (include_str!("../web/src/menubar.ts"), TypeScript),
};

static OVERLAY_HTML: &str = include_str!("../web/overlay.html");
static OVERLAY_JS: &str = include_str!("../web/src/overlay.ts");

fn main() {
    let state: Rc<RefCell<ShellState>> = Rc::new(RefCell::new(ShellState::new()));

    SolaApp::builder()
        .app_id("sola-shell")
        .window_size(1920, zoning::MENUBAR_HEIGHT)
        .decorated(false)
        .transparent(true)
        .web_assets(MENUBAR_ASSETS)
        .on_bus_event({
            let state = state.clone();
            move |topic, send_to_js, emit| {
                let mut s = state.borrow_mut();
                match topic {
                    Topic::Apps(apps) => {
                        s.switcher.apps = apps.clone();

                        if s.switcher.active {
                            tracing::info!(count = apps.len(), "updated app list (switcher active)");
                            let json = serde_json::to_string(&s.switcher.apps).unwrap_or_default();
                            let script = format!(
                                "renderSwitcher({}, {})",
                                json, s.switcher.selected
                            );
                            if let Some(ref ov) = s.overlay_webview {
                                push_overlay_js(ov, &script);
                            }
                        }
                    }
                    Topic::FocusChanged(app_id) => {
                        tracing::info!(app_id = %app_id, "focus changed");
                        s.focused_app_id = Some(app_id.clone());
                        s.zoning.set_focused(app_id.clone());

                        let menu = s.menus.get_menu(app_id);
                        let app_name = menu
                            .and_then(|m| m.menus.first())
                            .map(|d| d.label.as_str())
                            .unwrap_or(app_id);
                        let menu_labels: Vec<String> = menu
                            .map(|m| m.menus.iter().map(|d| d.label.clone()).collect())
                            .unwrap_or_default();

                        send_to_js(serde_json::json!({
                            "event": "focus",
                            "app_name": app_name,
                            "menu_labels": menu_labels,
                        }));
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

                        if let Some(g) = s.zoning.menubar_geometry() {
                            emit(Topic::SetWindowGeometry(g));
                        }
                        if let Some(g) = s.zoning.overlay_geometry() {
                            emit(Topic::SetWindowGeometry(g));
                        }

                        for geo in s.zoning.restore() {
                            emit(Topic::SetWindowGeometry(geo));
                        }
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
                                auto_focus: false,
                                keyboard_target: true,
                                size: Some((1920, zoning::MENUBAR_HEIGHT)),
                                position: Some((0, 0)),
                            },
                            WindowPolicy {
                                title: "overlay".into(),
                                zoned: false,
                                auto_focus: false,
                                keyboard_target: false,
                                size: None,
                                position: None,
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

                // Menubar command bridge via title
                let webview_ref = {
                    let child = window.child().unwrap();
                    child.downcast::<webkit6::WebView>().unwrap()
                };
                webview_ref.connect_notify_local(Some("title"), {
                    let bus = bus.clone();
                    let state = state.clone();
                    move |webview, _| {
                        let Some(title) = webview.title() else { return };
                        let title = title.to_string();
                        match title.as_str() {
                            "cmd:exit" => {
                                tracing::info!("exit requested from system menu");
                                let _ = bus.borrow_mut().emit(Topic::Shutdown);
                            }
                            "cmd:system_menu" => {
                                let items = serde_json::json!([
                                    {"type": "action", "id": "exit", "label": "Exit Sola"}
                                ]);
                                let script = format!("showDropdown({}, 0)", items);
                                if let Some(ref ov) = state.borrow().overlay_webview {
                                    push_overlay_js(ov, &script);
                                }
                            }
                            _ => {}
                        }
                    }
                });

                setup_overlay(window, &state, &bus);
                tracing::info!("shell ready");
            }
        })
        .run();
}

fn setup_overlay(
    window: &gtk4::ApplicationWindow,
    state: &Rc<RefCell<ShellState>>,
    bus: &Rc<RefCell<sola_bus::BusClient>>,
) {
    let app = window.application().unwrap();

    let overlay_window = gtk4::ApplicationWindow::new(&app);
    overlay_window.set_decorated(false);
    overlay_window.set_default_size(1920, 1080);
    overlay_window.set_title(Some("overlay"));

    let css = gtk4::CssProvider::new();
    css.load_from_data("window.overlay-window, window.overlay-window.background { background: transparent; }");
    gtk4::style_context_add_provider_for_display(
        &gdk4::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    overlay_window.add_css_class("overlay-window");

    let overlay_webview = webkit6::WebView::new();
    overlay_webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&overlay_webview) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_write_console_messages_to_stdout(true);
    }
    overlay_webview.connect_context_menu(|_, _, _| true);

    let html = OVERLAY_HTML.replace("__OVERLAY_JS__", &strip_ts_inline(OVERLAY_JS));
    overlay_webview.load_html(&html, None);

    overlay_window.set_child(Some(&overlay_webview));
    overlay_window.present();

    {
        let mut s = state.borrow_mut();
        s.overlay_webview = Some(overlay_webview.clone());
        s.overlay_window = Some(overlay_window.clone());
    }

    // Key controller on the MENUBAR window.
    // The compositor routes Super+key events directly here via wl_keyboard.key.
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

    key_ctrl.connect_key_pressed({
        let state = state.clone();
        let bus = bus.clone();
        let overlay_webview = overlay_webview.clone();
        move |_, _keyval, keycode, gtk_modifiers| {
            let mut s = state.borrow_mut();
            let shift_held = gtk_modifiers.contains(gdk4::ModifierType::SHIFT_MASK);

            // Shell system shortcuts (highest priority).
            if let Some(action) = s.menus.lookup_shortcut(keycode, shift_held, "sola-shell") {
                tracing::info!(action_id = %action.action_id, "shell shortcut");
                if action.action_id == "exit" {
                    let _ = bus.borrow_mut().emit(Topic::Shutdown);
                }
                return glib::Propagation::Stop;
            }

            // Super+Tab: activate switcher.
            if keycode == keycode::TAB && !s.switcher.active {
                tracing::info!("activating switcher");
                s.switcher.active = true;
                s.switcher.selected = if s.switcher.apps.len() > 1 { 1 } else { 0 };
                let json = serde_json::to_string(&s.switcher.apps).unwrap_or_default();
                let script = format!("renderSwitcher({}, {})", json, s.switcher.selected);
                push_overlay_js(&overlay_webview, &script);
                // Raise the overlay above other windows (no focus change).
                let _ = bus.borrow_mut().emit(Topic::RaiseApp("sola-shell".into()));
                return glib::Propagation::Stop;
            }

            // Switcher navigation.
            if s.switcher.active {
                match keycode {
                    keycode::TAB | keycode::RIGHT => {
                        s.switcher.select_next();
                        let sel = s.switcher.selected;
                        push_overlay_js(&overlay_webview, &format!("setSelection({sel})"));
                        return glib::Propagation::Stop;
                    }
                    keycode::LEFT => {
                        s.switcher.select_prev();
                        let sel = s.switcher.selected;
                        push_overlay_js(&overlay_webview, &format!("setSelection({sel})"));
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }

            // Zone snapping (Super+Numpad).
            if let Some(geo) = s.zoning.handle_key(keycode) {
                let _ = bus.borrow_mut().emit(Topic::SetWindowGeometry(geo));
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
                    let _ = bus.borrow_mut().emit(Topic::MenuAction(action));
                    return glib::Propagation::Stop;
                }
            }

            glib::Propagation::Proceed
        }
    });

    key_ctrl.connect_key_released({
        let state = state.clone();
        let overlay_webview = overlay_webview.clone();
        let bus = bus.clone();
        move |_, _keyval, keycode, _modifiers| {
            if keycode != keycode::SUPER_L {
                return;
            }

            let app_id = {
                let mut s = state.borrow_mut();
                if !s.switcher.active {
                    return;
                }
                s.switcher.active = false;
                s.switcher.selected_app_id().map(String::from)
            };

            tracing::info!(app_id = ?app_id, "deactivating switcher");
            push_overlay_js(&overlay_webview, "clear()");

            if let Some(app_id) = app_id {
                let _ = bus.borrow_mut().emit(Topic::RaiseApp(app_id));
            }
        }
    });

    overlay_webview.connect_notify_local(Some("title"), {
        let state = state.clone();
        let bus = bus.clone();
        move |webview, _| {
            let Some(title) = webview.title() else { return };
            let title = title.to_string();
            if let Ok(index) = title.parse::<usize>() {
                state.borrow_mut().switcher.selected = index;
            } else if let Some(action) = title.strip_prefix("action:") {
                tracing::info!(action, "overlay menu action");
                if action == "exit" {
                    let _ = bus.borrow_mut().emit(Topic::Shutdown);
                }
            }
        }
    });

    window.add_controller(key_ctrl);
}

fn push_overlay_js(webview: &webkit6::WebView, script: &str) {
    webview.evaluate_javascript(script, None, None, None::<&gio::Cancellable>, |_| {});
}

fn strip_ts_inline(ts: &str) -> String {
    ts.to_string()
}
