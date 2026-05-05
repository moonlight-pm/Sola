use sola_bus::BusClient;
use sola_bus::topics::{EvaluationPayload, Topic, TopicKind};

pub mod assets;
pub mod cef;
pub mod ctx;
pub mod strip;
pub mod wayland;
pub mod window;

/// Re-export of [`sola_core::watcher`] for backward compatibility.
pub use sola_core::watcher;

/// Re-export of [`sola_core::config`] for backward compatibility: the
/// traits used to live here, and downstream apps still write
/// `sola_kit::config::JsonConfig`.
pub use sola_core::config;

/// Re-export of [`sola_core::theme`] so apps that depend on sola-kit
/// can reach `Theme` without an additional direct dependency on sola-core.
pub use sola_core::theme;

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

    /// Hook for any post-construction setup that needs access to the
    /// runtime (e.g. attaching event controllers that dispatch into
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
/// bus callbacks can share `Rc<RefCell<_>>` and destructure into
/// disjoint `&mut` borrows.
pub struct AppRuntime<A: SolaApp> {
    pub app: A,
    pub ctx: AppCtx,
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

    // --- Binary self-watch ---
    watcher::watch_own_binary();

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
    // NOTE: AppCtx::new takes (bus, wayland, app_id) per B12's rewrite.
    // The B12 rewrite lands after B11 — so this line won't compile until B12.
    let mut ctx = AppCtx::new(bus.clone(), wayland.clone(), app_id);
    let mut app = A::new(&mut ctx);

    // --- Push the default theme CSS to every window once on init ---
    {
        let payload = serde_json::json!({
            "event": "theme",
            "css": theme_css(&sola_core::theme::Theme::default()),
        });
        for w in &ctx.windows {
            w.send_to_js(&payload);
        }
    }

    // --- Build BusRegistry and subscribe ---
    let mut registry: BusRegistry<A> = BusRegistry::new();
    app.register_bus(&mut registry, &mut ctx);
    let mut subscription_kinds = registry.kinds();
    for kind in [
        TopicKind::Shutdown,
        TopicKind::Windows,
        TopicKind::Copy,
        TopicKind::Paste,
        TopicKind::Evaluate,
        TopicKind::Theme,
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
    let runtime = std::rc::Rc::new(std::cell::RefCell::new(AppRuntime { app, ctx }));
    let registry = std::rc::Rc::new(registry);

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
                if cmd == EVALUATION_CMD {
                    let mut rt = runtime.borrow_mut();
                    emit_evaluation(args, &mut rt.ctx);
                    return;
                }
                let mut rt = runtime.borrow_mut();
                let AppRuntime { app, ctx } = &mut *rt;
                app.on_js_command(cmd, args, id, &source_for_dispatch, ctx);
            },
        );
        *source.inner.dispatcher.borrow_mut() = Some(dispatcher);
    }

    // --- after_runtime_ready hook ---
    {
        let runtime_weak = std::rc::Rc::downgrade(&runtime);
        let mut rt = runtime.borrow_mut();
        let AppRuntime { app, ctx } = &mut *rt;
        app.after_runtime_ready(runtime_weak, ctx);
    }

    // --- Bus → CEF UI thread bridge: deferred to D3/D5 ---
    spawn_bus_thread::<A>(bus.clone(), runtime.clone(), registry.clone());

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

// Bus → CEF UI thread bridge. Real implementation in D3/D5 — for B11 this
// is a no-op that compiles. The current shape borrows everything to keep
// future call-sites stable.
fn spawn_bus_thread<A: SolaApp>(
    bus: std::rc::Rc<std::cell::RefCell<BusClient>>,
    runtime: std::rc::Rc<std::cell::RefCell<AppRuntime<A>>>,
    registry: std::rc::Rc<BusRegistry<A>>,
) {
    // TODO(D3/D5): real bus polling thread + cef::post_task(UI, …) bridge.
    let _ = (bus, runtime, registry);
}


/// JS command name reserved for evaluation results. Never reaches the
/// app's `on_js_command`; the framework intercepts and emits a
/// `Topic::Evaluation`. Routing to the originating CLI is by
/// `Message::source` — the bus tags every emit with the emitting app's
/// id, so the CLI filters incoming `Evaluation` events by source.
const EVALUATION_CMD: &str = "__evaluation__";

