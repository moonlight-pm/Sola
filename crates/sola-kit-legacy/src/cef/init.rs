//! CEF process startup. Two distinct entry points:
//!
//! - `short_circuit_if_subprocess()` — called at the very top of `main()`.
//!   If we were re-execed by CEF as a renderer/GPU/utility worker, this
//!   hands control to `CefExecuteProcess` and exits the process when
//!   that worker is done.
//! - `initialize()` — called once in the browser process to start CEF.

use std::process::ExitCode;

// `wrap_app!` and `wrap_task!` expand to code referencing bare names from
// the cef crate (App, Task, ImplApp, ImplTask, WrapApp, WrapTask, RcImpl, …).
#[allow(unused_imports)]
use cef::{rc::*, *};

// ── Custom CEF App: command-line tweaks ──────────────────────────────────────

/// Switches we add to Chromium's command line on every process. Each is
/// `(name, optional value)` — value is `None` for boolean switches.
const KIT_CHROMIUM_SWITCHES: &[(&str, Option<&str>)] = &[
    // sola is a Wayland-only desktop. Without this, Chromium defaults
    // to its X11 ozone backend, which calls `XOpenDisplay()` and panics
    // in `aura::Env::Initialize` ("Missing X server or $DISPLAY") on a
    // TTY launch where no X server is running.
    ("ozone-platform", Some("wayland")),
];

// `KitCefApp` — process-wide CEF app object. Three jobs:
//
//   1. `on_register_custom_schemes` — declare `app://` as a STANDARD +
//      SECURE + CORS + FETCH scheme. CEF requires this in *every* process
//      (browser, renderer, GPU, …) before `cef::initialize` /
//      `cef::execute_process` runs, which is why `KitCefApp::new()` is
//      passed to both. Without it Chromium treats `app://` as opaque and
//      module loading + HTML rendering misbehave — pages display as
//      text instead of rendering.
//   2. `on_before_command_line_processing` — inject Chromium flags
//      (currently `--ozone-platform=wayland`) into every subprocess.
//   3. `render_process_handler` — supply the renderer-side MessageRouter
//      so `window.cefQuery` is installed in V8 contexts. The browser
//      process never invokes this, but returning a handler
//      unconditionally is harmless and cheaper than branching on type.
//
// `app://` scheme option flags (cef_scheme_options_t):
//   STANDARD (1)  — URL has authority + path; required for module URL
//                   resolution and `<script type="module">`.
//   SECURE (8)    — treated like https for mixed-content rules; lets
//                   secure pages load app:// resources without warnings.
//   CORS_ENABLED (16) — allows CORS requests; CEF docs say this should
//                       be set in most cases where STANDARD is set.
//   FETCH_ENABLED (64) — lets fetch() target app:// URLs.
const APP_SCHEME_OPTIONS: i32 = 1 | 8 | 16 | 64; // = 89

cef::wrap_app! {
    pub struct KitCefApp {
        // Carried through every CEF process so we can inject `--class`
        // (and matching `--name`) into Chromium's command line — that's
        // how Ozone-Wayland derives the `xdg_toplevel.app_id` for
        // *Chromium*-created windows like the DevTools popup. Without
        // it, devtools shows up to sola-shell as a separate app
        // (Chromium's default class) instead of part of `<app_id>`.
        // The kit's *primary* surface sets app_id directly on its own
        // `xdg_toplevel`, so this flag only changes secondary windows.
        app_id: &'static str,
    }

    impl App {
        fn on_register_custom_schemes(
            &self,
            registrar: Option<&mut SchemeRegistrar>,
        ) {
            if let Some(reg) = registrar {
                let scheme = CefString::from("app");
                let rc = reg.add_custom_scheme(Some(&scheme), APP_SCHEME_OPTIONS);
                tracing::info!(rc, opts = APP_SCHEME_OPTIONS, "cef::on_register_custom_schemes(\"app\")");
            } else {
                tracing::warn!("cef::on_register_custom_schemes called with NULL registrar");
            }
        }

        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            if let Some(cmd) = command_line {
                for (name, value) in KIT_CHROMIUM_SWITCHES {
                    let k = CefString::from(*name);
                    match value {
                        Some(v) => {
                            let v = CefString::from(*v);
                            cmd.append_switch_with_value(Some(&k), Some(&v));
                        }
                        None => {
                            cmd.append_switch(Some(&k));
                        }
                    }
                }

                // Group secondary CEF windows (DevTools etc.) under the
                // app's xdg_toplevel.app_id so sola-shell groups them
                // with the primary surface in the switcher / menubar.
                // `--class` is the resource class half of X11 WM_CLASS
                // and is also what Ozone-Wayland uses for the
                // xdg_toplevel.app_id, so it covers both backends.
                // We deliberately do *not* set `--name` (the X11 instance
                // half) — Chromium sometimes treats it as a separate
                // semantic flag and it's not needed for app grouping.
                let class_key = CefString::from("class");
                let id_value = CefString::from(self.app_id);
                cmd.append_switch_with_value(Some(&class_key), Some(&id_value));
            }
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(crate::cef::router::KitRenderProcessHandler::new())
        }
    }
}

