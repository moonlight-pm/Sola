//! CEF engine — implements `sola_browser_core::Engine` for the CEF backend.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use sola_browser_core::{
    ActiveHandle, Cmd, CursorHandle, Engine, FrameReceiver, FrameSlot, NavCmd, TabId,
    TabInfo, TabsHandle, TaggedFrame,
};

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

/// CEF-specific input event. Uses CEF's integer-pixel coordinates and
/// Windows virtual-key codes — distinct from core's WPE-shaped `InputEvent`.
/// Lives here (engine-specific) rather than in core.
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
    cmd_tx: Sender<Cmd<CefEngine>>,
    /// Receiver of (tab_id, frame) tuples. iced filters by active
    /// tab before importing.
    frames: Arc<Mutex<Receiver<TaggedFrame<CefFrame>>>>,
    /// Latest CSS cursor pushed by CEF's `OnCursorChange`. Encoded
    /// as a `CursorKind` discriminant; shader's `mouse_interaction`
    /// reads this every render.
    cursor: Arc<std::sync::atomic::AtomicU32>,
    /// Snapshot of all open tabs (id/url/title). Worker rebuilds
    /// this whenever tabs are opened/closed or URL/title changes.
    tabs: Arc<Mutex<Vec<TabInfo>>>,
    /// Currently active tab id. Atomic so the iced subscription
    /// can filter frames without acquiring a mutex per frame.
    active_tab: Arc<std::sync::atomic::AtomicU64>,
    /// Monotonic counter for assigning tab ids — chrome-side, so
    /// it can mint ids before sending `Cmd::OpenTab`.
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl Engine for CefEngine {
    type Frame = CefFrame;
    type Token = ();
    type Input = InputEvent;
    type Program = crate::frame::CefProgram;

    /// CEF subprocess gate. Must run first in `main`, before logging
    /// or Wayland init. If re-exec'd by CEF as a renderer / GPU /
    /// utility / zygote worker, `cef::execute_process` handles the
    /// worker loop and returns its exit code; we propagate it. The
    /// browser process gets `None` and continues normally.
    fn dispatch_subprocess(app_id: &'static str) -> Option<ExitCode> {
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

    /// Spawn the CEF engine. Initializes CEF, opens the initial
    /// tab with `url`, runs the message loop on a dedicated thread.
    /// `browser_subprocess_path = current_exe()` so that after the
    /// dispatcher `exec`s this binary, `--type=` workers re-exec
    /// correctly.
    fn spawn(app_id: &'static str, url: &str, width: u32, height: u32) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd<CefEngine>>();
        let (frame_tx, frame_rx) = channel::<TaggedFrame<CefFrame>>();
        let cursor = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let tabs_snapshot = Arc::new(Mutex::new(Vec::<TabInfo>::new()));
        let active_atomic = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let next_id = Arc::new(std::sync::atomic::AtomicU64::new(1));

        let initial_id = TabId(next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        active_atomic.store(initial_id.0, std::sync::atomic::Ordering::Relaxed);

        // Queue: size, then open initial tab, then activate it.
        // Processed by the pump on the worker side once the
        // CEF message loop is running.
        let _ = cmd_tx.send(Cmd::Resize { width, height });
        let _ = cmd_tx.send(Cmd::OpenTab {
            id: initial_id,
            url: url.to_string(),
        });
        let _ = cmd_tx.send(Cmd::SetActiveTab(initial_id));

        let cursor_w = cursor.clone();
        let snap_w = tabs_snapshot.clone();
        let active_w = active_atomic.clone();
        let next_id_w = next_id.clone();
        let worker = thread::Builder::new()
            .name("cef-engine".into())
            .spawn(move || {
                worker_main(
                    app_id, width, height, frame_tx, cmd_rx, cursor_w, snap_w, active_w,
                    next_id_w,
                )
            })
            .expect("spawn cef-engine thread");

        Self {
            worker: Some(worker),
            cmd_tx,
            frames: Arc::new(Mutex::new(frame_rx)),
            cursor,
            tabs: tabs_snapshot,
            active_tab: active_atomic,
            next_id,
        }
    }

    fn alloc_tab_id(&self) -> TabId {
        TabId(self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    fn cmd_sender(&self) -> Sender<Cmd<CefEngine>> {
        self.cmd_tx.clone()
    }

    fn tabs_handle(&self) -> TabsHandle {
        self.tabs.clone()
    }

    fn active_tab_handle(&self) -> ActiveHandle {
        self.active_tab.clone()
    }

    fn cursor_handle(&self) -> CursorHandle {
        self.cursor.clone()
    }

    fn frames(&self) -> FrameReceiver<CefFrame> {
        self.frames.clone()
    }

    fn make_program(slot: std::sync::Arc<FrameSlot<Self>>) -> Self::Program {
        crate::frame::CefProgram { slot }
    }

    fn shutdown(mut self) {
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
    /// Most recent viewport size requested by iced. Reported back
    /// from `RenderHandler::view_rect` for every browser (all
    /// tabs share the iced widget's bounds). Updated by
    /// `Cmd::Resize` and consulted on tab switch.
    size: Mutex<(u32, u32)>,
    frame_tx: Sender<TaggedFrame<CefFrame>>,
    cmd_rx: RefCell<Option<Receiver<Cmd<CefEngine>>>>,
    /// Live tabs. Ordering is presentation order in the tab strip
    /// (iced chrome controls insert/remove positions).
    tabs: RefCell<Vec<CefTabState>>,
    /// Active tab id, also mirrored in `active_atomic` for the
    /// iced subscription's frame filter.
    active: std::cell::Cell<TabId>,
    cursor: Arc<std::sync::atomic::AtomicU32>,
    /// Snapshot of `tabs` (id/url/title) — shared with iced for
    /// rendering the tab strip + URL bar.
    tabs_snapshot: Arc<Mutex<Vec<TabInfo>>>,
    /// Shared mirror of `active` for the iced side.
    active_atomic: Arc<std::sync::atomic::AtomicU64>,
    /// Shared monotonic tab-id counter (also held chrome-side). `on_before_popup`
    /// mints a background-tab id from this on the CEF UI thread.
    next_id: Arc<std::sync::atomic::AtomicU64>,
    /// Set by on_address_change / on_title_change; checked at
    /// the next cmd-pump tick to rebuild the shared snapshot.
    snapshot_dirty: std::cell::Cell<bool>,
}

/// Per-tab state. The Browser handle outlives until close;
/// `browser_id` is `browser.identifier()` cached for fast lookup
/// on every paint / address-change callback.
struct CefTabState {
    id: TabId,
    browser_id: i32,
    browser: cef::Browser,
    url: Arc<Mutex<String>>,
    title: Arc<Mutex<String>>,
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
    width: u32,
    height: u32,
    frame_tx: Sender<TaggedFrame<CefFrame>>,
    cmd_rx: Receiver<Cmd<CefEngine>>,
    cursor: Arc<std::sync::atomic::AtomicU32>,
    tabs_snapshot: Arc<Mutex<Vec<TabInfo>>>,
    active_atomic: Arc<std::sync::atomic::AtomicU64>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
) {
    let state = Rc::new(CefThreadState {
        size: Mutex::new((width, height)),
        frame_tx,
        cmd_rx: RefCell::new(Some(cmd_rx)),
        tabs: RefCell::new(Vec::new()),
        active: std::cell::Cell::new(TabId(u64::MAX)),
        cursor,
        tabs_snapshot,
        active_atomic,
        next_id,
        snapshot_dirty: std::cell::Cell::new(false),
    });
    CEF_STATE.with(|s| {
        s.set(state.clone()).map_err(|_| ()).expect("CEF_STATE set twice");
    });

    initialize_cef(app_id);

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
            browser: Option<&mut cef::Browser>,
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
            // Identify which tab this paint belongs to. CEF gives
            // us the source Browser; we map its identifier to our
            // TabId. If the tab has been closed but a paint is
            // still in-flight, drop the frame.
            let state = cef_state();
            let Some(browser) = browser else { return };
            let Some(tab_id) = tab_by_browser_id(&state, browser.identifier()) else {
                return;
            };
            let len = (width as usize) * (height as usize) * 4;
            let bytes = unsafe { std::slice::from_raw_parts(buffer, len) }.to_vec();
            tracing::trace!(w = width, h = height, bytes = len, ?tab_id, "CEF on_paint");
            let frame = CefFrame {
                pixels: Arc::new(bytes),
                width: width as u32,
                height: height as u32,
            };
            let state = cef_state();
            if state.frame_tx.send(TaggedFrame { tab_id, frame }).is_err() {
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
        fn on_before_close(&self, browser: Option<&mut cef::Browser>) {
            // CEF tells us this browser is going away. Drop it
            // from our tab list (idempotent — the cmd-side
            // `close_tab` may have already removed it; this fires
            // even on the last close cmd). If this was the very
            // last tab, quit the message loop so the engine
            // worker exits cleanly.
            let state = cef_state();
            if let Some(b) = browser {
                let bid = b.identifier();
                let removed = {
                    let mut tabs = state.tabs.borrow_mut();
                    if let Some(pos) = tabs.iter().position(|t| t.browser_id == bid) {
                        Some(tabs.remove(pos))
                    } else {
                        None
                    }
                };
                if removed.is_some() {
                    rebuild_snapshot(&state);
                }
            }
            if state.tabs.borrow().is_empty() {
                cef::quit_message_loop();
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn on_before_popup(
            &self,
            _browser: Option<&mut cef::Browser>,
            _frame: Option<&mut cef::Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&cef::CefString>,
            _target_frame_name: Option<&cef::CefString>,
            _target_disposition: cef::WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&cef::PopupFeatures>,
            _window_info: Option<&mut cef::WindowInfo>,
            _client: Option<&mut Option<cef::Client>>,
            _settings: Option<&mut cef::BrowserSettings>,
            _extra_info: Option<&mut Option<cef::DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            // ctrl/cmd/middle-click and target=_blank all arrive here. Cancel
            // the native popup (return 1) and open the target as a background
            // tab on this same (CEF UI) thread.
            let url = target_url.map(|u| u.to_string()).unwrap_or_default();
            if url.is_empty() {
                return 1;
            }
            let state = cef_state();
            let id = TabId(state.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            open_tab(&state, id, url); // no SetActiveTab → background tab
            1 // cancel the native popup — handled as a tab.
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
            browser: Option<&mut cef::Browser>,
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
            let state = cef_state();
            let Some(browser) = browser else { return };
            let bid = browser.identifier();
            if let Some(tab) = state.tabs.borrow().iter().find(|t| t.browser_id == bid) {
                *tab.url.lock().unwrap() = s;
                state.snapshot_dirty.set(true);
            }
        }

        fn on_title_change(
            &self,
            browser: Option<&mut cef::Browser>,
            title: Option<&cef::CefString>,
        ) {
            let s = title.map(|t| t.to_string()).unwrap_or_default();
            let state = cef_state();
            let Some(browser) = browser else { return };
            let bid = browser.identifier();
            if let Some(tab) = state.tabs.borrow().iter().find(|t| t.browser_id == bid) {
                *tab.title.lock().unwrap() = s;
                state.snapshot_dirty.set(true);
            }
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

            // Drain the main Cmd channel (resize, nav, tab ops, quit).
            let cmd_rx_guard = state.cmd_rx.borrow();
            let Some(rx) = cmd_rx_guard.as_ref() else {
                return;
            };

            let mut should_continue = true;
            loop {
                match rx.try_recv() {
                    Ok(cmd) => {
                        if !process_cmd(&state, cmd) {
                            should_continue = false;
                            break;
                        }
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

            if state.snapshot_dirty.replace(false) {
                rebuild_snapshot(&state);
            }

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

/// Process one Cmd from the main channel. Returns `false` for Quit
/// (caller stops the pump); `true` otherwise. `Cmd::Input` carries
/// CEF-native input (`engine::InputEvent`) and is dispatched to the
/// active tab here — no side-channel.
fn process_cmd(state: &CefThreadState, cmd: Cmd<CefEngine>) -> bool {
    match cmd {
        Cmd::Resize { width, height } => {
            *state.size.lock().unwrap() = (width, height);
            if let Some(tab) = active_tab(state) {
                if let Some(host) = tab.browser.host() {
                    host.was_resized();
                }
            }
        }
        Cmd::Input(ev) => {
            // CEF-native input (engine::InputEvent: integer pixels,
            // Windows VK codes), delivered on the normal Cmd channel and
            // dispatched to the active tab's browser host.
            if let Some(tab) = active_tab(state) {
                if let Some(host) = tab.browser.host() {
                    dispatch_input(&host, ev);
                }
            }
        }
        Cmd::Focus(focused) => {
            if let Some(tab) = active_tab(state) {
                if let Some(host) = tab.browser.host() {
                    host.set_focus(if focused { 1 } else { 0 });
                }
            }
        }
        Cmd::Nav(nav) => {
            if let Some(tab) = active_tab(state) {
                dispatch_nav(&tab.browser, nav);
            }
        }
        Cmd::Edit(edit) => {
            if let Some(tab) = active_tab(state) {
                if let Some(frame) = tab.browser.main_frame() {
                    use sola_browser_core::engine::EditCmd;
                    match edit {
                        EditCmd::Copy => frame.copy(),
                        EditCmd::Cut => frame.cut(),
                        EditCmd::Paste => frame.paste(),
                        EditCmd::SelectAll => frame.select_all(),
                        EditCmd::Undo => frame.undo(),
                        EditCmd::Redo => frame.redo(),
                    }
                }
            }
        }
        Cmd::OpenTab { id, url } => {
            open_tab(state, id, url);
        }
        Cmd::CloseTab(id) => {
            close_tab(state, id);
        }
        Cmd::SetActiveTab(id) => {
            let exists = state.tabs.borrow().iter().any(|t| t.id == id);
            if exists {
                state.active.set(id);
                state
                    .active_atomic
                    .store(id.0, std::sync::atomic::Ordering::Relaxed);
                // Re-trigger a paint for the newly-active tab.
                // was_resized() is the cheapest forced-repaint trigger
                // CEF exposes; it asks the WebProcess for a fresh
                // composited frame at the same size, which iced
                // will receive and import.
                if let Some(tab) = active_tab(state) {
                    if let Some(host) = tab.browser.host() {
                        host.was_resized();
                    }
                }
            }
        }
        Cmd::Quit => {
            cef::quit_message_loop();
            return false;
        }
        // CEF uses CPU OSR (memcpy in on_paint) — no buffer recycle token.
        Cmd::Release { .. } => {}
    }
    true
}

/// Borrowed lookup of the active tab. Returns `None` if the
/// active id doesn't match any open tab — e.g. between
/// SetActiveTab on the iced side and the corresponding cmd
/// landing on the worker.
fn active_tab(state: &CefThreadState) -> Option<std::cell::Ref<'_, CefTabState>> {
    let active = state.active.get();
    let tabs = state.tabs.borrow();
    let idx = tabs.iter().position(|t| t.id == active)?;
    Some(std::cell::Ref::map(tabs, |v| &v[idx]))
}

/// Look up the tab that owns a given CEF Browser identifier.
/// Called from RenderHandler::on_paint and DisplayHandler
/// callbacks, both of which receive the browser and need to
/// route per-tab.
fn tab_by_browser_id(state: &CefThreadState, browser_id: i32) -> Option<TabId> {
    state
        .tabs
        .borrow()
        .iter()
        .find(|t| t.browser_id == browser_id)
        .map(|t| t.id)
}

fn open_tab(state: &CefThreadState, id: TabId, initial_url: String) {
    let mut window_info = cef::WindowInfo::default();
    window_info.windowless_rendering_enabled = 1;
    window_info.external_begin_frame_enabled = 0;
    window_info.shared_texture_enabled = 0;

    let mut browser_settings = cef::BrowserSettings::default();
    browser_settings.background_color = 0xFFFF_FFFF;
    browser_settings.windowless_frame_rate = 60;

    // One handler set per tab. Cheap — they're cef::Rc handles
    // wrapping our tiny Boxes; copying does refcount work only.
    let render_handler = BrowserRenderHandler::new();
    let life_span_handler = BrowserLifeSpanHandler::new();
    let display_handler = BrowserDisplayHandler::new();
    let mut client = BrowserClient::new(render_handler, life_span_handler, display_handler);

    let url_c = cef::CefString::from(initial_url.as_str());
    let browser = match cef::browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut client),
        Some(&url_c),
        Some(&browser_settings),
        None,
        None,
    ) {
        Some(b) => b,
        None => {
            tracing::warn!(?id, "browser_host_create_browser_sync returned None");
            return;
        }
    };
    let browser_id = browser.identifier();

    let url = Arc::new(Mutex::new(initial_url.clone()));
    let title = Arc::new(Mutex::new(String::new()));

    state.tabs.borrow_mut().push(CefTabState {
        id,
        browser_id,
        browser,
        url,
        title,
    });
    rebuild_snapshot(state);
    tracing::info!(?id, browser_id, url = %initial_url, "opened tab");
}

fn close_tab(state: &CefThreadState, id: TabId) {
    let removed = {
        let mut tabs = state.tabs.borrow_mut();
        let Some(pos) = tabs.iter().position(|t| t.id == id) else {
            return;
        };
        tabs.remove(pos)
    };
    // Ask CEF to actually close the browser. The browser handle
    // will drop with `removed` going out of scope; CEF tears down
    // the renderer process when the last reference is gone.
    if let Some(host) = removed.browser.host() {
        host.close_browser(1);
    }
    drop(removed);
    rebuild_snapshot(state);
    tracing::info!(?id, "closed tab");
}

fn rebuild_snapshot(state: &CefThreadState) {
    let new: Vec<TabInfo> = state
        .tabs
        .borrow()
        .iter()
        .map(|t| TabInfo {
            id: t.id,
            url: t.url.lock().unwrap().clone(),
            title: t.title.lock().unwrap().clone(),
        })
        .collect();
    *state.tabs_snapshot.lock().unwrap() = new;
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
