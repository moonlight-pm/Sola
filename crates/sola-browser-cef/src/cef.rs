//! CEF engine wrapper used by the main browser binary.
//!
//! Public surface mirrors `sola-browser-wpe::wpe::WpeEngine` so the
//! shader / app code on the iced side stays nearly identical.
//!
//! Modeled on `sola-kit/src/cef/{init,handlers,browser}.rs`, which
//! already runs CEF + Wayland in this repo. Differences here:
//!
//! - **CPU OSR transport** instead of dma-buf. CEF's
//!   `on_accelerated_paint` does not work on NVIDIA proprietary
//!   (see sola-kit's `cef::browser::Browser::new` doc-comment for
//!   the long story). We use `shared_texture_enabled = 0` and copy
//!   the BGRA buffer out of `on_paint` into a `Vec<u8>` per frame.
//! - **No Wayland surface dependency.** The kit binds its CEF
//!   browser to a Wayland Surface and presents through
//!   `present_paint` / `present_dmabuf`. Here we hand frames to
//!   iced via `mpsc::channel`, and iced does the GPU upload via
//!   `wgpu::Queue::write_texture` (see `cpu_import.rs`).
//! - **Worker thread runs the CEF message loop.** The main thread
//!   is owned by iced; CEF's `run_message_loop` blocks, so it
//!   lives on a dedicated thread.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

// `wrap_app!`, `wrap_render_handler!`, `wrap_client!`, `wrap_task!`
// expand to code referencing bare names from `cef::*` (App, Client,
// RenderHandler, Task, ImplApp, RcImpl, …). Wildcard import as the
// macro docs recommend.
#[allow(unused_imports)]
use cef::{rc::*, *};

/// One frame as it crosses thread boundaries. CPU OSR path: we
/// own a copy of CEF's pixel buffer, life-cycle independent of
/// CEF's frame recycle. BGRA bytes, sRGB-encoded, `width * height * 4`
/// long. `Arc` so the shader Pipeline can hold a reference across
/// the queue.write_texture and the WGPU command submission without
/// extra copies.
pub struct CefFrame {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

pub enum Cmd {
    /// Request a new viewport size. Updates the shared
    /// `Mutex<(u32,u32)>` that `RenderHandler::view_rect` reports
    /// and posts a UI-thread task that calls `host().was_resized()`
    /// so CEF re-rasterises at the new size.
    Resize { width: u32, height: u32 },
    Quit,
}

/// Engine handle held by the main thread. Owns the worker thread
/// that runs CEF's message loop, the command channel into that
/// thread, and the receive end of the frame channel.
pub struct CefEngine {
    worker: Option<JoinHandle<()>>,
    cmd_tx: Sender<Cmd>,
    frames: Arc<Mutex<Receiver<CefFrame>>>,
}

impl CefEngine {
    /// Called *before* logger init at the top of `main`. If we were
    /// re-exec'd by CEF as a worker process (renderer / GPU /
    /// utility / zygote), `cef::execute_process` handles the worker
    /// loop and returns its exit code; we propagate it. The browser
    /// process gets `None` and continues normally.
    ///
    /// `app_id` flows through `BrowserCefApp::on_before_command_line_processing`
    /// so all CEF helper windows (DevTools, etc.) report the same
    /// `xdg_toplevel.app_id` as the primary surface.
    pub fn dispatch_subprocess(app_id: &'static str) -> Option<ExitCode> {
        // CEF 133+ requires `cef_api_hash` before any other CEF call.
        // Pins the API version (experimental floating tag 999999).
        unsafe {
            cef::sys::cef_api_hash(cef::sys::CEF_API_VERSION, 0);
        }

        let args = cef::args::Args::new();
        let main_args = args.as_main_args();

        let mut app = BrowserCefApp::new(app_id);
        let result =
            cef::execute_process(Some(main_args), Some(&mut app), std::ptr::null_mut());

        if result >= 0 {
            Some(ExitCode::from(result.clamp(0, 255) as u8))
        } else {
            None
        }
    }

