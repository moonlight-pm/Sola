mod chrome;
mod ipc;
mod state;
mod tabs;

use gtk4::prelude::*;
use include_dir::{include_dir, Dir};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::Topic;

/// XKB keycodes (evdev + 8) -- same values the compositor puts on the bus.
mod keycode {
    pub const T: u32 = 28;
    pub const W: u32 = 25;
    pub const L: u32 = 46;
}

static WEB_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web");

fn config_dir() -> PathBuf {
    let dir = glib::user_config_dir().join("sola");
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn mime_from_extension(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

fn setup_logging() {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let log_dir = "/opt/sola/log";
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| "sola_browser=info".into());

    let stderr_layer = fmt::layer().with_writer(std::io::stderr);

    if let Ok(file_appender) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{log_dir}/sola-browser.log"))
    {
        let file_layer = fmt::layer()
            .with_writer(std::sync::Mutex::new(file_appender))
            .with_ansi(false);
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .with(file_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .init();
    }
}

fn wait_for_wayland_socket() -> bool {
    let display = match std::env::var("WAYLAND_DISPLAY") {
        Ok(d) => d,
        Err(_) => {
            tracing::error!("WAYLAND_DISPLAY not set");
            return false;
        }
    };
    let runtime_dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) => d,
        Err(_) => {
            tracing::error!("XDG_RUNTIME_DIR not set");
            return false;
        }
    };
    let socket_path = PathBuf::from(&runtime_dir).join(&display);
    for attempt in 1..=20 {
        if socket_path.exists() {
            tracing::info!("wayland socket ready (attempt {attempt})");
            return true;
        }
        tracing::debug!("waiting for wayland socket (attempt {attempt}/20)");
        std::thread::sleep(Duration::from_millis(500));
    }
    tracing::error!("wayland socket not found after 10s");
    false
}

fn main() {
    setup_logging();
    tracing::info!("sola-browser starting");

    if !wait_for_wayland_socket() {
        std::process::exit(1);
    }

    glib::set_prgname(Some("sola-browser"));

    let app = gtk4::Application::new(None::<&str>, Default::default());
    app.connect_activate(build_ui);
    app.run_with_args::<String>(&[]);
}

