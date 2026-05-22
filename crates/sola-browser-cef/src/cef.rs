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
    /// Forward a user input event into CEF. Translated to the
    /// appropriate `BrowserHost::send_*_event` call on the CEF
    /// UI thread.
    Input(InputEvent),
    /// Toggle CEF focus on the OSR view. iced delivers keyboard
    /// events only to focused widgets, but CEF tracks focus
    /// independently and needs an explicit `set_focus` call.
    Focus(bool),
    /// Navigation operation (back/forward/reload/etc). Dispatched
    /// on the CEF UI thread to `browser.go_back()` etc.
    Nav(NavCmd),
    Quit,
}

/// Navigation actions the chrome triggers. 1:1 with CEF browser
/// API calls; same enum shape as sola-browser-wpe's NavCmd so the
/// chrome can be engine-agnostic.
#[derive(Debug, Clone)]
pub enum NavCmd {
    Back,
    Forward,
    Reload,
    Stop,
    LoadUrl(String),
}

/// Thread-safe input event shape. Mirrors the layout in
/// sola-browser-wpe but uses CEF's coordinate + modifier
/// conventions (integer pixels, `EVENTFLAG_*` bits).
#[derive(Debug, Clone)]
pub enum InputEvent {
    PointerMove { x: i32, y: i32, modifiers: u32 },
    PointerButton {
        down: bool,
        x: i32,
        y: i32,
        button: u32, /* 1=L, 2=M, 3=R — see input::button_to_modifier */
        modifiers: u32,
    },
    Scroll {
        x: i32,
        y: i32,
        delta_x: i32,
        delta_y: i32,
        precise: bool,
        modifiers: u32,
    },
    /// Keyboard event. `vk` is the Windows-style virtual-key code
    /// (per Chromium `keyboard_codes.h`), `character` is the
    /// post-shift Unicode codepoint to send as a CHAR event for
    /// printable input (None for non-printable keys like arrows
    /// or function keys).
    Key {
        down: bool,
        vk: u32,
        character: Option<u16>,
        modifiers: u32,
    },
}

