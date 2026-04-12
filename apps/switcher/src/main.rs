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
    // Logging: stderr + file at /opt/sola/log/sola-switcher.log
    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola-switcher.log");

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_switcher=info".into());

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!("sola-switcher starting");

    // Ensure WAYLAND_DISPLAY is set.
    if std::env::var("WAYLAND_DISPLAY").is_err() {
        unsafe { std::env::set_var("WAYLAND_DISPLAY", "wayland-0") };
    }

    // Wait for the Wayland socket to exist before starting GTK.
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap();
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .expect("XDG_RUNTIME_DIR must be set");
    let socket_path = std::path::PathBuf::from(&runtime_dir).join(&wayland_display);
    for attempt in 1..=20 {
        if socket_path.exists() {
            tracing::info!(path = %socket_path.display(), "wayland socket ready");
            break;
        }
        if attempt == 20 {
            tracing::error!(path = %socket_path.display(), "wayland socket not found after 10s, exiting");
            std::process::exit(1);
        }
        tracing::debug!(attempt, path = %socket_path.display(), "waiting for wayland socket");
        std::thread::sleep(Duration::from_millis(500));
    }

    // GDK4 Wayland uses prgname as the xdg_toplevel app_id.
    glib::set_prgname(Some("sola-switcher"));

    let app = gtk4::Application::new(None::<&str>, Default::default());

    app.connect_activate(|app| {
        let state = Rc::new(RefCell::new(State::default()));
        let bus: Rc<RefCell<Option<BusClient>>> = Rc::new(RefCell::new(None));

        // --- Transparent window background via CSS ---
        let css = gtk4::CssProvider::new();
        css.load_from_data("window, window.background { background: transparent; }");
        gtk4::style_context_add_provider_for_display(
            &gdk4::Display::default().unwrap(),
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // --- Window: undecorated, full output size, NOT presented yet ---
        let window = gtk4::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.set_default_size(1920, 1080);

        // --- WebView: transparent background, loads embedded HTML ---
        let webview = webkit6::WebView::new();
        webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
        webview.load_html(HTML, None);
        window.set_child(Some(&webview));

        // --- JS -> Rust bridge via document.title ---
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

        // --- Bus polling ---
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
                                    tracing::info!("activating switcher (Super+Tab)");
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
                                tracing::info!(count = apps.len(), "received app list");
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
        // Capture phase so we see events before the WebView consumes them.
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);

        key_controller.connect_key_pressed({
            let state = state.clone();
            let webview = webview.clone();
            move |_, _keyval, keycode, _modifiers| {
                let s = state.borrow();
                if !s.active {
                    return glib::Propagation::Proceed;
                }
                drop(s);
                match keycode {
                    keycode::TAB | keycode::RIGHT => {
                        let mut s = state.borrow_mut();
                        s.select_next();
                        let sel = s.selected;
                        drop(s);
                        push_selection(&webview, sel);
                        glib::Propagation::Stop
                    }
                    keycode::LEFT => {
                        let mut s = state.borrow_mut();
                        s.select_prev();
                        let sel = s.selected;
                        drop(s);
                        push_selection(&webview, sel);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            }
        });

        key_controller.connect_key_released({
            let state = state.clone();
            let bus = bus.clone();
            let webview = webview.clone();
            move |_, _keyval, keycode, _modifiers| {
                if keycode != keycode::SUPER_L { return; }

                let app_id = {
                    let mut s = state.borrow_mut();
                    if !s.active { return; }
                    s.active = false;
                    s.selected_app_id().map(String::from)
                };

                tracing::info!(app_id = ?app_id, "deactivating switcher (Super released)");

                // Clear the UI so the window becomes transparent.
                webview.evaluate_javascript(
                    "render([], 0)", None, None,
                    None::<&gio::Cancellable>, |_| {},
                );

                if let Some(ref mut client) = *bus.borrow_mut() {
                    if let Some(app_id) = app_id {
                        let _ = client.emit(Topic::RaiseApp(app_id));
                    }
                    let _ = client.emit(Topic::ReleaseInput);
                }
            }
        });

        window.add_controller(key_controller);

        tracing::info!("switcher ready, waiting for Super+Tab");
        // Do NOT present the window — it stays hidden until Super+Tab.
    });

    app.run();
}

/// Push a selection update to the WebView.
fn push_selection(webview: &webkit6::WebView, index: usize) {
    let script = format!("setSelection({index})");
    webview.evaluate_javascript(&script, None, None, None::<&gio::Cancellable>, |_| {});
}
