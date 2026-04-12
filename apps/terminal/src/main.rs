use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gio::prelude::*;
use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::{KeyEvent, Topic};

mod pty;
mod server;
mod state;
mod tmux;

/// HTML template with __RESTORED_TABS__ placeholder.
const HTML_TEMPLATE: &str = include_str!("../web/index.html");

/// XKB keycode for T (evdev 20 + 8 = 28).
const KEY_T: u32 = 28;

// Embedded source files (TypeScript stripped on-demand, CSS served as-is).
mod assets {
    // App source (TypeScript, stripped on-demand)
    pub const MAIN_TS: &str = include_str!("../web/src/main.ts");
    pub const APP_TS: &str = include_str!("../web/src/app.ts");
    pub const TERMINAL_PANE_TS: &str = include_str!("../web/src/terminal-pane.ts");

    // Lib (extractable to sola-app)
    pub const LIB_IPC_TS: &str = include_str!("../web/src/lib/ipc.ts");
    pub const LIB_STORE_TS: &str = include_str!("../web/src/lib/store.ts");
    pub const LIB_THEME_TS: &str = include_str!("../web/src/lib/theme.ts");

    // Components (extractable to sola-app)
    pub const SIDEBAR_TS: &str = include_str!("../web/src/components/sidebar.ts");

    // CSS
    pub const THEME_CSS: &str = include_str!("../web/src/theme.css");

    // Vendored JS
    pub const XTERM_MJS: &str = include_str!("../web/vendor/xterm.mjs");
    pub const XTERM_CSS: &str = include_str!("../web/vendor/xterm.css");
    pub const ADDON_FIT_MJS: &str = include_str!("../web/vendor/addon-fit.mjs");
    pub const ADDON_WEB_LINKS_MJS: &str = include_str!("../web/vendor/addon-web-links.mjs");
    pub const ARROW_INDEX_MJS: &str = include_str!("../web/vendor/arrow/index.mjs");
    pub const ARROW_INTERNAL_MJS: &str = include_str!("../web/vendor/arrow/chunks/internal-DchK7S7v.mjs");
}

/// Strip TypeScript type annotations, returning JavaScript.
fn strip_ts(source: &str) -> String {
    use swc_common::errors::Handler;
    use swc_common::sync::Lrc;
    use swc_common::SourceMap;
    use swc_ts_fast_strip::{operate, Mode, Options};

    let cm: Lrc<SourceMap> = Default::default();
    let handler = Handler::with_emitter_writer(Box::new(std::io::sink()), Some(cm.clone()));

    match operate(
        &cm,
        &handler,
        source.to_string(),
        Options {
            mode: Mode::StripOnly,
            ..Default::default()
        },
    ) {
        Ok(output) => output.code,
        Err(e) => {
            tracing::error!("TS strip failed: {e:?}");
            source.to_string()
        }
    }
}

/// Send a JSON message to the JS frontend via evaluate_javascript.
fn send_to_js(webview: &webkit6::WebView, msg: &str) {
    let js_str = serde_json::to_string(msg).unwrap_or_default();
    let script = format!("window.__solaRecv({js_str})");
    webview.evaluate_javascript(&script, None::<&str>, None::<&str>, None::<&gio::Cancellable>, |result| {
        if let Err(e) = result {
            tracing::debug!("JS eval error: {e}");
        }
    });
}

