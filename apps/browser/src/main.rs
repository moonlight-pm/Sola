mod chrome;
mod ipc;
mod state;
mod tabs;

use gtk4::prelude::*;
use sola_app::asset_bundle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::Topic;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/tabs.ts" => (include_str!("../web/src/tabs.ts"), TypeScript),
    "/src/address.ts" => (include_str!("../web/src/address.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
};

fn config_dir() -> PathBuf {
    let dir = glib::user_config_dir().join("sola");
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn setup_logging() {
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "sola_browser=info".into());

    let stderr_layer = fmt::layer().with_writer(std::io::stderr);
    let file_layer = fmt::layer().with_ansi(false).with_writer(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();
}

fn resolve_wayland_display(runtime_dir: &std::path::Path) -> String {
    if let Ok(v) = std::env::var("WAYLAND_DISPLAY") {
        if !v.is_empty() {
            return v;
        }
    }
    let name_file = runtime_dir.join("sola-wayland");
    for attempt in 1..=40 {
        if let Ok(contents) = std::fs::read_to_string(&name_file) {
            let name = contents.trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
        if attempt == 1 {
            tracing::info!(path = %name_file.display(), "waiting for sola-river to publish wayland socket name");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    tracing::warn!("sola-wayland name file never appeared; falling back to wayland-0");
    "wayland-0".to_string()
}

fn wait_for_wayland_socket() -> bool {
    let runtime_dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => {
            tracing::error!("XDG_RUNTIME_DIR not set");
            return false;
        }
    };
    // sola-river publishes the live socket name in $XDG_RUNTIME_DIR/sola-wayland.
    let display = resolve_wayland_display(&runtime_dir);
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &display) };
    let socket_path = runtime_dir.join(&display);
    for attempt in 1..=20 {
        if socket_path.exists() {
            tracing::info!(path = %socket_path.display(), "wayland socket ready");
            return true;
        }
        if attempt == 20 {
            tracing::error!(path = %socket_path.display(), "wayland socket not found after 10s");
            return false;
        }
        tracing::debug!(attempt, path = %socket_path.display(), "waiting for wayland socket");
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

fn main() {
    setup_logging();
    tracing::info!("sola-browser starting");

    // Enable remote Web Inspector if not already set (connect from any browser)
    if std::env::var("WEBKIT_INSPECTOR_HTTP_SERVER").is_err() {
        unsafe { std::env::set_var("WEBKIT_INSPECTOR_HTTP_SERVER", "0.0.0.0:9224") };
        tracing::info!("remote inspector enabled at http://0.0.0.0:9224");
    }

    unsafe { std::env::set_var("GTK_A11Y", "none") };

    // Watch own binary for hot-reload during development
    sola_app::watcher::watch_own_binary();

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

    // WebContext with app:/// URI scheme (serves app + platform assets)
    let platform = Box::leak(Box::new(sola_app::assets::platform_assets()));
    let html = APP_ASSETS
        .find("/index.html")
        .map(|a| a.content.to_string())
        .unwrap_or_else(|| "<html><body>No index.html</body></html>".to_string());
    let html = inject_import_map(&html);
    let web_context = sola_app::webview::create_web_context(APP_ASSETS, platform, html);

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

    // Chrome WebView with IPC handler
    let chrome_manager = webkit6::UserContentManager::new();
    let chrome_webview = webkit6::WebView::builder()
        .web_context(&web_context)
        .user_content_manager(&chrome_manager)
        .build();
    chrome_webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&chrome_webview) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_write_console_messages_to_stdout(true);
    }

    container.put(&chrome_webview, 0.0, 0.0);
    chrome_webview.set_size_request(1920, 1080);
    chrome_webview.load_uri("app:///index.html");

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
    });

    // IPC setup
    ipc::setup(&chrome_manager, &app_state);

    // Bus connection
    let bus: Rc<RefCell<BusClient>> = Rc::new(RefCell::new(BusClient::new()));
    {
        let mut client = bus.borrow_mut();
        client.set_app_id("sola-browser");
        if let Err(e) = client.connect() {
            tracing::warn!("bus not available: {e}");
        }
        let _ = client.emit_sticky(Topic::SetAppMenu(sola_bus::topics::AppMenuPayload {
            app_id: "sola-browser".into(),
            menus: vec![
                sola_bus::topics::MenuDefinition {
                    label: "Browser".into(),
                    items: vec![
                        sola_bus::topics::MenuItem::Action {
                            id: "new_tab".into(),
                            label: "New Tab".into(),
                            shortcut: Some(sola_core::KeyCode::T.meta()),
                            disabled: false,
                            checked: false,
                        },
                        sola_bus::topics::MenuItem::Action {
                            id: "close_tab".into(),
                            label: "Close Tab".into(),
                            shortcut: Some(sola_core::KeyCode::W.meta()),
                            disabled: false,
                            checked: false,
                        },
                        sola_bus::topics::MenuItem::Divider,
                        sola_bus::topics::MenuItem::Action {
                            id: "quit".into(),
                            label: "Quit Browser".into(),
                            shortcut: Some(sola_core::KeyCode::Q.meta()),
                            disabled: false,
                            checked: false,
                        },
                    ],
                },
                sola_bus::topics::MenuDefinition {
                    label: "Edit".into(),
                    items: vec![sola_bus::topics::MenuItem::Action {
                        id: "focus_address".into(),
                        label: "Focus Address Bar".into(),
                        shortcut: Some(sola_core::KeyCode::L.meta()),
                        disabled: false,
                        checked: false,
                    }],
                },
            ],
        }));
    }

    // Bus event source: keyboard shortcuts and OpenUrl handling
    let notify_fd = bus.borrow().notify_fd();
    if let Some(fd) = notify_fd {
        let app_state = app_state.clone();
        let bus = bus.clone();
        glib::unix_fd_add_local(fd, glib::IOCondition::IN, move |_fd, _cond| {
            let client = bus.borrow();
            client.drain_notify();
            let mut messages = Vec::new();
            while let Some(msg) = client.try_recv() {
                messages.push(msg);
            }
            drop(client);

            for msg in messages {
                let Some(topic) = Topic::parse(&msg) else {
                    continue;
                };
                match topic {
                    Topic::MenuAction(action) if action.app_id == "sola-browser" => {
                        match action.action_id.as_str() {
                            "new_tab" => {
                                let tab_id = uuid::Uuid::new_v4().to_string();
                                tabs::create_tab_webview(&app_state, &tab_id, None, None);
                                tabs::switch_tab(&app_state, &tab_id);

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
                                ipc::emit_event(&app_state.chrome_webview, "bus_new_tab", &data);
                                app_state.chrome_webview.grab_focus();
                                tracing::debug!("new tab {tab_id}");
                            }
                            "close_tab" => {
                                let active_id = app_state.active_tab_id.borrow().clone();
                                if let Some(id) = active_id {
                                    tabs::close_tab(&app_state, &id);

                                    let tabs = app_state.tabs.borrow();
                                    let next_id = tabs.last().map(|t| t.id.clone());
                                    drop(tabs);
                                    if let Some(next) = &next_id {
                                        tabs::switch_tab(&app_state, next);
                                    }

                                    ipc::emit_event(
                                        &app_state.chrome_webview,
                                        "tab_closed",
                                        &serde_json::json!({
                                            "tabId": id,
                                            "nextTabId": next_id,
                                        }),
                                    );
                                    tracing::debug!("closed tab {id}");
                                }
                            }
                            "focus_address" => {
                                ipc::emit_event(
                                    &app_state.chrome_webview,
                                    "bus_focus_address",
                                    &serde_json::json!({}),
                                );
                                app_state.chrome_webview.grab_focus();
                                tracing::debug!("focus address bar");
                            }
                            _ => {}
                        }
                    }
                    Topic::OpenUrl(req) => {
                        let tab_id = uuid::Uuid::new_v4().to_string();
                        tabs::create_tab_webview(&app_state, &tab_id, Some(&req.url), None);
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
                        ipc::emit_event(&app_state.chrome_webview, "bus_new_tab", &data);
                        tracing::info!(url = %req.url, "OpenUrl: created tab {tab_id}");
                    }
                    _ => {}
                }
            }
            glib::ControlFlow::Continue
        });
    }

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
    tracing::info!("sola-browser ready");
}

