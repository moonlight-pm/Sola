use sola_bus::BusClient;
use sola_bus::topics::{Topic, TopicKind};

// `wrap_task!` (used below) expands to code referencing CEF traits + RcImpl
// types by bare name. We can't `use cef::*` here because the local `pub mod
// cef` shadows the external crate; reach the macro and types via `::cef::`.
#[allow(unused_imports)]
use ::cef::{rc::*, *};

pub mod assets;
pub mod categories;
pub mod cef;
pub mod components;
pub mod ctx;
pub mod stderr_filter;
pub mod strip;
pub mod theme;
pub mod wayland;
pub mod window;

// Re-export for macro use and common consumer paths.
pub use assets::{Asset, AssetBundle, ContentType, DirMount};
pub use ctx::{AppCtx, BusProxy};
pub use theme::kit_default_theme;
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

    /// URL path of the app's root component file under `app://`. The
    /// file must export a Remix v3 component factory named `Main`
    /// (`(handle: Handle) => RenderFn`). The kit's built-in
    /// `index.tsx` imports it via the bare specifier
    /// `@sola/app-root`, which is mapped to this path through the
    /// kit-injected importmap.
    ///
    /// Default `/main.tsx`; apps override only when their root lives
    /// elsewhere in their asset bundle.
    const ROOT_COMPONENT: &'static str = "/main.tsx";

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

    /// Called once, immediately after the runtime is wrapped in its
    /// `Rc<RefCell<…>>`, and before the bus pump and JS dispatchers are
    /// installed. The `handle` is a clonable, weak-referencing entry
    /// point into the runtime — stash it if you need to re-enter app
    /// state from a one-shot timer or other out-of-band callback.
    ///
    /// Default: no-op.
    fn after_runtime_ready(&mut self, _handle: AppRuntimeHandle<Self>, _ctx: &mut AppCtx)
    where
        Self: Sized,
    {
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

    // --- stderr filter ---
    // Install before tracing init (so the tracing stderr layer's writes
    // also flow through the saved fd) and well before `cef::initialize`
    // forks worker subprocesses (so they inherit the redirected fd 2).
    // Suppresses a stray `eprintln!` in cef-rs that fires on every
    // renderer-side MessageRouter dispatch with an empty STRING payload.
    stderr_filter::install();

    // --- Logging ---
    sola_core::log::init(app_id);

    tracing::info!("{app_id} starting");

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
        // Chromium probes GSettings (proxy config, GTK theme, locale) on
        // startup. On a minimal NixOS with no desktop session in scope,
        // GLib finds no schema files anywhere on `XDG_DATA_DIRS` and
        // logs:
        //
        //   GLib-GIO-CRITICAL: g_settings_schema_source_lookup:
        //                      assertion 'source != NULL' failed
        //   Invalid UTF-16 string
        //
        // (the UTF-16 line is GLib trying to decode a string out of the
        // NULL schema lookup result.) Forcing the in-memory backend
        // skips schema discovery entirely; Chromium's queries return
        // empty values, which is what missing schemas would have given
        // it anyway, with no behavioural change.
        if std::env::var_os("GSETTINGS_BACKEND").is_none() {
            std::env::set_var("GSETTINGS_BACKEND", "memory");
        }
    }

    // --- Binary self-watch ---
    sola_core::watcher::watch_own_binary();

    // --- Wayland socket wait ---
    //
    // `activate_wayland_session` polls $XDG_RUNTIME_DIR/sola-wayland
    // for the socket name sola-river publishes and sets
    // WAYLAND_DISPLAY so the wayland client library picks it up.
    // Same helper sola-monitor-iced and any other non-kit sola app
    // uses — the discovery logic lives in sola-core::env.
    //
    // 20s timeout covers the worst case where sola-kit launches at
    // the same instant as sola-river and beats the socket-publish.
    let runtime_dir = sola_core::env::runtime_dir();
    let wayland_display = sola_core::env::activate_wayland_session(20_000);
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
    cef::init::initialize(app_id);

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
    let mut ctx = AppCtx::new(bus.clone(), wayland.clone(), app_id, A::ROOT_COMPONENT);
    let mut app = A::new(&mut ctx);

    // --- Build BusRegistry and subscribe to the topics the app declared ---
    let mut registry: BusRegistry<A> = BusRegistry::new();
    app.register_bus(&mut registry, &mut ctx);
    {
        let mut c = bus.borrow_mut();
        // Always include `Theme` in the kit's subscription set: the bus
        // pump turns every `Topic::Theme` delivery into a `__solaRecv`
        // CSS push to all kit-managed windows, regardless of whether
        // the app registered a handler. See `BusPumpTask::execute`.
        let mut kinds = registry.kinds();
        if !kinds.contains(&TopicKind::Theme) {
            kinds.push(TopicKind::Theme);
        }
        // Force-subscribe to `Evaluate` so `solactl eval <app> <expr>`
        // reaches us even when the app didn't register a handler. The
        // bus pump intercepts `Topic::Evaluate(payload)`, runs
        // `payload.expr` in the targeted window via CEF, and emits
        // `Topic::Evaluation` with the JSON-encoded result.
        if !kinds.contains(&TopicKind::Evaluate) {
            kinds.push(TopicKind::Evaluate);
        }
        // Force-subscribe to `CloseApp` so shell-initiated shutdowns
        // reach the pump regardless of whether the app overrode
        // `register_bus` and dropped the default registration. The
        // pump intercepts `Topic::CloseApp(app_id)` matching this
        // app and posts `cef::quit_message_loop` — without this,
        // sola-session's systemd SIGTERM is the only termination
        // signal and the app drags out a ~10s wait-then-kill cycle.
        if !kinds.contains(&TopicKind::CloseApp) {
            kinds.push(TopicKind::CloseApp);
        }
        if let Err(e) = c.subscribe(&kinds) {
            tracing::warn!("failed to subscribe: {e}");
        }
    }

    // --- Wrap runtime ---
    let runtime = std::rc::Rc::new(std::cell::RefCell::new(AppRuntime { app, ctx }));

    // --- Notify app that the runtime is ready ---
    // Tight borrow scope so the JS-dispatcher loop below can also borrow.
    {
        let handle = AppRuntimeHandle { weak: std::rc::Rc::downgrade(&runtime) };
        let mut rt = runtime.borrow_mut();
        let AppRuntime { app, ctx } = &mut *rt;
        app.after_runtime_ready(handle, ctx);
    }

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

                // Kit-internal commands — handled before delegating to
                // the user's `on_js_command`. Reserved namespace
                // `__sola/*`; nothing else should use it.
                if cmd == "__sola/eval-result" {
                    let result: Result<String, String> = if let Some(ok) =
                        args.get("ok").and_then(|v| v.as_str())
                    {
                        Ok(ok.to_string())
                    } else if let Some(err) = args.get("error").and_then(|v| v.as_str()) {
                        Err(err.to_string())
                    } else {
                        Err("eval-result missing ok/error".to_string())
                    };
                    let rt = runtime.borrow();
                    rt.ctx.emit(Topic::Evaluation(
                        sola_bus::topics::EvaluationPayload { result },
                    ));
                    return;
                }

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

                // Framework-level CloseApp: if the incoming `CloseApp`
                // targets THIS app, run `on_shutdown` and post a quit
                // unconditionally — don't rely on `register_bus` having
                // wired the default handler, since apps routinely
                // override that to register their own topics and lose
                // it. Symmetric with the `Shutdown` intercept above.
                if let Topic::CloseApp(app_id) = &topic {
                    let mut rt = self.runtime.borrow_mut();
                    if app_id == rt.ctx.app_id {
                        let AppRuntime { app, ctx } = &mut *rt;
                        app.on_shutdown(ctx);
                        drop(rt);
                        ::cef::quit_message_loop();
                        return;
                    }
                }

                // Framework default for `Topic::Theme`: lower the new
                // theme to CSS *and* serialize the structured Theme,
                // pushing both to every kit-managed window via
                // `__solaRecv` in a single message. `css` is what
                // `setupKit()` adopts onto the stylesheet for live
                // theming; `definition` is the structured input
                // (palette + bindings) that theme editors read so
                // they can render one row per token with the right
                // input type.
                //
                // This runs before the user's `on_theme` handler (if
                // any) so app-level mirroring is strictly an
                // in-memory concern.
                if let Topic::Theme(theme) = &topic {
                    let css = theme.to_css();
                    let definition =
                        serde_json::to_value(theme).unwrap_or(serde_json::Value::Null);
                    let payload = serde_json::json!({
                        "event": "theme",
                        "css": css,
                        "definition": definition,
                    });
                    let rt = self.runtime.borrow();
                    for window in &rt.ctx.windows {
                        window.send_to_js(&payload);
                    }
                }

                // Framework default for `Topic::Evaluate`: if it's
                // addressed at this app, wrap `payload.expr` in a
                // self-invoking async IIFE that runs it, JSON-encodes
                // the result, and calls back through `cefQuery` with
                // `cmd: "__sola/eval-result"`. The dispatcher closure
                // installed up in `run<A>` intercepts that command and
                // emits `Topic::Evaluation`. solactl picks it up via
                // its bus subscription. No request-id correlation —
                // matches the existing solactl one-at-a-time contract.
                if let Topic::Evaluate(payload) = &topic {
                    let rt = self.runtime.borrow();
                    if payload.target_app == rt.ctx.app_id {
                        let target = match &payload.window {
                            Some(title) => rt
                                .ctx
                                .windows
                                .iter()
                                .find(|w| w.title() == title)
                                .cloned(),
                            None => rt.ctx.windows.first().cloned(),
                        };
                        if let Some(window) = target {
                            let wrapped = build_eval_wrapper(&payload.expr);
                            window.eval_js(&wrapped);
                        } else {
                            tracing::warn!(
                                target_app = %payload.target_app,
                                window = ?payload.window,
                                "Topic::Evaluate: no matching window"
                            );
                            // Send a synthetic error reply so solactl
                            // doesn't time out.
                            rt.ctx.emit(Topic::Evaluation(
                                sola_bus::topics::EvaluationPayload {
                                    result: Err(
                                        "no matching window".to_string()
                                    ),
                                },
                            ));
                        }
                    }
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

// ── AppRuntimeHandle ─────────────────────────────────────────────────────────

/// A clonable, weak handle into the kit's `AppRuntime`. Use it to
/// re-enter app state from one-shot timers or other callbacks that
/// run after the initial `SolaApp::after_runtime_ready` call returns.
///
/// Obtained via `SolaApp::after_runtime_ready`; stash a clone for
/// each callback site you need.
pub struct AppRuntimeHandle<A: SolaApp> {
    weak: std::rc::Weak<std::cell::RefCell<AppRuntime<A>>>,
}

impl<A: SolaApp> Clone for AppRuntimeHandle<A> {
    fn clone(&self) -> Self {
        Self { weak: self.weak.clone() }
    }
}

impl<A: SolaApp> AppRuntimeHandle<A> {
    /// Re-enter the runtime. Returns `true` if the runtime is still
    /// alive and `f` was called, `false` if it has been dropped.
    pub fn with(&self, f: impl FnOnce(&mut A, &mut AppCtx)) -> bool {
        let Some(rc) = self.weak.upgrade() else {
            return false;
        };
        let mut rt = rc.borrow_mut();
        let AppRuntime { app, ctx } = &mut *rt;
        f(app, ctx);
        true
    }

    /// Schedule a one-shot callback on the CEF UI thread after
    /// `delay_ms` milliseconds. The callback receives mutable access
    /// to `A` and `AppCtx` just like any other kit hook.
    ///
    /// Does nothing if the runtime has already been dropped by the
    /// time the callback fires.
    pub fn schedule_after(&self, delay_ms: u64, f: impl FnOnce(&mut A, &mut AppCtx) + 'static) {
        let mut task = ScheduledTask::<A>::new(
            self.clone(),
            std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new(f)))),
        );
        ::cef::post_delayed_task(
            ::cef::ThreadId::from(::cef::sys::cef_thread_id_t::TID_UI),
            Some(&mut task),
            delay_ms as i64,
        );
    }
}

