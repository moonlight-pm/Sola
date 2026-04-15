mod menus;
mod switcher;
mod zoning;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_app::{asset_bundle, SolaApp};
use sola_bus::topics::{KeyEvent, Topic};

mod keycode {
    pub const BACKSPACE: u32 = 22;
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
                    Topic::Key(key) => {
                        handle_key(&mut s, key, send_to_js, emit);
                    }
                    Topic::Apps(apps) => {
                        if s.switcher.active {
                            tracing::info!(count = apps.len(), "received app list for switcher");
                            s.switcher.apps = apps.clone();
                            s.switcher.selected = if apps.len() > 1 { 1 } else { 0 };
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

                        // Re-send focus data if this menu is for the focused app
                        // (handles out-of-order sticky replay on reconnect)
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
                // Declare window policies before any surfaces map
                use sola_bus::topics::{WindowPolicy, WindowPolicyPayload};
                let _ = bus.borrow_mut().emit_sticky(Topic::SetWindowPolicy(
                    WindowPolicyPayload {
                        app_id: "sola-shell".into(),
                        windows: vec![
                            WindowPolicy {
                                title: "menubar".into(),
                                zoned: false,
                                auto_focus: false,
                                size: Some((1920, zoning::MENUBAR_HEIGHT)),
                                position: Some((0, 0)),
                            },
                            WindowPolicy {
                                title: "overlay".into(),
                                zoned: false,
                                auto_focus: false,
                                size: None,
                                position: None,
                            },
                        ],
                    },
                ));

                window.set_title(Some("menubar"));

                // Menubar command bridge via title
                let webview_ref = {
                    // The webview is the window's child
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

fn handle_key(
    s: &mut ShellState,
    key: &KeyEvent,
    _send_to_js: &dyn Fn(serde_json::Value),
    emit: &dyn Fn(Topic),
) {
    // Super+Shift+Backspace → shutdown (safety fallback)
    if key.pressed && key.code == keycode::BACKSPACE && key.super_held && key.shift_held {
        tracing::info!("shutdown chord");
        emit(Topic::Shutdown);
        return;
    }

    // Super+Tab → activate switcher
    if key.pressed && key.code == keycode::TAB && key.super_held && !s.switcher.active {
        tracing::info!("activating switcher");
        s.switcher.active = true;
        emit(Topic::GrabInput("sola-shell".into()));
        emit(Topic::ListApps);
        return;
    }

    // Zone snapping (Super+Numpad)
    if let Some(geo) = s.zoning.handle_key(key) {
        emit(Topic::SetWindowGeometry(geo));
        return;
    }

    // Menu shortcut lookup
    if key.pressed && key.super_held {
        if let Some(focused) = &s.focused_app_id {
            let focused = focused.clone();
            if let Some(action) = s.menus.lookup_shortcut(key.code, key.shift_held, &focused) {
                tracing::info!(
                    app_id = %action.app_id,
                    action_id = %action.action_id,
                    "menu shortcut matched"
                );
                emit(Topic::MenuAction(action));
            }
        }
    }
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

    // Transparent CSS for overlay
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

    // Build overlay HTML with embedded JS (simple, no asset system needed)
    let html = OVERLAY_HTML.replace("__OVERLAY_JS__", &strip_ts_inline(OVERLAY_JS));
    overlay_webview.load_html(&html, None);

    overlay_window.set_child(Some(&overlay_webview));
    // Present now — the policy system prevents auto-focus and force-resize.
    // Empty + transparent = invisible. Avoids map/unmap flicker on each activation.
    overlay_window.present();

    {
        let mut s = state.borrow_mut();
        s.overlay_webview = Some(overlay_webview.clone());
        s.overlay_window = Some(overlay_window.clone());
    }

    // Switcher key controller on the MENUBAR window.
    // GrabInput focuses the menubar (first sola-shell surface), so the
    // menubar's key controller receives Tab/Arrow/Super release events.
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

    key_ctrl.connect_key_pressed({
        let state = state.clone();
        let overlay_webview = overlay_webview.clone();
        move |_, _keyval, keycode, _modifiers| {
            let s = state.borrow();
            if !s.switcher.active {
                return glib::Propagation::Proceed;
            }
            drop(s);

            match keycode {
                keycode::TAB | keycode::RIGHT => {
                    let mut s = state.borrow_mut();
                    s.switcher.select_next();
                    let sel = s.switcher.selected;
                    drop(s);
                    push_overlay_js(&overlay_webview, &format!("setSelection({sel})"));
                    glib::Propagation::Stop
                }
                keycode::LEFT => {
                    let mut s = state.borrow_mut();
                    s.switcher.select_prev();
                    let sel = s.switcher.selected;
                    drop(s);
                    push_overlay_js(&overlay_webview, &format!("setSelection({sel})"));
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
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

            let mut client = bus.borrow_mut();
            if let Some(app_id) = app_id {
                let _ = client.emit(Topic::RaiseApp(app_id));
            }
            let _ = client.emit(Topic::ReleaseInput);
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

    // Add to menubar window — it gets focus during GrabInput
    window.add_controller(key_ctrl);
}

fn push_overlay_js(webview: &webkit6::WebView, script: &str) {
    webview.evaluate_javascript(script, None, None, None::<&gio::Cancellable>, |_| {});
}

/// Minimal TS→JS strip for inline overlay code (just remove type annotations).
/// For the simple overlay code we write, just stripping `: type` suffices.
/// Full strip uses sola-app's swc-based stripper, but we don't have that here.
fn strip_ts_inline(ts: &str) -> String {
    // For the overlay, we write plain JS-compatible TS (no type-only syntax).
    // Just return as-is — the overlay code avoids TypeScript-only features.
    ts.to_string()
}
