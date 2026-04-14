use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_app::{asset_bundle, SolaApp};
use sola_bus::topics::{App, KeyEvent, Topic};

mod keycode {
    pub const TAB: u32 = 23;
    pub const LEFT: u32 = 113;
    pub const RIGHT: u32 = 114;
    pub const SUPER_L: u32 = 133;
}

#[derive(Default)]
struct State {
    active: bool,
    apps: Vec<App>,
    selected: usize,
}

impl State {
    fn selected_app_id(&self) -> Option<&str> {
        self.apps.get(self.selected).map(|a| a.app_id.as_str())
    }

    fn select_next(&mut self) {
        if !self.apps.is_empty() {
            self.selected = (self.selected + 1) % self.apps.len();
        }
    }

    fn select_prev(&mut self) {
        if !self.apps.is_empty() {
            self.selected = (self.selected + self.apps.len() - 1) % self.apps.len();
        }
    }
}

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
};

fn main() {
    let state: Rc<RefCell<State>> = Rc::new(RefCell::new(State::default()));

    SolaApp::builder()
        .app_id("sola-switcher")
        .window_size(1920, 1080)
        .decorated(false)
        .transparent(true)
        .web_assets(APP_ASSETS)
        .on_bus_event({
            let state = state.clone();
            move |topic, send_to_js, emit| match topic {
                Topic::Key(KeyEvent {
                    code,
                    pressed: true,
                    super_held: true,
                    ..
                }) if *code == keycode::TAB && !state.borrow().active => {
                    tracing::info!("activating switcher (Super+Tab)");
                    state.borrow_mut().active = true;
                    emit(Topic::GrabInput("sola-switcher".into()));
                    emit(Topic::ListApps);
                }
                Topic::Apps(apps) if state.borrow().active => {
                    let mut s = state.borrow_mut();
                    tracing::info!(count = apps.len(), "received app list");
                    s.apps = apps.clone();
                    s.selected = if s.apps.len() > 1 { 1 } else { 0 };
                    let json = serde_json::to_string(&s.apps).unwrap_or_default();
                    send_to_js(serde_json::json!({
                        "event": "render",
                        "apps": json,
                        "selected": s.selected,
                    }));
                }
                _ => {}
            }
        })
        .on_activate({
            let state = state.clone();
            move |window, webview, bus| {
                let key_ctrl = gtk4::EventControllerKey::new();
                key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

                key_ctrl.connect_key_pressed({
                    let state = state.clone();
                    let webview = webview.clone();
                    move |_, _keyval, keycode, _modifiers| {
                        if !state.borrow().active {
                            return glib::Propagation::Proceed;
                        }
                        match keycode {
                            keycode::TAB | keycode::RIGHT => {
                                let mut s = state.borrow_mut();
                                s.select_next();
                                push_selection(&webview, s.selected);
                                glib::Propagation::Stop
                            }
                            keycode::LEFT => {
                                let mut s = state.borrow_mut();
                                s.select_prev();
                                push_selection(&webview, s.selected);
                                glib::Propagation::Stop
                            }
                            _ => glib::Propagation::Proceed,
                        }
                    }
                });

                key_ctrl.connect_key_released({
                    let state = state.clone();
                    let webview = webview.clone();
                    move |_, _keyval, keycode, _modifiers| {
                        if keycode != keycode::SUPER_L {
                            return;
                        }

                        let app_id = {
                            let mut s = state.borrow_mut();
                            if !s.active {
                                return;
                            }
                            s.active = false;
                            s.selected_app_id().map(String::from)
                        };

                        tracing::info!(app_id = ?app_id, "deactivating switcher");

                        webview.evaluate_javascript(
                            "clear()",
                            None,
                            None,
                            None::<&gio::Cancellable>,
                            |_| {},
                        );

                        let mut client = bus.borrow_mut();
                        if let Some(app_id) = app_id {
                            let _ = client.emit(Topic::RaiseApp(app_id));
                        }
                        let _ = client.emit(Topic::ReleaseInput);
                    }
                });

                // Mouse hover → JS sets document.title → sync Rust state
                webview.connect_notify_local(Some("title"), {
                    let state = state.clone();
                    move |webview, _| {
                        if let Some(title) = webview.title() {
                            if let Ok(index) = title.to_string().parse::<usize>() {
                                state.borrow_mut().selected = index;
                            }
                        }
                    }
                });

                window.add_controller(key_ctrl);
                tracing::info!("switcher ready, waiting for Super+Tab");
            }
        })
        .run();
}

fn push_selection(webview: &webkit6::WebView, index: usize) {
    let script = format!("setSelection({index})");
    webview.evaluate_javascript(&script, None, None, None::<&gio::Cancellable>, |_| {});
}
