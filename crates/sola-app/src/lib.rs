use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::Topic;

pub mod assets;
pub mod async_dispatch;
pub mod bridge;
pub mod config;
pub mod ctx;
pub mod strip;
pub mod watcher;
pub mod webview;
pub mod window;

// Re-export for macro use and common consumer paths.
pub use assets::{Asset, AssetBundle, ContentType};
pub use async_dispatch::{AppHandler, AsyncDispatcher};
pub use ctx::AppCtx;
pub use window::{WindowConfig, WindowHandle};

/// Trait implemented by every Sola app. Only `APP_ID` and `new` are
/// required; other methods have default no-op impls so apps opt in to
/// what they need.
pub trait SolaApp: 'static {
    const APP_ID: &'static str;

    /// Construct the app. This is where windows are created via
    /// `ctx.add_window`; they get auto-declared to the compositor via
    /// `SetWindowPolicy` after `new` returns.
    fn new(ctx: &mut AppCtx) -> Self
    where
        Self: Sized;

    /// Called for every raw bus message before topic parsing.
    /// Override to access message metadata (id, timestamp, sticky flags).
    /// Default: no-op.
    fn on_raw_bus_message(&mut self, _msg: &sola_bus::Message, _ctx: &mut AppCtx) {}

    /// Dispatch a bus event. Default: ignore.
    fn on_bus_event(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let _ = (topic, ctx);
    }

    /// Dispatch a JS command from a specific window. Default: ignore.
    ///
    /// `id` is the request id provided by `@sola/ipc`'s `invoke()` — pass it
    /// back in the reply envelope (`{"id": ..., "result": ...}`) so the JS
    /// `invoke` promise resolves. `None` means the command was fire-and-forget.
    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &serde_json::Value,
        id: Option<u64>,
        source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        let _ = (cmd, args, id, source, ctx);
    }

    /// Called right before GTK quits on Topic::Shutdown. Default: ignore.
    fn on_shutdown(&mut self, ctx: &mut AppCtx) {
        let _ = ctx;
    }

    /// Hook for any post-construction setup that needs access to the
    /// runtime (e.g. attaching GTK event controllers that dispatch into
    /// self via the runtime). Default: ignore.
    #[allow(unused_variables)]
    fn after_runtime_ready(
        &mut self,
        runtime: std::rc::Weak<std::cell::RefCell<AppRuntime<Self>>>,
        ctx: &mut AppCtx,
    ) where
        Self: Sized,
    {
    }
}

/// Runtime container — holds the user app and its context together so
/// GTK / bus callbacks can share `Rc<RefCell<_>>` and destructure into
/// disjoint `&mut` borrows.
pub struct AppRuntime<A: SolaApp> {
    pub app: A,
    pub ctx: AppCtx,
}

/// Entry point for the trait-based API. Bootstraps logging, waits for
/// the Wayland socket, starts the GTK app, connects the bus, and runs
/// `A::new` followed by the event loop.
pub fn run<A: SolaApp>() {
    let app_id: &'static str = A::APP_ID;

    // --- Logging ---
    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("{}=info", app_id.replace('-', "_")).into());

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

    tracing::info!("{app_id} starting");

    // --- Binary self-watch ---
    watcher::watch_own_binary();

    // --- Wayland socket wait ---
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
            tracing::error!(path = %socket_path.display(), "wayland socket not found after 10s");
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    unsafe { std::env::set_var("GDK_BACKEND", "wayland") };
    unsafe { std::env::set_var("GTK_A11Y", "none") };

    glib::set_prgname(Some(app_id));

    let gtk_app = gtk4::Application::new(None::<&str>, Default::default());

    gtk_app.connect_activate(move |gtk_app| {
        // --- Bus ---
        let bus = Rc::new(RefCell::new(BusClient::new()));
        {
            let mut c = bus.borrow_mut();
            c.set_app_id(app_id);
            if let Err(e) = c.connect() {
                tracing::warn!("bus not available: {e}");
            }
        }

        // --- Build AppCtx, run A::new ---
        let mut ctx = AppCtx::new(bus.clone(), gtk_app.clone(), app_id);
        let app = A::new(&mut ctx);

        // --- Auto-emit WindowPolicy for all windows created in new() ---
        ctx.emit_window_policy();

        // --- Wrap runtime ---
        let runtime = Rc::new(RefCell::new(AppRuntime { app, ctx }));

        // --- Install per-window JS dispatchers ---
        let window_handles: Vec<WindowHandle> = runtime.borrow().ctx.windows.clone();
        for source in window_handles {
            let runtime_weak = Rc::downgrade(&runtime);
            let source_for_dispatch = source.clone();
            let dispatcher: window::JsDispatcher = Box::new(
                move |cmd: &str, args: &serde_json::Value, id: Option<u64>| {
                    let Some(runtime) = runtime_weak.upgrade() else {
                        return;
                    };
                    let mut rt = runtime.borrow_mut();
                    let AppRuntime { app, ctx } = &mut *rt;
                    app.on_js_command(cmd, args, id, &source_for_dispatch, ctx);
                },
            );
            *source.inner.dispatcher.borrow_mut() = Some(dispatcher);
        }

        // --- after_runtime_ready hook ---
        let runtime_weak = Rc::downgrade(&runtime);
        {
            let mut rt = runtime.borrow_mut();
            let AppRuntime { app, ctx } = &mut *rt;
            app.after_runtime_ready(runtime_weak, ctx);
        }

        // --- Bus event loop ---
        let notify_fd = bus.borrow().notify_fd();
        if let Some(fd) = notify_fd {
            let runtime = runtime.clone();
            let gtk_app = gtk_app.clone();
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
                    {
                        let mut rt = runtime.borrow_mut();
                        let AppRuntime { app, ctx } = &mut *rt;
                        app.on_raw_bus_message(&msg, ctx);
                    }
                    let Some(topic) = Topic::parse(&msg) else {
                        continue;
                    };
                    if matches!(topic, Topic::Shutdown) {
                        {
                            let mut rt = runtime.borrow_mut();
                            let AppRuntime { app, ctx } = &mut *rt;
                            app.on_shutdown(ctx);
                        }
                        gtk_app.quit();
                        return glib::ControlFlow::Continue;
                    }
                    let mut rt = runtime.borrow_mut();
                    let AppRuntime { app, ctx } = &mut *rt;
                    app.on_bus_event(&topic, ctx);
                }
                glib::ControlFlow::Continue
            });
        } else {
            tracing::warn!("bus notify_fd unavailable; no bus events will be delivered");
        }

        tracing::info!("{app_id} ready");
    });

    gtk_app.run();
}

/// Inject the platform import map into HTML.
pub(crate) fn inject_import_map(html: &str) -> String {
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