// ── ScheduledTask ─────────────────────────────────────────────────────────────

::cef::wrap_task! {
    struct ScheduledTask<A: SolaApp,> {
        handle: AppRuntimeHandle<A>,
        // Rc<RefCell<Option<…>>> gives Clone (via Rc) + interior mutability
        // so we can take the FnOnce out of the &self execute method.
        callback: std::rc::Rc<std::cell::RefCell<Option<Box<dyn FnOnce(&mut A, &mut AppCtx)>>>>,
    }

    impl Task {
        fn execute(&self) {
            if let Some(cb) = self.callback.borrow_mut().take() {
                self.handle.with(cb);
            }
        }
    }
}

/// Synchronous bootstrap stub that installs a queueing
/// `window.__solaRecv` before any module import has had a chance to
/// load. Without this, Rust calls into JS race against the
/// async-loaded `ipc.ts` (which installs the real handler) — typically
/// when a sticky topic is replayed at subscribe-time and the app's
/// handler immediately calls `send_to_js`. The real handler in
/// `ipc.ts` replaces this stub on load and drains
/// `window.__solaRecvQueue`.
///
/// **Reuses any pre-existing queue** so a Rust→JS push that landed
/// before the parser reached this `<script>` (see
/// `WindowHandle::send_raw_json_to_js`'s self-defending wrapper) is
/// preserved into the queue this stub installs.
const BOOTSTRAP_SCRIPT: &str = r#"  <script>
  (function () {
    var q = window.__solaRecvQueue || [];
    window.__solaRecvQueue = q;
    window.__solaRecv = function (json) { q.push(json); };
  })();
  </script>
