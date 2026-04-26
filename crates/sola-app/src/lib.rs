use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::{Topic, TopicKind};

pub mod assets;
pub mod async_dispatch;
pub mod bridge;
pub mod ctx;
pub mod strip;
pub mod webview;
pub mod window;

/// Re-export of [`sola_core::watcher`] for backward compatibility.
pub use sola_core::watcher;

/// Re-export of [`sola_core::config`] for backward compatibility: the
/// traits used to live here, and downstream apps still write
/// `sola_app::config::JsonConfig`.
pub use sola_core::config;

// Re-export for macro use and common consumer paths.
pub use assets::{Asset, AssetBundle, ContentType};
pub use async_dispatch::{AppHandler, AsyncDispatcher};
pub use ctx::AppCtx;
pub use window::{WindowConfig, WindowHandle};

/// Per-topic handler registry used by `SolaApp::register_bus`. Aliases
/// `sola_bus::BusRegistry<A, AppCtx>` so downstream apps still write
/// `BusRegistry<Self>`.
pub type BusRegistry<A> = sola_bus::BusRegistry<A, AppCtx>;
/// Handler signature for `BusRegistry<A>`.
pub type BusHandler<A> = sola_bus::BusHandler<A, AppCtx>;

/// Trait implemented by every Sola app. Only `APP_ID` and `new` are
/// required; other methods have default no-op impls so apps opt in to
/// what they need.
pub trait SolaApp: 'static {
    const APP_ID: &'static str;

    /// Construct the app. This is where windows are created via
    /// `ctx.add_window`. sola-river does not receive any policy
    /// declarations from apps — the shell drives all window geometry,
    /// focus, and z-order via Frame/Focus/Composition topics.
    fn new(ctx: &mut AppCtx) -> Self
    where
        Self: Sized;

    /// Called for every raw bus message before topic parsing.
    /// Override to access message metadata (id, timestamp, sticky flags).
    /// Default: no-op.
    fn on_raw_bus_message(&mut self, _msg: &sola_bus::Message, _ctx: &mut AppCtx) {}

    /// Register per-topic handlers. The set of registered topic kinds
    /// automatically becomes this app's bus subscription.
    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx)
    where
        Self: Sized,
    {
        bus.on(TopicKind::CloseApp, Self::on_close_app);
    }

    /// Default CloseApp handler — exits the app when the incoming app_id
    /// matches `Self::APP_ID`. Apps that need pre-exit logic override
    /// `on_shutdown`, not this method.
    fn on_close_app(&mut self, topic: &Topic, ctx: &mut AppCtx)
    where
        Self: Sized,
    {
        if let Topic::CloseApp(app_id) = topic {
            if app_id == Self::APP_ID {
                self.on_shutdown(ctx);
                ctx.shutdown();
            }
        }
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
    sola_core::log::init(app_id);

    // Suppress WebKit's "Can't connect to a11y bus" warning. The WebKit
    // WebProcess looks up org.a11y.Bus on the session bus; an empty
    // WEBKIT_A11Y_BUS_ADDRESS short-circuits that. (NO_AT_BRIDGE is GTK
    // 2/3 only; GTK_A11Y=none doesn't reach WebKit's own AT-SPI module.)
    // SAFETY: single-threaded; this runs before GTK/WebKit init.
    unsafe { std::env::set_var("WEBKIT_A11Y_BUS_ADDRESS", "") };

    tracing::info!("{app_id} starting");

    /// Poll `$XDG_RUNTIME_DIR/sola-wayland` for up to 20s, falling back to
    /// `$WAYLAND_DISPLAY` and then `"wayland-0"`. The name file is preferred
    /// over the env var because sola inherits env from the user's shell,
    /// which may be stale from a prior session.
    fn resolve_wayland_display() -> String {
        for attempt in 1..=40 {
            if let Some(name) = sola_core::env::wayland_socket() {
                return name;
            }
            if attempt == 1 {
                tracing::info!("waiting for sola-river to publish wayland socket name");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if let Ok(v) = std::env::var("WAYLAND_DISPLAY") {
            if !v.is_empty() {
                tracing::warn!(name = %v, "sola-wayland name file never appeared; using WAYLAND_DISPLAY env");
                return v;
            }
        }
        tracing::error!("no wayland socket name available; defaulting to wayland-0");
        "wayland-0".to_string()
    }

    // --- Binary self-watch ---
    watcher::watch_own_binary();

    // --- Wayland socket wait ---
    //
    // sola-river writes the live socket name (e.g. `wayland-1`) to
    // `$XDG_RUNTIME_DIR/sola-wayland`. River's libwayland picks the
    // first free `wayland-N`, which isn't always `wayland-0`, so
    // hardcoding would race us against a stale-lock scenario.
    let runtime_dir = sola_core::env::runtime_dir();
    let wayland_display = resolve_wayland_display();
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &wayland_display) };
    let socket_path = runtime_dir.join(&wayland_display);
    // Verify the socket is actually there — merely setting env isn't enough.
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
    // GTK4 defaults to the Vulkan renderer, which drifts into corrupted
    // swapchain state under rapid invalidation (yellow static, endless
    // VK_SUBOPTIMAL_KHR warnings) and then stalls the compositor at
    // shutdown. GL matches WebKit6's compositing path and is stable.
    unsafe { std::env::set_var("GSK_RENDERER", "gl") };

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
        let mut app = A::new(&mut ctx);

        // --- Build BusRegistry and subscribe ---
        let mut registry: BusRegistry<A> = BusRegistry::new();
        app.register_bus(&mut registry, &mut ctx);
        // Framework-level topics — the sola-app event loop below intercepts
        // these independent of the app's registry. They must be subscribed
        // so the bus actually delivers them.
        let mut subscription_kinds = registry.kinds();
        for kind in [
            TopicKind::Shutdown,
            TopicKind::Windows,
            TopicKind::Copy,
            TopicKind::Paste,
        ] {
            if !subscription_kinds.contains(&kind) {
                subscription_kinds.push(kind);
            }
        }
        {
            let mut c = bus.borrow_mut();
            if let Err(e) = c.subscribe(&subscription_kinds) {
                tracing::warn!("failed to subscribe: {e}");
            }
        }

        // --- Wrap runtime ---
        let runtime = Rc::new(RefCell::new(AppRuntime { app, ctx }));
        let registry = Rc::new(registry);

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
            let registry = registry.clone();
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
                    // Framework-level interception. These topics are
                    // handled by the framework before the app sees them
                    // (or in addition to — apps still get registry dispatch).
                    match &topic {
                        Topic::Windows(apps) => {
                            let mut rt = runtime.borrow_mut();
                            rt.ctx.known_windows = apps.clone();
                        }
                        Topic::Copy(req) => {
                            let rt = runtime.borrow();
                            if let Some(handle) = rt.ctx.find_window_by_id(req.window_id) {
                                handle.send_to_js(&serde_json::json!({"event": "copy"}));
                            }
                        }
                        Topic::Paste(req) => {
                            // WebKit's navigator.clipboard.readText() requires
                            // a user-activation transient that host-injected
                            // JS doesn't provide, so fetch the text via
                            // GdkClipboard on the Rust side and deliver it
                            // already-resolved as part of the event payload.
                            let rt = runtime.borrow();
                            if let Some(handle) = rt.ctx.find_window_by_id(req.window_id) {
                                let handle = handle.clone();
                                if let Some(display) = gdk4::Display::default() {
                                    use gdk4::prelude::DisplayExt;
                                    let clipboard = display.clipboard();
                                    clipboard.read_text_async(
                                        None::<&gio::Cancellable>,
                                        move |result| {
                                            let text = result
                                                .ok()
                                                .flatten()
                                                .map(|s| s.to_string())
                                                .unwrap_or_default();
                                            handle.send_to_js(&serde_json::json!({
                                                "event": "paste",
                                                "text": text,
                                            }));
                                        },
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                    let mut rt = runtime.borrow_mut();
                    let AppRuntime { app, ctx } = &mut *rt;
                    registry.dispatch(&topic, app, ctx);
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

/// Inject a synchronous bootstrap that installs a queueing
/// `window.__solaRecv` before any module import has had a chance to
/// load. Without this, Rust calls into JS that race against the
/// async-loaded `ipc.ts` (which installs the real handler) — typically
/// when a sticky topic is replayed at subscribe-time and the app's
/// handler immediately calls `send_to_js`. The real handler in
/// `ipc.ts` replaces this stub on load and drains
/// `window.__solaRecvQueue`.
pub(crate) fn inject_solarecv_bootstrap(html: &str) -> String {
    const BOOTSTRAP: &str = r#"  <script>
  (function () {
    var q = [];
    window.__solaRecvQueue = q;
    window.__solaRecv = function (json) { q.push(json); };
  })();
  </script>
"#;
    if let Some(pos) = html.find("</head>") {
        let mut result = String::with_capacity(html.len() + BOOTSTRAP.len());
        result.push_str(&html[..pos]);
        result.push_str(BOOTSTRAP);
        result.push_str(&html[pos..]);
        return result;
    }
    // No </head> — fall back to before the first <script>.
    if let Some(pos) = html.find("<script") {
        let mut result = String::with_capacity(html.len() + BOOTSTRAP.len());
        result.push_str(&html[..pos]);
        result.push_str(BOOTSTRAP);
        result.push_str(&html[pos..]);
        return result;
    }
    html.to_string()
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
