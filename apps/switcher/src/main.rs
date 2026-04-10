use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::{App, Topic};

/// Switcher HTML/CSS/JS, embedded at compile time.
const HTML: &str = include_str!("../web/index.html");

/// XKB keycodes (evdev + 8) — same values the compositor puts on the bus.
mod keycode {
    pub const TAB: u32 = 23;
    pub const LEFT: u32 = 113;
    pub const RIGHT: u32 = 114;
    pub const SUPER_L: u32 = 133;
}

/// Switcher runtime state.
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sola_switcher=info".into()),
        )
        .init();

    // GDK4 Wayland uses prgname as the xdg_toplevel app_id.
    glib::set_prgname(Some("sola-switcher"));

    let app = gtk4::Application::new(None::<&str>, Default::default());

    app.connect_activate(|app| {
        let state = Rc::new(RefCell::new(State::default()));
        let bus: Rc<RefCell<Option<BusClient>>> = Rc::new(RefCell::new(None));

        // --- Window: undecorated, full output size, NOT presented yet ---
        let window = gtk4::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.set_default_size(1920, 1080);

        // --- WebView: transparent background, loads embedded HTML ---
        let webview = webkit6::WebView::new();
        webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
        webview.load_html(HTML, None);
        window.set_child(Some(&webview));

        // --- JS → Rust bridge via document.title ---
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

        // --- Bus connection ---
        match BusClient::connect() {
            Ok(client) => {
                tracing::info!("connected to bus");
                *bus.borrow_mut() = Some(client);
            }
            Err(e) => tracing::warn!("bus not available: {e}"),
        }

        // --- Bus polling (50ms) ---
        glib::timeout_add_local(Duration::from_millis(50), {
            let state = state.clone();
            let bus = bus.clone();
            let webview = webview.clone();
            let window = window.clone();
            move || {
                let mut bus_ref = bus.borrow_mut();
                if let Some(ref mut client) = *bus_ref {
                    while let Some(msg) = client.try_recv() {
                        let Some(topic) = Topic::parse(&msg) else { continue };
                        match topic {
                            Topic::Key(key) => {
                                if !state.borrow().active
                                    && key.pressed
                                    && key.code == keycode::TAB
                                    && key.super_held
                                {
                                    tracing::info!("activating switcher");
                                    state.borrow_mut().active = true;
                                    window.present();
                                    let _ = client.emit(
                                        Topic::GrabInput("sola-switcher".into()),
                                    );
                                    let _ = client.emit(Topic::ListApps);
                                }
                            }
                            Topic::Apps(apps) => {
                                let mut s = state.borrow_mut();
                                if !s.active { continue; }
                                s.apps = apps;
                                s.selected = if s.apps.len() > 1 { 1 } else { 0 };
                                let json =
                                    serde_json::to_string(&s.apps).unwrap_or_default();
                                let script =
                                    format!("render({json}, {})", s.selected);
                                webview.evaluate_javascript(
                                    &script, None, None,
                                    None::<&gio::Cancellable>,
                                    |_| {},
                                );
                            }
                            _ => {}
                        }
                    }
                }
                glib::ControlFlow::Continue
            }
        });

        // --- Keyboard: Tab/Arrow cycle, Super release completes ---
        let key_controller = gtk4::EventControllerKey::new();

        key_controller.connect_key_pressed({
            let state = state.clone();
            let webview = webview.clone();
            move |_, _keyval, keycode, _modifiers| {
                let mut s = state.borrow_mut();
                if !s.active {
                    return glib::Propagation::Proceed;
                }
                match keycode {
                    keycode::TAB | keycode::RIGHT => {
                        s.select_next();
                        push_selection(&webview, s.selected);
                        glib::Propagation::Stop
                    }
                    keycode::LEFT => {
                        s.select_prev();
                        push_selection(&webview, s.selected);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            }
        });

        key_controller.connect_key_released({
            let state = state.clone();
            let bus = bus.clone();
            let window = window.clone();
            move |_, _keyval, keycode, _modifiers| {
                if keycode != keycode::SUPER_L { return; }

                let app_id = {
                    let mut s = state.borrow_mut();
                    if !s.active { return; }
                    s.active = false;
                    s.selected_app_id().map(String::from)
                };

                tracing::info!("deactivating switcher");

                if let Some(ref mut client) = *bus.borrow_mut() {
                    if let Some(app_id) = app_id {
                        let _ = client.emit(Topic::RaiseApp(app_id));
                    }
                    let _ = client.emit(Topic::ReleaseInput);
                }

                window.set_visible(false);
            }
        });

        window.add_controller(key_controller);

        // Do NOT present the window — it stays hidden until Super+Tab.
    });

    app.run();
}

/// Push a selection update to the WebView.
fn push_selection(webview: &webkit6::WebView, index: usize) {
    let script = format!("setSelection({index})");
    webview.evaluate_javascript(&script, None, None, None::<&gio::Cancellable>, |_| {});
}
