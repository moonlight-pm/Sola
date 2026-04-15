use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk4::prelude::*;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedReceiver;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::Topic;

pub mod assets;
pub mod bridge;
pub mod config;

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
///
/// NOTE: This is the legacy builder API. New apps should implement the
/// `SolaApp` trait and use `sola_app::run::<A>()` instead. This type is
/// scheduled for removal once all apps have migrated.
pub struct SolaAppBuilder {
    app_id: String,
    window_width: i32,
    window_height: i32,
    decorated: bool,
    transparent: bool,
    app_assets: &'static AssetBundle,
    initial_state: Option<String>,
    handler_factory: Option<Box<dyn FnOnce(mpsc::Sender<String>) -> Box<dyn AppHandler>>>,
    bus_handler: Option<Box<dyn Fn(&Topic, &dyn Fn(serde_json::Value), &dyn Fn(Topic)) + 'static>>,
    on_activate_callback: Option<
        Box<
            dyn FnOnce(&gtk4::ApplicationWindow, &webkit6::WebView, Rc<RefCell<BusClient>>)
                + 'static,
        >,
    >,
    js_command_handler: Option<Box<dyn Fn(&str, &serde_json::Value) + 'static>>,
}

impl Default for SolaAppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SolaAppBuilder {
    pub fn new() -> Self {
        Self {
            app_id: "sola-app".to_string(),
            window_width: 1920,
            window_height: 1080,
            decorated: false,
            transparent: false,
            app_assets: &AssetBundle { assets: &[] },
            initial_state: None,
            handler_factory: None,
            bus_handler: None,
            on_activate_callback: None,
            js_command_handler: None,
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

    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
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

    pub fn on_js_command<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str, &serde_json::Value) + 'static,
    {
        self.js_command_handler = Some(Box::new(handler));
        self
    }

    pub fn on_bus_event<F>(mut self, handler: F) -> Self
    where
        F: Fn(&Topic, &dyn Fn(serde_json::Value), &dyn Fn(Topic)) + 'static,
    {
        self.bus_handler = Some(Box::new(handler));
        self
    }

    pub fn on_activate<F>(mut self, callback: F) -> Self
    where
        F: FnOnce(&gtk4::ApplicationWindow, &webkit6::WebView, Rc<RefCell<BusClient>>) + 'static,
    {
        self.on_activate_callback = Some(Box::new(callback));
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

        unsafe { std::env::set_var("GDK_BACKEND", "wayland") };
        unsafe { std::env::set_var("GTK_A11Y", "none") };

        glib::set_prgname(Some(&self.app_id));

        let app = gtk4::Application::new(None::<&str>, Default::default());

        let app_id = self.app_id;
        let window_width = self.window_width;
        let window_height = self.window_height;
        let decorated = self.decorated;
        let transparent = self.transparent;
        let app_assets: &'static AssetBundle = self.app_assets;
        let initial_state = self.initial_state;
        // Wrap in RefCell since connect_activate requires Fn (not FnOnce)
        let handler_factory = RefCell::new(self.handler_factory);
        let bus_handler = RefCell::new(self.bus_handler);
        let on_activate_callback = RefCell::new(self.on_activate_callback);
        let js_command_handler = RefCell::new(self.js_command_handler);

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

            let (event_tx, event_rx) = mpsc::channel::<String>();

            // UserContentManager + command dispatch
            let ucm = if let Some(js_handler) = js_command_handler.borrow_mut().take() {
                webview::create_content_manager_with_handler(js_handler)
            } else {
                let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let ucm = webview::create_content_manager(cmd_tx);

                if let Some(factory) = handler_factory.borrow_mut().take() {
                    let handler = factory(event_tx.clone());
                    std::thread::spawn(move || {
                        let rt = Runtime::new().expect("failed to create tokio runtime");
                        rt.block_on(dispatch_loop(handler, cmd_rx, event_tx));
                    });
                }

                ucm
            };

            // Transparent window background
            if transparent {
                let css = gtk4::CssProvider::new();
                css.load_from_data("window, window.background { background: transparent; }");
                gtk4::style_context_add_provider_for_display(
                    &gdk4::Display::default().unwrap(),
                    &css,
                    gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
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
            // Suppress WebKit's default right-click menu. In a Wayland session
            // without xdg-desktop-portal it can hang opening the popup, which
            // looks to the user as a frozen terminal on right-click.
            webview.connect_context_menu(|_, _, _| true);
            if transparent {
                webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
            }
            window.set_child(Some(&webview));

            // Event poller (tokio → glib → JS)
            bridge::setup_event_poller(webview.clone(), event_rx);

            // Load app
            webview.load_uri("app:///index.html");

            // Bus connection
            let bus: Rc<RefCell<BusClient>> = Rc::new(RefCell::new(BusClient::new()));
            {
                let mut client = bus.borrow_mut();
                client.set_app_id(&app_id);
                if let Err(e) = client.connect() {
                    tracing::warn!("bus not available: {e}");
                }
            }

            // Clone before bus_handler moves its copy into the fd callback
            let bus_for_activate = bus.clone();

            // Bus event source — fires when bus messages arrive on the socket
            if let Some(bus_handler) = bus_handler.borrow_mut().take() {
                let webview_for_bus = webview.clone();
                let notify_fd = bus.borrow().notify_fd();
                if let Some(fd) = notify_fd {
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
                            let send = |value: serde_json::Value| {
                                bridge::send_to_js(&webview_for_bus, &value.to_string());
                            };
                            let emit = |topic: Topic| {
                                let _ = bus.borrow_mut().emit(topic);
                            };
                            bus_handler(&topic, &send, &emit);
                        }
                        glib::ControlFlow::Continue
                    });
                }
            }

            // App-specific post-setup
            if let Some(callback) = on_activate_callback.borrow_mut().take() {
                callback(&window, &webview, bus_for_activate);
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
    mut cmd_rx: UnboundedReceiver<String>,
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