fn build_ui(app: &gtk4::Application) {
    let display = gdk4::Display::default().expect("could not get display");

    // Transparent window CSS
    let css = gtk4::CssProvider::new();
    css.load_from_data("window, window.background { background: transparent; }");
    gtk4::style_context_add_provider_for_display(
        &display,
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Window
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Sola Browser")
        .default_width(1920)
        .default_height(1080)
        .decorated(false)
        .build();

    let container = gtk4::Fixed::new();
    window.set_child(Some(&container));

    // WebKit setup
    let web_context = webkit6::WebContext::new();
    web_context.register_uri_scheme("sola-browser", |request| {
        let path = request.path().unwrap_or_default().to_string();
        let path = path.strip_prefix('/').unwrap_or(&path);
        let path = if path.is_empty() { "index.html" } else { path };
        match WEB_DIST.get_file(path) {
            Some(file) => {
                let data = file.contents();
                let mime = mime_from_extension(path);
                let bytes = glib::Bytes::from(data);
                let stream = gio::MemoryInputStream::from_bytes(&bytes);
                request.finish(&stream, data.len() as i64, Some(mime));
            }
            None => {
                tracing::warn!("embedded file not found: {path}");
                let bytes = glib::Bytes::from(b"Not Found" as &[u8]);
                let stream = gio::MemoryInputStream::from_bytes(&bytes);
                request.finish(&stream, 9, Some("text/plain"));
            }
        }
    });

    // Network session for tab WebViews (cookies, cache)
    let data_dir = glib::user_data_dir().join("sola").join("browser");
    let cache_dir = glib::user_cache_dir().join("sola").join("browser");
    std::fs::create_dir_all(&data_dir).ok();
    std::fs::create_dir_all(&cache_dir).ok();
    let network_session = webkit6::NetworkSession::new(
        Some(data_dir.to_str().unwrap()),
        Some(cache_dir.to_str().unwrap()),
    );
    if let Some(cookie_mgr) = network_session.cookie_manager() {
        let cookie_db = data_dir.join("cookies.db");
        cookie_mgr.set_persistent_storage(
            cookie_db.to_str().unwrap(),
            webkit6::CookiePersistentStorage::Sqlite,
        );
    }

    // Chrome WebView
    let chrome_manager = webkit6::UserContentManager::new();
    let chrome_webview = webkit6::WebView::builder()
        .web_context(&web_context)
        .user_content_manager(&chrome_manager)
        .build();
    chrome_webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&chrome_webview) {
        settings.set_enable_developer_extras(true);
    }

    container.put(&chrome_webview, 0.0, 0.0);
    chrome_webview.set_size_request(1920, 1080);
    chrome_webview.load_uri("sola-browser://index.html");

    // Shared state
    let app_state = Rc::new(AppState {
        container: container.clone(),
        chrome_webview: chrome_webview.clone(),
        web_context,
        network_session,
        tab_store_path: config_dir().join("browser-tabs.json"),
        history_path: config_dir().join("browser-history.json"),
        tab_store: RefCell::new(state::TabStore::load(
            &config_dir().join("browser-tabs.json"),
        )),
        history: RefCell::new(state::BrowsingHistory::load(
            &config_dir().join("browser-history.json"),
        )),
        tabs: RefCell::new(Vec::new()),
        active_tab_id: RefCell::new(None),
        focused: RefCell::new(false),
    });

    // IPC setup
    ipc::setup(&chrome_manager, &app_state);

    // Bus connection
    let bus: Rc<RefCell<Option<BusClient>>> = Rc::new(RefCell::new(None));
    match BusClient::connect() {
        Ok(client) => {
            tracing::info!("connected to bus");
            *bus.borrow_mut() = Some(client);
        }
        Err(e) => tracing::warn!("bus not available: {e}"),
    }

    // Bus poll loop: keyboard shortcuts and OpenUrl handling
    glib::timeout_add_local(Duration::from_millis(50), {
        let app_state = app_state.clone();
        let bus = bus.clone();
        move || {
            let mut bus_ref = bus.borrow_mut();
            if let Some(ref mut client) = *bus_ref {
                while let Some(msg) = client.try_recv() {
                    let Some(topic) = Topic::parse(&msg) else { continue };
                    match topic {
                        Topic::Key(key) => {
                            if key.pressed && key.super_held && *app_state.focused.borrow() {
                                match key.code {
                                    keycode::T => {
                                        let tab_id = uuid::Uuid::new_v4().to_string();
                                        tabs::create_tab_webview(
                                            &app_state, &tab_id, None, None,
                                        );
                                        tabs::switch_tab(&app_state, &tab_id);

                                        // Persist new tab
                                        let mut store = app_state.tab_store.borrow_mut();
                                        store.tabs.push(state::PersistedTab {
                                            url: String::new(),
                                            title: String::new(),
                                            session_state: None,
                                        });
                                        drop(store);
                                        app_state.persist_tabs();

                                        let data = serde_json::json!({
                                            "tabId": tab_id,
                                            "url": "",
                                            "activate": true,
                                        });
                                        ipc::emit_event_json(
                                            &app_state.chrome_webview,
                                            "bus_new_tab",
                                            &data,
                                        );
                                        tracing::debug!("Super+T: new tab {tab_id}");
                                    }
                                    keycode::W => {
                                        let active_id =
                                            app_state.active_tab_id.borrow().clone();
                                        if let Some(id) = active_id {
                                            tabs::close_tab(&app_state, &id);
                                            ipc::emit_event_json(
                                                &app_state.chrome_webview,
                                                "tab_closed",
                                                &serde_json::json!({ "tabId": id }),
                                            );
                                            tracing::debug!("Super+W: closed tab {id}");
                                        }
                                    }
                                    keycode::L => {
                                        ipc::emit_event_json(
                                            &app_state.chrome_webview,
                                            "bus_focus_address",
                                            &serde_json::json!({}),
                                        );
                                        tracing::debug!("Super+L: focus address bar");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Topic::FocusChanged(app_id) => {
                            let is_focused = app_id == "sola-browser";
                            *app_state.focused.borrow_mut() = is_focused;
                            tracing::debug!(app_id, is_focused, "focus changed");
                        }
                        Topic::OpenUrl(req) => {
                            let tab_id = uuid::Uuid::new_v4().to_string();
                            tabs::create_tab_webview(
                                &app_state,
                                &tab_id,
                                Some(&req.url),
                                None,
                            );
                            if req.activate {
                                tabs::switch_tab(&app_state, &tab_id);
                            }

                            // Persist new tab
                            let mut store = app_state.tab_store.borrow_mut();
                            store.tabs.push(state::PersistedTab {
                                url: req.url.clone(),
                                title: String::new(),
                                session_state: None,
                            });
                            drop(store);
                            app_state.persist_tabs();

                            let data = serde_json::json!({
                                "tabId": tab_id,
                                "url": req.url,
                                "activate": req.activate,
                            });
                            ipc::emit_event_json(
                                &app_state.chrome_webview,
                                "bus_new_tab",
                                &data,
                            );
                            tracing::info!(url = %req.url, "OpenUrl: created tab {tab_id}");
                        }
                        _ => {}
                    }
                }
            }
            glib::ControlFlow::Continue
        }
    });

    // Handle window resize
    window.connect_default_width_notify({
        let app_state = app_state.clone();
        move |win| resize_views(&app_state, win.width(), win.height())
    });
    window.connect_default_height_notify({
        let app_state = app_state.clone();
        move |win| resize_views(&app_state, win.width(), win.height())
    });

    // Capture session state on close
    window.connect_close_request({
        let app_state = app_state.clone();
        move |_| {
            tracing::info!("browser window closing, capturing session state");
            tabs::capture_session_state(&app_state);
            glib::Propagation::Proceed
        }
    });

    window.present();
}

fn resize_views(app_state: &AppState, width: i32, height: i32) {
    app_state.chrome_webview.set_size_request(width, height);
    let area = chrome::content_area(width, height);
    for tab in app_state.tabs.borrow().iter() {
        tab.webview.set_size_request(area.width, area.height);
        app_state
            .container
            .move_(&tab.webview, area.x as f64, area.y as f64);
    }
}

struct Tab {
    id: String,
    webview: webkit6::WebView,
}

struct AppState {
    container: gtk4::Fixed,
    chrome_webview: webkit6::WebView,
    web_context: webkit6::WebContext,
    network_session: webkit6::NetworkSession,
    tab_store_path: PathBuf,
    history_path: PathBuf,
    tab_store: RefCell<state::TabStore>,
    history: RefCell<state::BrowsingHistory>,
    tabs: RefCell<Vec<Tab>>,
    active_tab_id: RefCell<Option<String>>,
    focused: RefCell<bool>,
}

impl AppState {
    fn persist_tabs(&self) {
        self.tab_store.borrow().save(&self.tab_store_path);
    }

    fn persist_history(&self) {
        self.history.borrow().save(&self.history_path);
    }
}