    /// Spawn the CEF engine. Initializes CEF, creates the OSR
    /// browser, and runs the message loop on a dedicated thread.
    pub fn spawn(app_id: &'static str, url: &str, width: u32, height: u32) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (frame_tx, frame_rx) = channel::<CefFrame>();
        let url = url.to_string();
        let worker = thread::Builder::new()
            .name("cef-engine".into())
            .spawn(move || worker_main(app_id, url, width, height, frame_tx, cmd_rx))
            .expect("spawn cef-engine thread");
        Self {
            worker: Some(worker),
            cmd_tx,
            frames: Arc::new(Mutex::new(frame_rx)),
        }
    }

    pub fn cmd_sender(&self) -> Sender<Cmd> {
        self.cmd_tx.clone()
    }

    pub fn frames(&self) -> Arc<Mutex<Receiver<CefFrame>>> {
        self.frames.clone()
    }

    pub fn shutdown(mut self) {
        let _ = self.cmd_tx.send(Cmd::Quit);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

// ──────────────────────────────────────────────────────────────────
// Worker-thread side. Everything below runs on the CEF UI thread
// (= our worker thread). The thread-local CEF_STATE makes the
// view-rect, frame-tx, and the live `cef::Browser` reachable from
// the wrapped handler / task callbacks without threading state
// through their generated structs.
// ──────────────────────────────────────────────────────────────────

struct CefThreadState {
    size: Mutex<(u32, u32)>,
    frame_tx: Sender<CefFrame>,
    cmd_rx: RefCell<Option<Receiver<Cmd>>>,
    browser: RefCell<Option<cef::Browser>>,
}

thread_local! {
    static CEF_STATE: OnceLock<Rc<CefThreadState>> = const { OnceLock::new() };
}

fn cef_state() -> Rc<CefThreadState> {
    CEF_STATE
        .with(|s| s.get().cloned())
        .expect("CEF_STATE not initialised on this thread")
}

fn worker_main(
    app_id: &'static str,
    url: String,
    width: u32,
    height: u32,
    frame_tx: Sender<CefFrame>,
    cmd_rx: Receiver<Cmd>,
) {
    let state = Rc::new(CefThreadState {
        size: Mutex::new((width, height)),
        frame_tx,
        cmd_rx: RefCell::new(Some(cmd_rx)),
        browser: RefCell::new(None),
    });
    CEF_STATE.with(|s| {
        s.set(state.clone()).map_err(|_| ()).expect("CEF_STATE set twice");
    });

    initialize_cef(app_id);

    // Create the OSR browser. `WindowInfo` flags chosen to match
    // sola-kit's CPU OSR path (NVIDIA proprietary can't do
    // `on_accelerated_paint`).
    let mut window_info = cef::WindowInfo::default();
    window_info.windowless_rendering_enabled = 1;
    window_info.external_begin_frame_enabled = 0;
    window_info.shared_texture_enabled = 0;

    let mut browser_settings = cef::BrowserSettings::default();
    browser_settings.background_color = 0xFFFF_FFFF;

    let render_handler = BrowserRenderHandler::new();
    let life_span_handler = BrowserLifeSpanHandler::new();
    let mut client = BrowserClient::new(render_handler, life_span_handler);

    let url_c = cef::CefString::from(url.as_str());
    let inner = cef::browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut client),
        Some(&url_c),
        Some(&browser_settings),
        None,
        None,
    )
    .expect("cef::browser_host_create_browser_sync returned None");
    tracing::info!(url = %url, "CEF browser created");
    *state.browser.borrow_mut() = Some(inner);

    // Recurring task that pumps the cmd_rx on the UI thread. CEF's
    // message loop owns the thread; we can't have our own select
    // loop, so we re-post ourselves every ~16 ms (≈ 60 Hz).
    let mut pump = CmdPumpTask::new();
    cef::post_delayed_task(
        cef::ThreadId::from(cef::sys::cef_thread_id_t::TID_UI),
        Some(&mut pump),
        16,
    );

    tracing::info!("CEF engine entering run_message_loop");
    cef::run_message_loop();
    tracing::info!("CEF engine run_message_loop returned");

    cef::shutdown();
}

// ── CEF App ───────────────────────────────────────────────────────

cef::wrap_app! {
    pub struct BrowserCefApp {
        app_id: &'static str,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // Force Ozone-Wayland backend on Chromium subprocesses.
            // Without this Chromium defaults to X11 ozone and panics
            // in `aura::Env::Initialize` on a Wayland-only TTY.
            if let Some(cmd) = command_line {
                let k = CefString::from("ozone-platform");
                let v = CefString::from("wayland");
                cmd.append_switch_with_value(Some(&k), Some(&v));

                // Group CEF's secondary windows (DevTools etc.)
                // under our app_id for sola-shell's switcher.
                let class_key = CefString::from("class");
                let id_value = CefString::from(self.app_id);
                cmd.append_switch_with_value(Some(&class_key), Some(&id_value));
            }
        }
    }
}