"#;

/// Build the kit's importmap with the app's root-component path
/// substituted into `@sola/app-root`. Keeps app authors out of the
/// business of mirroring kit-internal paths in their own HTML (and
/// out of sync with the kit). HTML allows only one
/// `<script type="importmap">` per document, so the kit's must be the
/// only one — apps that need additional entries should add a kit-side
/// extension hook (not yet built; pull when the second consumer
/// appears).
fn build_importmap(root_component: &str) -> String {
    format!(
        r#"  <script type="importmap">
  {{
    "imports": {{
      "@sola/ipc":                 "/lib/ipc.ts",
      "@sola/kit":                 "/lib/kit.ts",
      "@sola/badge":               "/lib/components/badge.tsx",
      "@sola/bindings-editor":     "/lib/components/bindings-editor.tsx",
      "@sola/button":              "/lib/components/button.tsx",
      "@sola/card":                "/lib/components/card.tsx",
      "@sola/color-picker":        "/lib/components/color-picker.tsx",
      "@sola/field":               "/lib/components/field.tsx",
      "@sola/font-input":          "/lib/components/font-input.tsx",
      "@sola/number-input":        "/lib/components/number-input.tsx",
      "@sola/container":           "/lib/components/container.tsx",
      "@sola/split":               "/lib/components/split.tsx",
      "@sola/popover":             "/lib/components/popover.tsx",
      "@sola/popover-select":      "/lib/components/popover-select.tsx",
      "@sola/root":                "/lib/components/root.tsx",
      "@sola/sidebar":             "/lib/components/sidebar.tsx",
      "@sola/stack":               "/lib/components/stack.tsx",
      "@sola/swatch":              "/lib/components/swatch.tsx",
      "@sola/text":                "/lib/components/text.tsx",
      "@sola/text-input":          "/lib/components/text-input.tsx",
      "@sola/token-value-editor":  "/lib/components/token-value-editor.tsx",
      "@sola/app-root":            "{root_component}",
      "@chenglou/pretext":         "/vendor/pretext/dist/layout.js",
      "@remix-run/ui":             "/vendor/remix-ui/index.ts",
      "@remix-run/ui/jsx-runtime": "/vendor/remix-ui/jsx-runtime.ts"
    }}
  }}
  </script>
"#
    )
}

