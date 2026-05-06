use sola_bus::BusClient;
use sola_bus::topics::{Topic, TopicKind};

// `wrap_task!` (used below) expands to code referencing CEF traits + RcImpl
// types by bare name. We can't `use cef::*` here because the local `pub mod
// cef` shadows the external crate; reach the macro and types via `::cef::`.
#[allow(unused_imports)]
use ::cef::{rc::*, *};

pub mod assets;
pub mod cef;
pub mod ctx;
pub mod strip;
pub mod wayland;
pub mod window;

// Re-export for macro use and common consumer paths.
pub use assets::{Asset, AssetBundle, ContentType};
pub use ctx::{AppCtx, BusProxy};
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
    fn on_close_app(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx)
    where
        Self: Sized,
    {
        if let Topic::CloseApp(app_id) = delivery.topic {
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

    /// Called right before the CEF message loop quits on Topic::Shutdown.
    /// Default: ignore.
    fn on_shutdown(&mut self, ctx: &mut AppCtx) {
        let _ = ctx;
    }
}

/// Runtime container — holds the user app and its context together so
/// bus callbacks can share `Rc<RefCell<_>>` and destructure into
/// disjoint `&mut` borrows.
pub(crate) struct AppRuntime<A: SolaApp> {
    pub(crate) app: A,
    pub(crate) ctx: AppCtx,
}

/// Entry point for the trait-based API. Bootstraps logging, waits for
/// the Wayland socket, initializes CEF, connects the bus, and runs
/// `A::new` followed by the CEF message loop.
pub fn run<A: SolaApp>() {
    let app_id: &'static str = A::APP_ID;

    // --- Logging ---
    sola_core::log::init(app_id);

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
            std::thread::sleep(std::time::Duration::from_millis(500));
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

    // --- GPU driver environment ---
    // NixOS keeps GPU drivers under `/run/opengl-driver/`, but only sets
    // the relevant env vars in interactive desktop sessions. sola-kit
    // launched from a TTY inherits a shell env without them, so:
    //
    //   - `__EGL_VENDOR_LIBRARY_DIRS` points libglvnd at NixOS's
    //     `/run/opengl-driver/share/glvnd/egl_vendor.d/{10_nvidia,50_mesa}.json`
    //     so it can find vendor ICDs. Without this, libEGL.so loads but
    //     dispatches to nothing → the GPU process can't initialize Skia
    //     ("Unable to initialize SkSurface") and OnAcceleratedPaint
    //     never fires.
    //   - `LIBVA_DRIVERS_PATH` and `VK_ICD_FILENAMES` cover the analogous
    //     dispatch for VA-API and Vulkan drivers respectively.
    //
    // Setting these here (before cef::init::initialize forks subprocesses)
    // ensures every CEF worker inherits them.
    //
    // SAFETY: single-threaded at this point.
    unsafe {
        if std::env::var_os("__EGL_VENDOR_LIBRARY_DIRS").is_none() {
            std::env::set_var(
                "__EGL_VENDOR_LIBRARY_DIRS",
                "/run/opengl-driver/share/glvnd/egl_vendor.d",
            );
        }
        if std::env::var_os("LIBVA_DRIVERS_PATH").is_none() {
            std::env::set_var("LIBVA_DRIVERS_PATH", "/run/opengl-driver/lib/dri");
        }
        if std::env::var_os("VK_ICD_FILENAMES").is_none() {
            std::env::set_var(
                "VK_ICD_FILENAMES",
                "/run/opengl-driver/share/vulkan/icd.d/nvidia_icd.x86_64.json",
            );
        }
    }

    // --- Binary self-watch ---
    sola_core::watcher::watch_own_binary();

    // --- Wayland socket wait ---
    let runtime_dir = sola_core::env::runtime_dir();
    let wayland_display = resolve_wayland_display();
    // SAFETY: single-threaded; this runs before the Wayland connection.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &wayland_display) };
    let socket_path = runtime_dir.join(&wayland_display);
    for attempt in 1..=20 {
        if socket_path.exists() {
            tracing::info!(path = %socket_path.display(), "wayland socket ready");
            break;
        }
        if attempt == 20 {
            tracing::error!(path = %socket_path.display(), "wayland socket not found after 10s");
            std::process::exit(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // --- CEF initialize (browser process) ---
    cef::init::initialize();

    // --- Register app:// scheme factory ---
    // Must be called after cef::initialize and before any Browser navigates
    // to an app:// URL.
    cef::init::register_app_scheme();

    // --- Wayland connection ---
    let wayland = std::rc::Rc::new(std::cell::RefCell::new(
        wayland::WaylandClient::connect_owned(),
    ));

    // --- Bus ---
    let bus = std::rc::Rc::new(std::cell::RefCell::new(BusClient::new()));
    {
        let mut c = bus.borrow_mut();
        c.set_app_id(app_id);
        if let Err(e) = c.connect() {
            tracing::warn!("bus not available: {e}");
        }
    }

    // --- Build AppCtx, run A::new ---
    let mut ctx = AppCtx::new(bus.clone(), wayland.clone(), app_id);
    let mut app = A::new(&mut ctx);

    // --- Build BusRegistry and subscribe to the topics the app declared ---
    let mut registry: BusRegistry<A> = BusRegistry::new();
    app.register_bus(&mut registry, &mut ctx);
    {
        let mut c = bus.borrow_mut();
        if let Err(e) = c.subscribe(&registry.kinds()) {
            tracing::warn!("failed to subscribe: {e}");
        }
    }

    // --- Wrap runtime ---
    let runtime = std::rc::Rc::new(std::cell::RefCell::new(AppRuntime { app, ctx }));

    // --- Install per-window JS dispatchers ---
    let window_handles: Vec<WindowHandle> = runtime.borrow().ctx.windows.clone();
    for source in window_handles {
        let runtime_weak = std::rc::Rc::downgrade(&runtime);
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

    // --- Bus → CEF UI thread bridge ---
    // Recurring CEF UI-thread task that drains the BusClient and dispatches
    // to per-topic handlers via the registry. 16 ms tick mirrors the Wayland
    // event-pump cadence.
    start_bus_pump::<A>(bus.clone(), runtime.clone(), std::rc::Rc::new(registry));

    // --- Wayland event-pump: recurring CEF UI-thread task drains
    //     the queue every ~16 ms so configures, frame callbacks, and
    //     buffer-release events reach our handlers. ---
    cef::init::start_wayland_pump(wayland.clone());

    // --- Run CEF's main loop. Blocks until cef::quit_message_loop() is posted. ---
    tracing::info!("{app_id} entering CEF message loop");
    cef::init::run_message_loop();

    // --- Cleanup ---
    cef::init::shutdown();
    tracing::info!("{app_id} stopped");
}

// ── Bus pump ────────────────────────────────────────────────────────────────

::cef::wrap_task! {
    pub struct BusPumpTask<A: SolaApp,> {
        bus: std::rc::Rc<std::cell::RefCell<BusClient>>,
        runtime: std::rc::Rc<std::cell::RefCell<AppRuntime<A>>>,
        registry: std::rc::Rc<BusRegistry<A>>,
    }

    impl Task {
        fn execute(&self) {
            // Drain everything pending in one pass so we don't accumulate
            // backpressure across ticks.
            let messages: Vec<sola_bus::Message> = {
                let bus = self.bus.borrow();
                bus.drain_notify();
                let mut msgs = Vec::new();
                while let Some(msg) = bus.try_recv() {
                    msgs.push(msg);
                }
                msgs
            };

            for msg in messages {
                {
                    let mut rt = self.runtime.borrow_mut();
                    let AppRuntime { app, ctx } = &mut *rt;
                    app.on_raw_bus_message(&msg, ctx);
                }

                let Some(topic) = Topic::parse(&msg) else {
                    continue;
                };

                if matches!(topic, Topic::Shutdown) {
                    {
                        let mut rt = self.runtime.borrow_mut();
                        let AppRuntime { app, ctx } = &mut *rt;
                        app.on_shutdown(ctx);
                    }
                    ::cef::quit_message_loop();
                    return;
                }

                let mut rt = self.runtime.borrow_mut();
                let AppRuntime { app, ctx } = &mut *rt;
                let retracted = topic.kind().behavior().is_sticky() && !msg.sticky;
                let delivery = sola_bus::Delivery {
                    topic: &topic,
                    retracted,
                    source: &msg.source,
                };
                self.registry.dispatch(&delivery, app, ctx);
            }

            // Re-post for next tick. 16 ms ≈ 60 Hz, matches the Wayland pump.
            let mut next = BusPumpTask::<A>::new(
                self.bus.clone(),
                self.runtime.clone(),
                self.registry.clone(),
            );
            ::cef::post_delayed_task(
                ::cef::ThreadId::from(::cef::sys::cef_thread_id_t::TID_UI),
                Some(&mut next),
                16,
            );
        }
    }
}

fn start_bus_pump<A: SolaApp>(
    bus: std::rc::Rc<std::cell::RefCell<BusClient>>,
    runtime: std::rc::Rc<std::cell::RefCell<AppRuntime<A>>>,
    registry: std::rc::Rc<BusRegistry<A>>,
) {
    let mut task = BusPumpTask::<A>::new(bus, runtime, registry);
    ::cef::post_task(::cef::ThreadId::from(::cef::sys::cef_thread_id_t::TID_UI), Some(&mut task));
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