/// Engine handle held by the main thread. Owns the worker thread
/// that runs CEF's message loop, the command channel into that
/// thread, and the receive end of the frame channel.
pub struct CefEngine {
    worker: Option<JoinHandle<()>>,
    cmd_tx: Sender<Cmd>,
    frames: Arc<Mutex<Receiver<CefFrame>>>,
    /// Latest CSS cursor pushed by CEF's `OnCursorChange`. Encoded
    /// as a `CursorKind` discriminant; shader's `mouse_interaction`
    /// reads this every render. Worker thread writes.
    cursor: Arc<std::sync::atomic::AtomicU32>,
    /// Current page URL. Updated on the worker thread when CEF
    /// fires `DisplayHandler::on_address_change`. Read by the
    /// chrome to populate the URL bar.
    url: Arc<Mutex<String>>,
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
        let cursor = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cursor_worker = cursor.clone();
        let url_state = Arc::new(Mutex::new(url.to_string()));
        let url_worker = url_state.clone();
        let url_owned = url.to_string();
        let worker = thread::Builder::new()
            .name("cef-engine".into())
            .spawn(move || {
                worker_main(
                    app_id,
                    url_owned,
                    width,
                    height,
                    frame_tx,
                    cmd_rx,
                    cursor_worker,
                    url_worker,
                )
            })
            .expect("spawn cef-engine thread");
        Self {
            worker: Some(worker),
            cmd_tx,
            frames: Arc::new(Mutex::new(frame_rx)),
            cursor,
            url: url_state,
        }
    }

    /// Shared handle to the current page URL. Updated on
    /// CEF's `on_address_change`; safe to read from any thread.
    pub fn url_handle(&self) -> Arc<Mutex<String>> {
        self.url.clone()
    }

    /// Shared handle to the latest cursor shape. Non-blocking read,
    /// safe to call from iced's render thread.
    pub fn cursor_handle(&self) -> Arc<std::sync::atomic::AtomicU32> {
        self.cursor.clone()
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
    cursor: Arc<std::sync::atomic::AtomicU32>,
    url: Arc<Mutex<String>>,
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
    cursor: Arc<std::sync::atomic::AtomicU32>,
    url_state: Arc<Mutex<String>>,
) {
    let state = Rc::new(CefThreadState {
        size: Mutex::new((width, height)),
        frame_tx,
        cmd_rx: RefCell::new(Some(cmd_rx)),
        browser: RefCell::new(None),
        cursor,
        url: url_state,
    });
    CEF_STATE.with(|s| {
        s.set(state.clone()).map_err(|_| ()).expect("CEF_STATE set twice");
    });

    initialize_cef(app_id);

    let mut window_info = cef::WindowInfo::default();
    window_info.windowless_rendering_enabled = 1;
    window_info.external_begin_frame_enabled = 0;
    window_info.shared_texture_enabled = 0;

    let mut browser_settings = cef::BrowserSettings::default();
    browser_settings.background_color = 0xFFFF_FFFF;
    browser_settings.windowless_frame_rate = 60;

    let render_handler = BrowserRenderHandler::new();
    let life_span_handler = BrowserLifeSpanHandler::new();
    let display_handler = BrowserDisplayHandler::new();
    let mut client = BrowserClient::new(render_handler, life_span_handler, display_handler);

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

// ── DisplayHandler (cursor + console / status) ────────────────────

cef::wrap_display_handler! {
    pub struct BrowserDisplayHandler {}

    impl DisplayHandler {
        fn on_cursor_change(
            &self,
            _browser: Option<&mut cef::Browser>,
            _cursor: cef::sys::cef_cursor_handle_t,
            type_: cef::CursorType,
            _custom_cursor_info: Option<&cef::CursorInfo>,
        ) -> ::std::os::raw::c_int {
            let kind = crate::input::cef_cursor_to_kind(type_);
            cef_state().cursor.store(
                kind as u32,
                std::sync::atomic::Ordering::Relaxed,
            );
            // 1 = we handled it. CEF won't try to set a native-window
            // cursor (there's none in OSR anyway).
            1
        }

        fn on_address_change(
            &self,
            _browser: Option<&mut cef::Browser>,
            frame: Option<&mut cef::Frame>,
            url: Option<&cef::CefString>,
        ) {
            // Only main-frame navigations update the address bar.
            // Sub-frames (iframes) fire this too but their URL
            // shouldn't drive the chrome.
            let is_main = frame
                .as_ref()
                .map(|f| f.is_main() != 0)
                .unwrap_or(false);
            if !is_main {
                return;
            }
            let s = url.map(|u| u.to_string()).unwrap_or_default();
            *cef_state().url.lock().unwrap() = s;
        }
    }
}

// ── Client ────────────────────────────────────────────────────────

cef::wrap_client! {
    pub struct BrowserClient {
        pub render_handler: cef::RenderHandler,
        pub life_span_handler: cef::LifeSpanHandler,
        pub display_handler: cef::DisplayHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<cef::RenderHandler> {
            Some(self.render_handler.clone())
        }

        fn life_span_handler(&self) -> Option<cef::LifeSpanHandler> {
            Some(self.life_span_handler.clone())
        }

        fn display_handler(&self) -> Option<cef::DisplayHandler> {
            Some(self.display_handler.clone())
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
                    Ok(Cmd::Input(ev)) => {
                        if let Some(browser) = state.browser.borrow().as_ref() {
                            if let Some(host) = browser.host() {
                                dispatch_input(&host, ev);
                            }
                        }
                    }
                    Ok(Cmd::Focus(focused)) => {
                        if let Some(browser) = state.browser.borrow().as_ref() {
                            if let Some(host) = browser.host() {
                                host.set_focus(if focused { 1 } else { 0 });
                            }
                        }
                    }
                    Ok(Cmd::Nav(nav)) => {
                        if let Some(browser) = state.browser.borrow().as_ref() {
                            dispatch_nav(browser, nav);
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

// ── Input dispatch (runs on CEF UI thread via CmdPumpTask) ────────

/// Materialize an `InputEvent` as CEF `MouseEvent` / `KeyEvent`
/// and hand it to the browser host. Per CEF's docs a key press
/// is three events: RAWKEYDOWN → optional CHAR (for printable
/// input) → KEYUP. KeyEvent::default() sets `size` correctly so
/// CEF accepts it.
fn dispatch_input(host: &cef::BrowserHost, ev: InputEvent) {
    use cef::{KeyEvent, KeyEventType, MouseButtonType, MouseEvent};
    match ev {
        InputEvent::PointerMove { x, y, modifiers } => {
            let me = MouseEvent { x, y, modifiers };
            host.send_mouse_move_event(Some(&me), 0);
        }
        InputEvent::PointerButton { down, x, y, button, modifiers } => {
            let me = MouseEvent { x, y, modifiers };
            let bt = match button {
                1 => MouseButtonType::LEFT,
                2 => MouseButtonType::MIDDLE,
                3 => MouseButtonType::RIGHT,
                _ => return,
            };
            // CEF wants click_count > 0 (typically 1 for single,
            // 2 for double); it does its own double-click detection
            // by time/position. 1 is a safe default.
            host.send_mouse_click_event(Some(&me), bt, if down { 0 } else { 1 }, 1);
        }
        InputEvent::Scroll { x, y, delta_x, delta_y, precise, modifiers } => {
            let mut me = MouseEvent { x, y, modifiers };
            if precise {
                me.modifiers |= cef::sys::cef_event_flags_t::EVENTFLAG_PRECISION_SCROLLING_DELTA.0;
            }
            host.send_mouse_wheel_event(Some(&me), delta_x, delta_y);
        }
        InputEvent::Key { down, vk, character, modifiers } => {
            // RAWKEYDOWN / KEYUP carry the VK code. CHAR carries
            // the produced text character (post-shift). For
            // printable input we send RAWKEYDOWN then CHAR on the
            // way down; for non-printable (arrows, function keys)
            // we send only RAWKEYDOWN/KEYUP.
            let mut ke = KeyEvent::default();
            ke.modifiers = modifiers;
            ke.windows_key_code = vk as i32;
            ke.native_key_code = 0;
            ke.is_system_key = 0;
            if down {
                ke.type_ = KeyEventType::RAWKEYDOWN;
                host.send_key_event(Some(&ke));
                if let Some(ch) = character {
                    let mut char_ev = ke.clone();
                    char_ev.type_ = KeyEventType::CHAR;
                    char_ev.character = ch;
                    char_ev.unmodified_character = ch;
                    host.send_key_event(Some(&char_ev));
                }
            } else {
                ke.type_ = KeyEventType::KEYUP;
                host.send_key_event(Some(&ke));
            }
        }
    }
}

/// Dispatch a `NavCmd` to the live CEF browser. Runs on the CEF
/// UI thread (called from the cmd pump).
fn dispatch_nav(browser: &cef::Browser, nav: NavCmd) {
    match nav {
        NavCmd::Back => browser.go_back(),
        NavCmd::Forward => browser.go_forward(),
        NavCmd::Reload => browser.reload(),
        NavCmd::Stop => browser.stop_load(),
        NavCmd::LoadUrl(url) => {
            if let Some(frame) = browser.main_frame() {
                let url_c = cef::CefString::from(url.as_str());
                frame.load_url(Some(&url_c));
                tracing::info!(url = %url, "Nav::LoadUrl");
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
