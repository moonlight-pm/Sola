mod agent;
mod auth;
mod bridge;
mod bus_tools;
mod mcp_config;
mod session;

use bridge::{Command, Event};
use gtk4::prelude::*;
use webkit6::prelude::*;
use session::SessionManager;
use sola_bus::BusClient;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;

const HTML: &str = include_str!("../web/index.html");

fn main() {
    // Logging: stderr + file
    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola-agent.log");

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_agent=info".into());

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

    tracing::info!("sola-agent starting");

    // Tokio runtime (lives until app exits)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    let rt_handle = rt.handle().clone();

    // Load auth
    let auth = match auth::AuthManager::load() {
        Ok(a) => Arc::new(RwLock::new(a)),
        Err(e) => {
            tracing::error!("Failed to load credentials: {:#}", e);
            eprintln!("Failed to load Claude credentials: {:#}", e);
            eprintln!("Make sure you're logged into Claude Code (~/.claude/.credentials.json)");
            std::process::exit(1);
        }
    };

    // Connect to sola-bus (optional)
    let bus_client: Option<Arc<Mutex<BusClient>>> = match BusClient::connect() {
        Ok(client) => {
            tracing::info!("Connected to sola-bus");
            Some(Arc::new(Mutex::new(client)))
        }
        Err(e) => {
            tracing::warn!("Could not connect to sola-bus: {} — bus tools disabled", e);
            None
        }
    };

    // Session manager
    let session_mgr = Arc::new(SessionManager::new());

    // GTK application
    glib::set_prgname(Some("sola-agent"));
    let app = gtk4::Application::new(None::<&str>, Default::default());

    app.connect_activate({
        let auth = auth.clone();
        let session_mgr = session_mgr.clone();
        let bus = bus_client.clone();
        let rt_handle = rt_handle.clone();
        move |app| {
            // Event channel: tokio tasks → GTK main thread
            let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
            let event_tx = Arc::new(event_tx);

            // Transparent window background
            let css = gtk4::CssProvider::new();
            css.load_from_data("window, window.background { background: transparent; }");
            gtk4::style_context_add_provider_for_display(
                &gdk4::Display::default().unwrap(),
                &css,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            let window = gtk4::ApplicationWindow::new(app);
            window.set_decorated(false);
            window.set_default_size(1400, 900);

            // WebView
            let webview = webkit6::WebView::new();
            webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));

            // JS → Rust bridge via document.title
            webview.connect_notify_local(Some("title"), {
                let auth = auth.clone();
                let session_mgr = session_mgr.clone();
                let bus = bus.clone();
                let rt_handle = rt_handle.clone();
                let event_tx = event_tx.clone();
                move |webview, _| {
                    let Some(title) = webview.title() else { return };
                    let title_str = title.to_string();
                    if title_str.is_empty() || title_str == "sola-agent" {
                        return;
                    }
                    let Ok(cmd) = serde_json::from_str::<Command>(&title_str) else {
                        return;
                    };
                    tracing::debug!(?cmd, "received command from frontend");
                    dispatch_command(
                        cmd,
                        auth.clone(),
                        session_mgr.clone(),
                        bus.clone(),
                        event_tx.clone(),
                        &rt_handle,
                    );
                }
            });

            // Poll event channel → push to WebView (60fps)
            glib::timeout_add_local(Duration::from_millis(16), {
                let webview = webview.clone();
                move || {
                    while let Ok(event) = event_rx.try_recv() {
                        bridge::dispatch_event(&webview, &event);
                    }
                    glib::ControlFlow::Continue
                }
            });

            webview.load_html(HTML, None);
            window.set_child(Some(&webview));
            window.present();
        }
    });

    app.run();
    drop(rt);
}

fn dispatch_command(
    cmd: Command,
    auth: Arc<RwLock<auth::AuthManager>>,
    session_mgr: Arc<SessionManager>,
    bus: Option<Arc<Mutex<BusClient>>>,
    event_tx: Arc<std::sync::mpsc::Sender<Event>>,
    rt_handle: &tokio::runtime::Handle,
) {
    match cmd {
        Command::NewSession { working_dir } => {
            let event_tx = event_tx.clone();
            let session_mgr = session_mgr.clone();
            rt_handle.spawn(async move {
                let dir = std::path::PathBuf::from(&working_dir);
                if !dir.is_dir() {
                    let _ = event_tx.send(Event::Error {
                        session_id: None,
                        message: format!("Not a directory: {}", working_dir),
                    });
                    return;
                }
                let session_id = session_mgr.create_session(dir).await;
                let _ = event_tx.send(Event::SessionState {
                    session_id,
                    status: "idle".into(),
                });
            });
        }

        Command::SendMessage { session_id, text } => {
            let auth = auth.clone();
            let session_mgr = session_mgr.clone();
            let bus = bus.clone();
            let event_tx = event_tx.clone();
            rt_handle.spawn(async move {
                let working_dir = {
                    let sessions = session_mgr.sessions.read().await;
                    match sessions.get(&session_id) {
                        Some(s) => s.working_dir.clone(),
                        None => {
                            let _ = event_tx.send(Event::Error {
                                session_id: Some(session_id),
                                message: "Session not found".into(),
                            });
                            return;
                        }
                    }
                };

                let cancel_token = {
                    let sessions = session_mgr.sessions.read().await;
                    sessions.get(&session_id).unwrap().cancel_token.clone()
                };

                let bus_tools: Vec<Box<dyn claurst_tools::Tool>> = match &bus {
                    Some(b) => bus_tools::create_bus_tools(b.clone()),
                    None => Vec::new(),
                };

                agent::run_session_message(
                    session_id,
                    text,
                    working_dir,
                    auth,
                    session_mgr,
                    event_tx,
                    bus_tools,
                    cancel_token,
                )
                .await;
            });
        }

        Command::Cancel { session_id } => {
            let session_mgr = session_mgr.clone();
            rt_handle.spawn(async move {
                session_mgr.cancel_session(&session_id).await;
            });
        }

        Command::CloseSession { session_id } => {
            let session_mgr = session_mgr.clone();
            rt_handle.spawn(async move {
                session_mgr.close_session(&session_id).await;
            });
        }

        Command::RenameConversation { session_id, name } => {
            let session_mgr = session_mgr.clone();
            rt_handle.spawn(async move {
                session_mgr.rename_session(&session_id, name).await;
            });
        }

        Command::ListConversations => {
            let _ = event_tx.send(Event::ConversationsList {
                conversations: Vec::new(),
            });
        }

        Command::ResumeSession { session_id: _ } => {
            let _ = event_tx.send(Event::Error {
                session_id: None,
                message: "Resume not yet implemented".into(),
            });
        }
    }
}