// ── RenderHandler (OSR callbacks) ─────────────────────────────────

cef::wrap_render_handler! {
    pub struct BrowserRenderHandler {}

    impl RenderHandler {
        fn view_rect(
            &self,
            _browser: Option<&mut cef::Browser>,
            rect: Option<&mut cef::Rect>,
        ) {
            let state = cef_state();
            let (w, h) = *state.size.lock().unwrap();
            if let Some(r) = rect {
                r.x = 0;
                r.y = 0;
                r.width = w as i32;
                r.height = h as i32;
            }
        }

        // Pin scale + rect to our view so Chromium doesn't auto-scale
        // OSR rasterisation against the compositor's preferred
        // fractional scale (sola-kit hit this; see its handlers.rs
        // comment for context).
        fn root_screen_rect(
            &self,
            _browser: Option<&mut cef::Browser>,
            rect: Option<&mut cef::Rect>,
        ) -> ::std::os::raw::c_int {
            let state = cef_state();
            let (w, h) = *state.size.lock().unwrap();
            if let Some(r) = rect {
                r.x = 0;
                r.y = 0;
                r.width = w as i32;
                r.height = h as i32;
            }
            1
        }

        fn screen_point(
            &self,
            _browser: Option<&mut cef::Browser>,
            view_x: ::std::os::raw::c_int,
            view_y: ::std::os::raw::c_int,
            screen_x: Option<&mut ::std::os::raw::c_int>,
            screen_y: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            if let Some(sx) = screen_x { *sx = view_x; }
            if let Some(sy) = screen_y { *sy = view_y; }
            1
        }

        fn screen_info(
            &self,
            _browser: Option<&mut cef::Browser>,
            screen_info: Option<&mut cef::ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            let state = cef_state();
            let (w, h) = *state.size.lock().unwrap();
            if let Some(si) = screen_info {
                si.device_scale_factor = 1.0;
                si.depth = 24;
                si.depth_per_component = 8;
                si.is_monochrome = 0;
                si.rect = cef::Rect { x: 0, y: 0, width: w as i32, height: h as i32 };
                si.available_rect = cef::Rect { x: 0, y: 0, width: w as i32, height: h as i32 };
            }
            1
        }

        // CPU OSR. Fires when the browser was created with
        // `shared_texture_enabled = 0`. `buffer` is valid only for
        // the duration of this call — we memcpy out.
        fn on_paint(
            &self,
            _browser: Option<&mut cef::Browser>,
            type_: cef::PaintElementType,
            _dirty_rects: Option<&[cef::Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            // Ignore popup paints (e.g. <select> dropdowns); they
            // arrive on a separate surface. We don't host them.
            if type_ != cef::PaintElementType::from(cef::sys::cef_paint_element_type_t::PET_VIEW) {
                return;
            }
            if width <= 0 || height <= 0 || buffer.is_null() {
                return;
            }
            let len = (width as usize) * (height as usize) * 4;
            let bytes = unsafe { std::slice::from_raw_parts(buffer, len) }.to_vec();
            tracing::trace!(w = width, h = height, bytes = len, "CEF on_paint");
            let frame = CefFrame {
                pixels: Arc::new(bytes),
                width: width as u32,
                height: height as u32,
            };
            let state = cef_state();
            if state.frame_tx.send(frame).is_err() {
                // Receiver dropped — iced is shutting down. Tell
                // the CEF loop to exit so we shut down cleanly.
                tracing::info!("frame channel closed, quitting CEF message loop");
                cef::quit_message_loop();
            }
        }

        // Block dma-buf path. We picked the CPU path deliberately
        // (NVIDIA can't do CEF's accelerated transport). If CEF
        // ever produces an accelerated frame from this config it's
        // a bug — log loudly and ignore.
        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut cef::Browser>,
            _type_: cef::PaintElementType,
            _dirty_rects: Option<&[cef::Rect]>,
            _info: Option<&cef::AcceleratedPaintInfo>,
        ) {
            tracing::error!(
                "on_accelerated_paint fired with shared_texture_enabled=0 — \
                 this should not happen, ignoring frame"
            );
        }
    }
}