/// Handle a `Topic::Evaluate` addressed to this process. Runs on the
/// CEF UI thread. Called from the bus loop's `Topic::Evaluate` arm —
/// wired up in D5.
#[allow(dead_code)]
fn handle_evaluate(
    app_id: &'static str,
    req: &sola_bus::topics::EvaluatePayload,
    ctx: &mut AppCtx,
) {
    let target = match &req.window {
        Some(title) => ctx.windows.iter().find(|w| w.title() == title).cloned(),
        None => ctx.windows.first().cloned(),
    };
    let Some(target) = target else {
        ctx.emit(Topic::Evaluation(EvaluationPayload {
            result: Err(format!(
                "no window matching {:?} in {app_id}",
                req.window
            )),
        }));
        return;
    };

    // Inline the expression into a wrapper that:
    //   1. Awaits the value (no-op on non-Promises).
    //   2. Catches runtime errors and JSON serialization errors.
    //   3. Posts the result back via cefQuery; the MessageRouter (D1)
    //      routes the request to the Rust handler which forwards it via
    //      the bus as `Topic::Evaluation`.
    //
    // Syntax errors in `expr` cause the wrapper to fail to parse and
    // `eval_js` silently fails; the CLI hits its timeout.
    // Acceptable for a developer tool — the user iterates.
    let wrapped = format!(
        "(async () => {{\n  let __value, __err;\n  try {{ __value = await (async () => {{ return ({expr}); }})(); }}\n  catch (e) {{ __err = String(e); }}\n  const __payload = (__err === undefined) ? {{ value: __value }} : {{ error: __err }};\n  let __body;\n  try {{ __body = JSON.stringify({{ cmd: '{cmd}', args: __payload }}); }}\n  catch (serErr) {{ __body = JSON.stringify({{ cmd: '{cmd}', args: {{ error: 'serialize: ' + String(serErr) }} }}); }}\n  window.cefQuery({{ request: __body, onSuccess: () => {{}}, onFailure: () => {{}} }});\n}})()",
        expr = req.expr,
        cmd = EVALUATION_CMD,
    );
    target.eval_js(&wrapped);
}

/// Convert an `__evaluation__` JS message into a `Topic::Evaluation`.
fn emit_evaluation(args: &serde_json::Value, ctx: &mut AppCtx) {
    let result = if let Some(err) = args.get("error").and_then(|v| v.as_str()) {
        Err(err.to_string())
    } else {
        let value = args
            .get("value")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let json = serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string());
        Ok(json)
    };
    ctx.emit(Topic::Evaluation(EvaluationPayload { result }));
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

/// Render a Theme as a `:root { ... }` CSS block.
///
/// Single source of truth is the Rust `Theme` — there is no static
/// kit.css to drift. The framework's bus loop renders the current theme
/// on every `Topic::Theme` delivery and pushes the full CSS to the JS
/// side, which swaps it into a constructable stylesheet via
/// `CSSStyleSheet.replaceSync`.
pub fn theme_css(theme: &sola_core::theme::Theme) -> String {
    use std::fmt::Write;
    let mut s = String::from(":root {\n");
    for (var, value) in theme.to_css_vars() {
        let _ = writeln!(s, "  {var}: {value};");
    }
    s.push('}');
    s.push('\n');
    s
}

/// Inject the platform import map into HTML.
pub(crate) fn inject_import_map(html: &str) -> String {
    let platform_imports = r#""preact": "/vendor/preact/preact.module.js",
      "preact/jsx-runtime": "/vendor/preact/jsxRuntime.module.js",
      "preact/hooks": "/vendor/preact/hooks.module.js",
      "@preact/signals-core": "/vendor/preact/signals-core.module.js",
      "@preact/signals": "/vendor/preact/signals.module.js",
      "@sola/ipc": "/lib/ipc.js",
      "@sola/store": "/lib/store.js",
      "@sola/kit": "/lib/kit.js",
      "~/": "/lib/""#;

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
