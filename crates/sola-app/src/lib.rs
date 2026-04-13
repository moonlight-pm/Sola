use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::Topic;

pub mod assets;
pub mod bridge;
pub mod strip;
pub mod watcher;
pub mod webview;

// Re-export for macro use
pub use assets::{Asset, AssetBundle, ContentType};

/// Trait for app-specific command handling.
/// Implement this to handle commands sent from the JS frontend.
#[async_trait::async_trait]
pub trait AppHandler: Send + Sync + 'static {
    async fn dispatch(&self, cmd: &str, args: &serde_json::Value) -> serde_json::Value;
}

/// Builder for configuring and running a Sola WebView app.
pub struct SolaApp {
    app_id: String,
    window_width: i32,
    window_height: i32,
    decorated: bool,
    app_assets: &'static AssetBundle,
    initial_state: Option<String>,
    handler_factory: Option<Box<dyn FnOnce(mpsc::Sender<String>) -> Box<dyn AppHandler>>>,
    bus_handler: Option<Box<dyn Fn(&Topic, &dyn Fn(serde_json::Value)) + 'static>>,
}

impl SolaApp {
    pub fn builder() -> Self {
        Self {
            app_id: "sola-app".to_string(),
            window_width: 1920,
            window_height: 1080,
            decorated: false,
            app_assets: &AssetBundle { assets: &[] },
            initial_state: None,
            handler_factory: None,
            bus_handler: None,
        }
    }

    pub fn app_id(mut self, id: &str) -> Self {
        self.app_id = id.to_string();
        self
    }

    pub fn window_size(mut self, width: i32, height: i32) -> Self {
        self.window_width = width;
        self.window_height = height;
        self
    }

    pub fn decorated(mut self, decorated: bool) -> Self {
        self.decorated = decorated;
        self
    }

    pub fn web_assets(mut self, assets: &'static AssetBundle) -> Self {
        self.app_assets = assets;
        self
    }

    pub fn initial_state(mut self, state: &str) -> Self {
        self.initial_state = Some(state.to_string());
        self
    }

    pub fn handler<H, F>(mut self, factory: F) -> Self
    where
        H: AppHandler,
        F: FnOnce(mpsc::Sender<String>) -> H + 'static,
    {
        self.handler_factory = Some(Box::new(move |tx| Box::new(factory(tx))));
        self
    }

    pub fn on_bus_event<F>(mut self, handler: F) -> Self
    where
        F: Fn(&Topic, &dyn Fn(serde_json::Value)) + 'static,
    {
        self.bus_handler = Some(Box::new(handler));
        self
    }

    pub fn run(self) {
        // Logging
        let log_dir = "/opt/sola/log";
        let _ = std::fs::create_dir_all(log_dir);
        let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| format!("{}=info", self.app_id.replace('-', "_")).into());

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

        tracing::info!("{} starting", self.app_id);

        // Watch own binary for updates (auto-restart on deploy)
        watcher::watch_own_binary();

        // Wayland socket wait
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            unsafe { std::env::set_var("WAYLAND_DISPLAY", "wayland-0") };
        }
        let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap();
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR must be set");
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

        glib::set_prgname(Some(&self.app_id));

        let app = gtk4::Application::new(None::<&str>, Default::default());

        let app_id = self.app_id;
        let window_width = self.window_width;
        let window_height = self.window_height;
        let decorated = self.decorated;
        let app_assets: &'static AssetBundle = self.app_assets;
        let initial_state = self.initial_state;
        // Wrap in RefCell since connect_activate requires Fn (not FnOnce)
        let handler_factory = RefCell::new(self.handler_factory);
        let bus_handler = RefCell::new(self.bus_handler);

        app.connect_activate(move |app| {
            // Prepare HTML with initial state
            let platform = Box::leak(Box::new(assets::platform_assets()));
            let html_raw = app_assets
                .find("/index.html")
                .map(|a| a.content.to_string())
                .unwrap_or_else(|| "<html><body>No index.html</body></html>".to_string());

            let html = if let Some(ref state_json) = initial_state {
                html_raw.replace("__RESTORED_STATE__", state_json)
            } else {
                html_raw
            };

            // Inject platform import map
            let html = inject_import_map(&html);

            // WebContext with app:/// URI scheme
            let web_context = webview::create_web_context(app_assets, platform, html);

            // Channels
            let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let (event_tx, event_rx) = mpsc::channel::<String>();

            // UserContentManager
            let ucm = webview::create_content_manager(cmd_tx);

            // Spawn tokio thread with app handler
            if let Some(factory) = handler_factory.borrow_mut().take() {
                let handler = factory(event_tx.clone());
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new()
                        .expect("failed to create tokio runtime");
                    rt.block_on(dispatch_loop(handler, cmd_rx, event_tx));
                });
            }

            // Window
            let window = gtk4::ApplicationWindow::new(app);
            window.set_decorated(decorated);
            window.set_default_size(window_width, window_height);

            // WebView
            let webview = webkit6::WebView::builder()
                .web_context(&web_context)
                .user_content_manager(&ucm)
                .build();
            if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
                settings.set_enable_developer_extras(true);
                settings.set_enable_write_console_messages_to_stdout(true);
            }
            window.set_child(Some(&webview));

            // Event poller (tokio → glib → JS)
            bridge::setup_event_poller(webview.clone(), event_rx);

            // Load app
            webview.load_uri("app:///index.html");

            // Bus connection
            let bus: Rc<RefCell<BusClient>> = Rc::new(RefCell::new(BusClient::new()));
            if let Err(e) = bus.borrow_mut().connect() {
                tracing::warn!("bus not available: {e}");
            }

            // Bus polling
            if let Some(bus_handler) = bus_handler.borrow_mut().take() {
                let webview_for_bus = webview.clone();
                glib::timeout_add_local(Duration::from_millis(50), move || {
                    let client = bus.borrow_mut();
                    while let Some(msg) = client.try_recv() {
                        let Some(topic) = Topic::parse(&msg) else { continue };
                        let send = |value: serde_json::Value| {
                            bridge::send_to_js(&webview_for_bus, &value.to_string());
                        };
                        bus_handler(&topic, &send);
                    }
                    glib::ControlFlow::Continue
                });
            }

            window.present();
            tracing::info!("{app_id} ready");
        });

        app.run();
    }
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

    // No import map found — inject one before first <script>
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

/// Command dispatch loop running on the tokio runtime.
async fn dispatch_loop(
    handler: Box<dyn AppHandler>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    event_tx: mpsc::Sender<String>,
) {
    while let Some(msg) = cmd_rx.recv().await {
        let parsed: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Invalid command JSON: {e}");
                continue;
            }
        };

        let id = parsed.get("id").and_then(|v| v.as_u64());
        let cmd = parsed.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let args = parsed.get("args").cloned().unwrap_or(serde_json::json!({}));

        let result = handler.dispatch(cmd, &args).await;

        if let Some(id) = id {
            let response = serde_json::json!({ "id": id, "result": result });
            let _ = event_tx.send(response.to_string());
        }
    }
}