/// Insert `injection` into `html` before `</head>`, falling back to
/// before the first `<script>` if the head closer is missing.
fn inject_before_head_close(html: &str, injection: &str) -> String {
    if let Some(pos) = html.find("</head>") {
        let mut result = String::with_capacity(html.len() + injection.len());
        result.push_str(&html[..pos]);
        result.push_str(injection);
        result.push_str(&html[pos..]);
        return result;
    }
    if let Some(pos) = html.find("<script") {
        let mut result = String::with_capacity(html.len() + injection.len());
        result.push_str(&html[..pos]);
        result.push_str(injection);
        result.push_str(&html[pos..]);
        return result;
    }
    html.to_string()
}

/// Build the `<link rel="stylesheet">` block for every `Css` asset in the
/// given bundles, in order. Walks both `bundle.assets` (per-file
/// `include_bytes!` entries) and `bundle.dirs` (whole-tree
/// `include_dir!` mounts) so app bundles using either layout get their
/// stylesheets auto-linked. Apps no longer enumerate component
/// stylesheets in their own `index.html` — adding a `.css` file to the
/// relevant bundle makes it appear in every kit app automatically.
///
/// Bundles are walked in the order supplied; with `platform_assets()`
/// first and the app's bundle second, app CSS naturally cascades over
/// kit defaults.
fn kit_css_links(bundles: &[&assets::AssetBundle]) -> String {
    let mut out = String::new();
    for bundle in bundles {
        for asset in bundle.assets {
            if asset.content_type == assets::ContentType::Css {
                push_link(&mut out, asset.path);
            }
        }
        for mount in bundle.dirs {
            walk_dir_for_css(&mut out, mount.url_prefix, mount.dir);
        }
    }
    out
}

fn push_link(out: &mut String, href: &str) {
    out.push_str("  <link rel=\"stylesheet\" href=\"");
    out.push_str(href);
    out.push_str("\" />\n");
}