/// Subprocess gate — call this at the top of `main()`.
///
/// `app_id` is threaded into `KitCefApp` so subprocesses inject the
/// same `--class=<app_id>` into their command line as the browser
/// process. CEF re-execs this binary for each worker, so they all
/// hit this same gate and need the same class.
///
/// Returns `Some(ExitCode)` if the current process is a CEF worker
/// (renderer/GPU/utility/zygote); the caller should `return code` from
/// `main()` immediately. Returns `None` if this is the main browser
/// process.
pub fn short_circuit_if_subprocess(app_id: &'static str) -> Option<ExitCode> {
    // CEF 133+ requires `cef_api_hash` to be called before ANY other
    // CEF API function. It pins the API version the application was
    // compiled against; without this, struct version fields read as -1
    // and CToCpp wrappers reject them with "called with invalid
    // version -1" errors. `CEF_API_VERSION` is the experimental
    // floating tag (999999), which matches the cef-rs binding's own
    // `init_methods` layout. Subsequent calls are ignored, so it's safe
    // to invoke this in both the browser-process and subprocess paths.
    unsafe {
        cef::sys::cef_api_hash(cef::sys::CEF_API_VERSION, 0);
    }

    // `Args::new()` reads `std::env::args()` internally and holds the
    // CString/argv storage alive for the duration of this call.
    let args = cef::args::Args::new();
    let main_args = args.as_main_args();

    // Linux: no Windows sandbox — pass null.
    // Returns >= 0 if this process is a CEF worker (renderer/GPU/utility/zygote);
    // returns -1 if this is the main browser process. The `KitCefApp` is
    // passed in both paths so subprocess command-line processing also sees
    // our `--disable-features=...` injection.
    let mut app = KitCefApp::new(app_id);
    let result = cef::execute_process(Some(main_args), Some(&mut app), std::ptr::null_mut());

    if result >= 0 {
        // CEF docs: the return value is the worker's exit code.
        // Clamp to u8 range — process exit codes are 0–255 on POSIX.
        Some(ExitCode::from(result.clamp(0, 255) as u8))
    } else {
        // result == -1: we are the main browser process, continue normally.
        None
    }
}

/// Initialize CEF in the browser process. Call exactly once, after
/// `short_circuit_if_subprocess` has returned None.
///
/// `app_id` is threaded into `KitCefApp` so the browser process injects
/// `--class=<app_id>` (and matching `--name`) into Chromium's command
/// line. See `KitCefApp::on_before_command_line_processing`.
pub fn initialize(app_id: &'static str) {
    let release   = crate::cef::distribution::release_dir();
    let resources = crate::cef::distribution::resources_dir();
    let locales   = crate::cef::distribution::locales_dir();
    let exe = std::env::current_exe().expect("current_exe");

    // Per-app cache root. CEF's process-singleton lock lives in this
    // directory; if two kit apps share it, launching the second app
    // while the first is running causes CEF to forward the launch to
    // the first process ("Opening in existing browser session") and the
    // user sees a stray Chrome-style window instead of the new app.
    // Scoping by `app_id` gives each kit app its own singleton domain.
    //
    // Without any explicit `root_cache_path`, CEF would default to
    // `~/.config/cef_user_data/` and warn about "unintended process
    // singleton behavior". Owning a known sola-specific path also makes
    // recovery (kill + clear lock) straightforward.
    let cache_root = crate::cef::distribution::cef_dir()
        .join("runtime")
        .join(app_id);
    let _ = std::fs::create_dir_all(&cache_root);

    let mut settings = cef::Settings::default();
    settings.framework_dir_path    = cef::CefString::from(&*release.to_string_lossy());
    settings.resources_dir_path    = cef::CefString::from(&*resources.to_string_lossy());
    settings.locales_dir_path      = cef::CefString::from(&*locales.to_string_lossy());
    settings.browser_subprocess_path = cef::CefString::from(&*exe.to_string_lossy());
    settings.root_cache_path       = cef::CefString::from(&*cache_root.to_string_lossy());
    settings.no_sandbox                  = 1; // true — no Windows sandbox on Linux
    settings.windowless_rendering_enabled = 1; // true — OSR / off-screen rendering
    settings.external_message_pump       = 0; // false — use cef::run_message_loop
    settings.multi_threaded_message_loop = 0; // false — single main-thread loop
    // DISABLE silences Chromium's WARNING + ERROR output to stderr (FATAL
    // still surfaces, which is what we want for genuine crashes). Without
    // this, every startup spams a `dbus/object_proxy.cc` error from the
    // UPower DisplayDevice probe (NixOS doesn't run upowerd) plus assorted
    // first-run noise that drowns out our own tracing logs. Bump back to
    // WARNING (or INFO) when actively debugging Chromium internals.
    settings.log_severity = cef::LogSeverity::DISABLE;

    // `Args::new()` reads `std::env::args()` internally and keeps the
    // CString/argv storage alive for the duration of the call.
    let args = cef::args::Args::new();
    let main_args = args.as_main_args();

    // Same App as in `short_circuit_if_subprocess`, so the browser process
    // sees the same command-line flag injection as the workers.
    let mut app = KitCefApp::new(app_id);

    // CEF C-API convention: returns 1 (non-zero positive) on success, 0 on failure.
    let rc = cef::initialize(Some(main_args), Some(&settings), Some(&mut app), std::ptr::null_mut());
    if rc <= 0 {
        panic!("cef::initialize failed (return code {rc})");
    }
}

