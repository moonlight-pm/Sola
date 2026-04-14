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
}

impl ShellState {
    fn new() -> Self {
        Self {
            focused_app_id: None,
            menus: menus::MenuCache::new(),
            zoning: zoning::ZoningState::new(),
            switcher: switcher::SwitcherState::default(),
            overlay_webview: None,
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

                        let menu_labels: Vec<String> = s
                            .menus
                            .get_menu(app_id)
                            .map(|m| m.menus.iter().map(|d| d.label.clone()).collect())
                            .unwrap_or_default();

                        send_to_js(serde_json::json!({
                            "event": "focus",
                            "app_id": app_id,
                            "menu_labels": menu_labels,
                        }));
                    }
                    Topic::SetAppMenu(payload) => {
                        s.menus.set_menu(payload.clone());
                    }
                    Topic::OutputGeometry(geo) => {
                        s.zoning.set_output_size(geo);

                        // Position the menubar
                        if let Some(menubar_geo) = s.zoning.menubar_geometry() {
                            emit(Topic::SetWindowGeometry(menubar_geo));
                        }

                        // Restore saved zones with menubar offset
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
    overlay_window.present();

    state.borrow_mut().overlay_webview = Some(overlay_webview.clone());

    // Overlay key controller for switcher navigation
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

    // Mouse hover in switcher → JS sets document.title → sync Rust state
    overlay_webview.connect_notify_local(Some("title"), {
        let state = state.clone();
        move |webview, _| {
            if let Some(title) = webview.title() {
                if let Ok(index) = title.to_string().parse::<usize>() {
                    state.borrow_mut().switcher.selected = index;
                }
            }
        }
    });

    overlay_window.add_controller(key_ctrl);
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