/// Inject the platform import map into HTML.
fn inject_import_map(html: &str) -> String {
    let platform_imports = r#""@arrow-js/core": "/vendor/arrow/index.mjs",
      "@sola/ipc": "/lib/ipc.js",
      "@sola/store": "/lib/store.js",
      "@sola/theme": "/lib/theme.js""#;

    // If there's an existing import map, merge into it
    if let Some(pos) = html.find("\"imports\"") {
        if let Some(brace) = html[pos..].find('{') {
            let insert_pos = pos + brace + 1;
            let mut result = String::with_capacity(html.len() + 100);
            result.push_str(&html[..insert_pos]);
            result.push('\n');
            result.push_str("      ");
            result.push_str(platform_imports);
            result.push(',');
            result.push_str(&html[insert_pos..]);
            return result;
        }
    }

    // No import map found -- inject one before first <script>
    let import_map = format!(
        r#"  <script type="importmap">
  {{
    "imports": {{
      {platform_imports}
    }}
  }}
  </script>
"#
    );

    if let Some(pos) = html.find("<script") {
        let mut result = String::with_capacity(html.len() + import_map.len());
        result.push_str(&html[..pos]);
        result.push_str(&import_map);
        result.push_str(&html[pos..]);
        result
    } else {
        html.to_string()
    }
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
}

impl AppState {
    fn persist_tabs(&self) {
        self.tab_store.borrow().save(&self.tab_store_path);
    }

    fn persist_history(&self) {
        self.history.borrow().save(&self.history_path);
    }
}
