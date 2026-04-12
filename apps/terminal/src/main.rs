use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::{KeyEvent, Topic};

mod pty;
mod server;
mod state;
mod tmux;

/// Embedded HTML with placeholders replaced at runtime.
const HTML: &str = include_str!("../web/dist/index.html");

/// XKB keycode for T (evdev 20 + 8 = 28).
const KEY_T: u32 = 28;

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
        // Safe: no tokio runtime yet, no contention.
        {
            let mut titles = terminal_state.custom_titles.try_write().unwrap();
            for tab in &restored_tabs {
                if let Some(ref title) = tab.custom_title {
                    titles.insert(tab.tmux_session.clone(), title.clone());
                }
            }
        }

        // Channel for glib -> tokio bus events
        let (bus_tx, bus_rx) = tokio::sync::mpsc::unbounded_channel::<server::BusEvent>();

        // Spawn tokio runtime on a background thread, get the WS port back
        let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
        let server_state = terminal_state.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(async {
                let port = server::start(server_state, bus_rx).await;
                let _ = port_tx.send(port);
                // Keep the runtime alive forever
                futures_util::future::pending::<()>().await;
            });
        });

        let port = port_rx.recv().expect("failed to receive WS port from server thread");
        tracing::info!(port, "WebSocket server ready");

        // Window: undecorated, 1920x1080
        let window = gtk4::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.set_default_size(1920, 1080);

        // WebView with developer extras
        let webview = webkit6::WebView::new();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
            settings.set_enable_developer_extras(true);
        }
        window.set_child(Some(&webview));

        // Load HTML with placeholders replaced
        let html = HTML
            .replace("__WS_PORT__", &port.to_string())
            .replace("__RESTORED_TABS__", &restored_json);
        webview.load_html(&html, None);

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
        let bus_tx = Rc::new(bus_tx);
        glib::timeout_add_local(Duration::from_millis(50), {
            let bus = bus.clone();
            let bus_tx = bus_tx.clone();
            move || {
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
                                let _ = bus_tx.send(server::BusEvent::NewTab);
                            }
                            _ => {}
                        }
                    }
                }
                glib::ControlFlow::Continue
            }
        });

        window.present();
        tracing::info!("sola-terminal ready");
    });

    app.run();
}