/// Recursively walk an `include_dir::Dir` tree, emitting a `<link>` for
/// every file with a `.css` extension. `url_prefix` is the mount's
/// public URL prefix; file paths come from `include_dir` relative to
/// the tree root, so concatenation yields the same URL the asset
/// server resolves via `find_in_dir`.
fn walk_dir_for_css(
    out: &mut String,
    url_prefix: &'static str,
    dir: &'static include_dir::Dir<'static>,
) {
    for file in dir.files() {
        let path = file.path().to_string_lossy();
        if path.ends_with(".css") {
            let href: &'static str = Box::leak(
                format!("{url_prefix}{path}").into_boxed_str(),
            );
            push_link(out, href);
        }
    }
    for subdir in dir.dirs() {
        walk_dir_for_css(out, url_prefix, subdir);
    }
}

/// Wrap a user-supplied JS expression in an async IIFE that runs it,
/// JSON-encodes the result, and calls back via `cefQuery` with
/// `cmd: "__sola/eval-result"`. The dispatcher closure in `run<A>`
/// intercepts that command and emits `Topic::Evaluation`.
///
/// `expr` should be a single JavaScript expression (not a statement
/// list). Object literals need their own parens — `{a: 1}` parses as a
/// block; pass `({a: 1})` instead. Promises are awaited.
fn build_eval_wrapper(expr: &str) -> String {
    format!(
        r#"
(async () => {{
  function send(payload) {{
    if (!window.cefQuery) {{ return; }}
    window.cefQuery({{
      request: JSON.stringify({{ cmd: '__sola/eval-result', args: payload }}),
      onSuccess: function () {{}},
      onFailure: function () {{}},
    }});
  }}
  try {{
    const r = await (async () => ({expr}))();
    const json = JSON.stringify(r === undefined ? null : r);
    send({{ ok: json }});
  }} catch (e) {{
    send({{ error: String(e && e.message ? e.message : e) }});
  }}
}})();
"#,
        expr = expr,
    )
}

/// Inject the kit's importmap, auto-discovered component stylesheets,
/// and the `__solaRecv` bootstrap into the app's index.html.
///
/// Order is intentional:
///   1. `window.__solaInitial` — serialised seed JSON (or `null`)
///      injected as an inline `<script>` so it is available
///      synchronously when `index.tsx` evaluates.
///   2. Importmap — installed before any module loads so bare
///      specifiers resolve.
///   3. Stylesheet `<link>`s — kit CSS first, app CSS second so app
///      rules cascade over kit defaults. The browser begins fetching
///      both while the JS bootstrap runs.
///   4. `__solaRecv` queueing stub — installed before any
///      `<script type="module">` runs so Rust→JS pushes that race
///      module loading don't drop on the floor.
///
/// Note: `serde_json::to_string` output is used as-is for the inline
/// script value. Apps are responsible for ensuring their seed data
/// does not contain literal `</script>` sequences.
pub(crate) fn inject_kit_head(
    html: &str,
    root_component: &str,
    app_assets: &assets::AssetBundle,
    initial_state: Option<&serde_json::Value>,
) -> String {
    let initial_json = match initial_state {
        Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    };
    let initial_script = format!("<script>window.__solaInitial = {};</script>", initial_json);
    let with_initial = inject_before_head_close(html, &initial_script);
    let importmap = build_importmap(root_component);
    let with_map = inject_before_head_close(&with_initial, &importmap);
    let links = kit_css_links(&[assets::platform_assets(), app_assets]);
    let with_css = inject_before_head_close(&with_map, &links);
    inject_before_head_close(&with_css, BOOTSTRAP_SCRIPT)
}

#[cfg(test)]
mod importmap_tests {
    use super::build_importmap;

    /// The importmap is JSON-ish text formatted with column alignment.
    /// Strip every space and newline so assertions don't break the next
    /// time a longer key is added and the alignment column shifts.
    fn normalize(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    #[test]
    fn importmap_resolves_app_root_to_given_path() {
        let im = normalize(&build_importmap("/menubar.tsx"));
        assert!(
            im.contains(r#""@sola/app-root":"/menubar.tsx""#),
            "expected @sola/app-root mapped to /menubar.tsx; full importmap:\n{}",
            build_importmap("/menubar.tsx")
        );
    }

    #[test]
    fn importmap_resolves_a_different_path() {
        let im = normalize(&build_importmap("/main.tsx"));
        assert!(
            im.contains(r#""@sola/app-root":"/main.tsx""#),
            "expected @sola/app-root mapped to /main.tsx; full importmap:\n{}",
            build_importmap("/main.tsx")
        );
    }
}