// ── Wayland event-pump task ──────────────────────────────────────────────────

// `PumpWaylandTask` — periodic CEF UI-thread task that drains the
// Wayland event queue. CEF owns the main thread via `run_message_loop`,
// so we can't have our own blocking `event_queue.dispatch_blocking(…)`
// loop. Instead we re-post ourselves every ~16 ms (matches the 60 Hz
// default frame cadence) so configure events, frame callbacks, and
// buffer-release events reach our handlers without contention with
// CEF. `Rc<RefCell<…>>` is `!Send`/`!Sync`, but the Task only ever
// runs on TID_UI which IS our main thread; the macro doesn't add
// Send/Sync bounds, so the type-checker is happy.
cef::wrap_task! {
    pub struct PumpWaylandTask {
        wayland: std::rc::Rc<std::cell::RefCell<crate::wayland::WaylandClient>>,
    }

    impl Task {
        fn execute(&self) {
            self.wayland.borrow_mut().dispatch_pending();

            // Re-post ourselves for the next tick. The Task we were
            // posted as is about to be released; we hand CEF a fresh
            // one so the loop continues. 16 ms ≈ 60 Hz.
            let mut next = PumpWaylandTask::new(self.wayland.clone());
            cef::post_delayed_task(cef::ThreadId::from(cef::sys::cef_thread_id_t::TID_UI), Some(&mut next), 16);
        }
    }
}

/// Kick off the Wayland-event-pump loop on the CEF UI thread. Call once,
/// after `cef::initialize()` returns success and before
/// `cef::run_message_loop()`.
pub fn start_wayland_pump(wayland: std::rc::Rc<std::cell::RefCell<crate::wayland::WaylandClient>>) {
    let mut task = PumpWaylandTask::new(wayland);
    cef::post_task(cef::ThreadId::from(cef::sys::cef_thread_id_t::TID_UI), Some(&mut task));
}

/// Register the `app://` custom scheme factory. Call once, immediately
/// after `initialize()` succeeds.
///
/// The scheme itself is declared as STANDARD + SECURE + CORS + FETCH in
/// `KitCefApp::on_register_custom_schemes` (which CEF invokes earlier,
/// before `cef::initialize`). This call only attaches the per-request
/// factory that turns `app://...` requests into `AssetBundle` lookups.
pub fn register_app_scheme() {
    let scheme = cef::CefString::from("app");
    // Empty domain = match all authorities under app://.
    let domain = cef::CefString::from("");
    // `AppSchemeFactory::new()` is generated by `wrap_scheme_handler_factory!`
    // and returns a `cef::SchemeHandlerFactory`.
    let mut factory = crate::cef::scheme::AppSchemeFactory::new();
    cef::register_scheme_handler_factory(Some(&scheme), Some(&domain), Some(&mut factory));
}

/// Run CEF's message loop on the current (main) thread. Blocks until
/// `cef::quit_message_loop()` is posted from any CEF thread.
pub fn run_message_loop() {
    cef::run_message_loop();
}

/// Tear down CEF cleanly. Call once after `run_message_loop` returns.
pub fn shutdown() {
    cef::shutdown();
}