fn main() {
    // Logging: stderr + file at /opt/sola/log/sola-terminal.log
    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola-terminal.log");

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_terminal=info".into());

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

    tracing::info!("sola-terminal starting");

    // tmux cleanup
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::reload_config();

    // Ensure WAYLAND_DISPLAY is set.
    if std::env::var("WAYLAND_DISPLAY").is_err() {
        unsafe { std::env::set_var("WAYLAND_DISPLAY", "wayland-0") };
    }

    // Wait for the Wayland socket to exist before starting GTK.
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap();
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR must be set");
    let socket_path = std::path::PathBuf::from(&runtime_dir).join(&wayland_display);
    for attempt in 1..=20 {
        if socket_path.exists() {
            tracing::info!(path = %socket_path.display(), "wayland socket ready");
            break;
        }
        if attempt == 20 {
            tracing::error!(
                path = %socket_path.display(),
                "wayland socket not found after 10s, exiting"
            );
            std::process::exit(1);
        }
        tracing::debug!(
            attempt,
            path = %socket_path.display(),
            "waiting for wayland socket"
        );
        std::thread::sleep(Duration::from_millis(500));
    }

    // GDK4 Wayland uses prgname as the xdg_toplevel app_id.
    glib::set_prgname(Some("sola-terminal"));

    let app = gtk4::Application::new(None::<&str>, Default::default());

    app.connect_activate(|app| {
        // Load restored state and serialize for the web frontend
        let restored_tabs = state::TerminalState::load_from_disk();
        let restored_json = serde_json::to_string(&restored_tabs).unwrap_or_default();

        let terminal_state = Arc::new(state::TerminalState::new());

        // Populate custom_titles from restored data.
        {
            let mut titles = terminal_state.custom_titles.try_write().unwrap();
            for tab in &restored_tabs {
                if let Some(ref title) = tab.custom_title {
                    titles.insert(tab.tmux_session.clone(), title.clone());
                }
            }
        }

        // Build the HTML template with restored tabs.
        let html = HTML_TEMPLATE.replace("__RESTORED_TABS__", &restored_json);

        // --- WebContext with app:/// URI scheme ---
        let web_context = webkit6::WebContext::new();
        let html_clone = html.clone();
        web_context.register_uri_scheme("app", move |request| {
            let uri = request.uri().unwrap_or_default().to_string();
            // Extract path, stripping query/fragment
            let path = uri
                .strip_prefix("app://")
                .unwrap_or(&uri)
                .split('?')
                .next()
                .unwrap_or("/")
                .split('#')
                .next()
                .unwrap_or("/");
            // Normalize: app:///path gives "/path", app:// gives ""
            let path = if path.is_empty() { "/" } else { path };

            // Browser requests .js (from import extensions), but sources are .ts.
            // Map both /src/x.ts and /src/x.js to the same TypeScript source.
            let (body, content_type) = match path {
                "/" | "/index.html" => (html_clone.clone(), "text/html; charset=utf-8"),
                // App source
                "/src/main.ts" | "/src/main.js" => (strip_ts(assets::MAIN_TS), "application/javascript"),
                "/src/app.ts" | "/src/app.js" => (strip_ts(assets::APP_TS), "application/javascript"),
                "/src/terminal-pane.ts" | "/src/terminal-pane.js" => (strip_ts(assets::TERMINAL_PANE_TS), "application/javascript"),
                // Lib
                "/src/lib/ipc.ts" | "/src/lib/ipc.js" => (strip_ts(assets::LIB_IPC_TS), "application/javascript"),
                "/src/lib/store.ts" | "/src/lib/store.js" => (strip_ts(assets::LIB_STORE_TS), "application/javascript"),
                "/src/lib/theme.ts" | "/src/lib/theme.js" => (strip_ts(assets::LIB_THEME_TS), "application/javascript"),
                // Components
                "/src/components/sidebar.ts" | "/src/components/sidebar.js" => (strip_ts(assets::SIDEBAR_TS), "application/javascript"),
                // CSS
                "/src/theme.css" => (assets::THEME_CSS.to_string(), "text/css"),
                // Vendored JS
                "/vendor/xterm.mjs" => (assets::XTERM_MJS.to_string(), "application/javascript"),
                "/vendor/xterm.css" => (assets::XTERM_CSS.to_string(), "text/css"),
                "/vendor/addon-fit.mjs" => (assets::ADDON_FIT_MJS.to_string(), "application/javascript"),
                "/vendor/addon-web-links.mjs" => (assets::ADDON_WEB_LINKS_MJS.to_string(), "application/javascript"),
                "/vendor/arrow/index.mjs" => (assets::ARROW_INDEX_MJS.to_string(), "application/javascript"),
                "/vendor/arrow/chunks/internal-DchK7S7v.mjs" => (assets::ARROW_INTERNAL_MJS.to_string(), "application/javascript"),
                _ => {
                    tracing::warn!("404: {path}");
                    let body = "Not Found".to_string();
                    let bytes = body.into_bytes();
                    let gbytes = glib::Bytes::from(&bytes);
                    let stream = gio::MemoryInputStream::from_bytes(&gbytes);
                    request.finish(&stream, bytes.len() as i64, Some("text/plain"));
                    return;
                }
            };

            let bytes = body.into_bytes();
            let len = bytes.len() as i64;
            let gbytes = glib::Bytes::from(&bytes);
            let stream = gio::MemoryInputStream::from_bytes(&gbytes);
            request.finish(&stream, len, Some(content_type));
        });

        // --- UserContentManager for JS <-> Rust IPC ---
        let ucm = webkit6::UserContentManager::new();
        ucm.register_script_message_handler("sola", None::<&str>);

        // Channel: glib -> tokio (commands from JS)
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        // Channel: tokio -> glib (responses + PTY events)
        let (event_tx, event_rx) = std::sync::mpsc::channel::<String>();

        // Handle JS -> Rust messages
        let cmd_tx_clone = cmd_tx.clone();
        ucm.connect_script_message_received(Some("sola"), move |_ucm, js_value| {
            let msg: String = js_value.to_string().into();
            let _ = cmd_tx_clone.send(msg);
        });

        // Spawn tokio runtime for PTY management + command dispatch
        let server_state = terminal_state.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(server::command_loop(server_state, cmd_rx, event_tx));
        });

        // Window: undecorated, 1920x1080
        let window = gtk4::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.set_default_size(1920, 1080);

        // WebView with custom context, UCM, and developer extras
        let webview = webkit6::WebView::builder()
            .web_context(&web_context)
            .user_content_manager(&ucm)
            .build();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
            settings.set_enable_developer_extras(true);
            settings.set_enable_write_console_messages_to_stdout(true);
        }
        window.set_child(Some(&webview));

        // Poll tokio -> glib messages and forward to JS
        let webview_for_events = webview.clone();
        glib::timeout_add_local(Duration::from_millis(2), move || {
            while let Ok(msg) = event_rx.try_recv() {
                send_to_js(&webview_for_events, &msg);
            }
            glib::ControlFlow::Continue
        });

        // Load frontend from custom URI scheme (no network)
        webview.load_uri("app:///index.html");

        // Bus connection
        let bus: Rc<RefCell<Option<BusClient>>> = Rc::new(RefCell::new(None));
        match BusClient::connect() {
            Ok(client) => {
                tracing::info!("connected to bus");
                *bus.borrow_mut() = Some(client);
            }
            Err(e) => tracing::warn!("bus not available: {e}"),
        }

        // Bus polling: listen for Super+T to create new tab
        let webview_for_bus = webview.clone();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let mut bus_ref = bus.borrow_mut();
            if let Some(ref mut client) = *bus_ref {
                while let Some(msg) = client.try_recv() {
                    let Some(topic) = Topic::parse(&msg) else {
                        continue;
                    };
                    match topic {
                        Topic::Key(KeyEvent {
                            code: KEY_T,
                            pressed: true,
                            super_held: true,
                            ..
                        }) => {
                            tracing::info!("Super+T: requesting new tab");
                            let msg = serde_json::json!({"event": "new_tab"}).to_string();
                            send_to_js(&webview_for_bus, &msg);
                        }
                        _ => {}
                    }
                }
            }
            glib::ControlFlow::Continue
        });

        window.present();
        tracing::info!("sola-terminal ready");
    });

    app.run();
}