// ── LifeSpanHandler ───────────────────────────────────────────────

cef::wrap_life_span_handler! {
    pub struct BrowserLifeSpanHandler {}

    impl LifeSpanHandler {
        fn on_before_close(&self, _browser: Option<&mut cef::Browser>) {
            // Last browser closing — drop our cached reference so
            // CEF can finish teardown, then quit the message loop.
            let state = cef_state();
            state.browser.borrow_mut().take();
            cef::quit_message_loop();
        }
    }
}

// ── Client ────────────────────────────────────────────────────────

cef::wrap_client! {
    pub struct BrowserClient {
        pub render_handler: cef::RenderHandler,
        pub life_span_handler: cef::LifeSpanHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<cef::RenderHandler> {
            Some(self.render_handler.clone())
        }

        fn life_span_handler(&self) -> Option<cef::LifeSpanHandler> {
            Some(self.life_span_handler.clone())
        }
    }
}

// ── Cmd pump task ─────────────────────────────────────────────────

cef::wrap_task! {
    pub struct CmdPumpTask {}

    impl Task {
        fn execute(&self) {
            let state = cef_state();
            // Re-borrow the receiver each tick — we own it for the
            // life of the worker thread.
            let cmd_rx_guard = state.cmd_rx.borrow();
            let Some(rx) = cmd_rx_guard.as_ref() else {
                return;
            };

            let mut should_continue = true;
            loop {
                match rx.try_recv() {
                    Ok(Cmd::Resize { width, height }) => {
                        *state.size.lock().unwrap() = (width, height);
                        if let Some(browser) = state.browser.borrow().as_ref() {
                            if let Some(host) = browser.host() {
                                host.was_resized();
                                tracing::info!(width, height, "CEF was_resized");
                            }
                        }
                    }
                    Ok(Cmd::Quit) => {
                        cef::quit_message_loop();
                        should_continue = false;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        cef::quit_message_loop();
                        should_continue = false;
                        break;
                    }
                }
            }
            drop(cmd_rx_guard);

            if should_continue {
                let mut next = CmdPumpTask::new();
                cef::post_delayed_task(
                    cef::ThreadId::from(cef::sys::cef_thread_id_t::TID_UI),
                    Some(&mut next),
                    16,
                );
            }
        }
    }
}

// ── CEF initialization (paths + Settings) ─────────────────────────

fn initialize_cef(app_id: &'static str) {
    use std::path::PathBuf;
    let cef_dir: PathBuf = PathBuf::from(env!("SOLA_BROWSER_CEF_DIR"));
    let release = cef_dir.join("Release");
    let resources = cef_dir.join("Resources");
    let locales = resources.join("locales");
    let exe = std::env::current_exe().expect("current_exe");

    // Per-app cache root, scoped by app_id so concurrent sola apps
    // don't share CEF's singleton lock (which would forward launches
    // to the running process instead of creating a fresh browser).
    let cache_root = cef_dir.join("runtime").join(app_id);
    let _ = std::fs::create_dir_all(&cache_root);

    let mut settings = cef::Settings::default();
    settings.framework_dir_path = cef::CefString::from(&*release.to_string_lossy());
    settings.resources_dir_path = cef::CefString::from(&*resources.to_string_lossy());
    settings.locales_dir_path = cef::CefString::from(&*locales.to_string_lossy());
    settings.browser_subprocess_path = cef::CefString::from(&*exe.to_string_lossy());
    settings.root_cache_path = cef::CefString::from(&*cache_root.to_string_lossy());
    settings.no_sandbox = 1;
    settings.windowless_rendering_enabled = 1;
    settings.external_message_pump = 0;
    settings.multi_threaded_message_loop = 0;
    // Silence Chromium's WARNING/ERROR stderr noise (UPower probe,
    // first-run warnings, etc.). FATAL still surfaces.
    settings.log_severity = cef::LogSeverity::DISABLE;

    let args = cef::args::Args::new();
    let main_args = args.as_main_args();
    let mut app = BrowserCefApp::new(app_id);

    let rc = cef::initialize(
        Some(main_args),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if rc <= 0 {
        panic!("cef::initialize failed (return code {rc})");
    }
    tracing::info!("CEF initialized");
}
