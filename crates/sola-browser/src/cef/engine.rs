//! CEF engine — implements `crate::engine::Engine` for the CEF backend.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;

use crate::cef::paint::{self, DirtyRect, PixelRing};

use crate::engine::{
    ActiveHandle, ClipboardHandle, Cmd, CursorHandle, DownloadsHandle, Engine, FrameReceiver,
    FrameSlot, NavCmd, PasskeysHandle, TabId, TabInfo, TabsHandle, TaggedFrame,
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
/// extra copies. Cheap to clone (shared pixels) for tab-switch park.
#[derive(Clone)]
pub struct CefFrame {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// Empty = full buffer is new. Otherwise chrome may upload only these.
    pub dirty: Vec<crate::cef::paint::DirtyRect>,
}

/// CEF-specific input event. Uses CEF's integer-pixel coordinates and
/// Windows virtual-key codes — distinct from core's WPE-shaped `InputEvent`.
/// Lives here (engine-specific) rather than in core.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum InputEvent {
    PointerMove {
        x: i32,
        y: i32,
        modifiers: u32,
    },
    PointerButton {
        down: bool,
        x: i32,
        y: i32,
        button: u32, /* 1=L, 2=M, 3=R — see input::button_to_modifier */
        modifiers: u32,
        /// 1 = single, 2 = double, 3 = triple. OSR does not infer this.
        #[serde(default = "default_click_count")]
        click_count: u32,
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
    /// Begin or update an IME composition (OSR `ImeSetComposition`).
    ImeSetComposition {
        text: String,
        /// UTF-16 selection start inside `text`.
        selection_from: u32,
        /// UTF-16 selection end inside `text`.
        selection_to: u32,
    },
    /// Commit composed text (OSR `ImeCommitText`).
    ImeCommit {
        text: String,
    },
    /// Cancel the current composition (`ImeCancelComposition`).
    ImeCancel,
    /// Pointer left the OSR view (`send_mouse_move_event` with mouse_leave).
    PointerLeave {
        x: i32,
        y: i32,
        modifiers: u32,
    },
}

fn default_click_count() -> u32 {
    1
}

/// Engine handle held by the main thread. Owns the worker thread
/// that runs CEF's message loop, the command channel into that
/// thread, and the receive end of the frame channel.
pub struct CefEngine {
    worker: Option<JoinHandle<()>>,
    cmd_tx: Sender<Cmd<CefEngine>>,
    /// Latest-wins frame mailbox. iced filters by paint tab before importing.
    frames: FrameReceiver<CefFrame>,
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
    /// Page-copy handoff (see [`ClipboardHandle`]). Unused for now — CEF
    /// page copy still goes through Chromium's own clipboard via
    /// `frame.copy()`; kept so the chrome's drain-on-Tick is engine-agnostic
    /// and a future selection bridge can fill it.
    clipboard_out: ClipboardHandle,
    ime: crate::engine::ImeHandle,
    downloads: DownloadsHandle,
    passkeys: PasskeysHandle,
}

impl Engine for CefEngine {
    type Frame = CefFrame;
    type Token = ();
    type Input = InputEvent;
    type Program = crate::cef::frame::CefProgram;

    fn frame_size(frame: &Self::Frame) -> (u32, u32) {
        (frame.width, frame.height)
    }

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
        let mut app = BrowserCefApp::new(app_id, BrowserRenderProcessHandler::new());
        let result = cef::execute_process(Some(main_args), Some(&mut app), std::ptr::null_mut());
        if result >= 0 {
            Some(ExitCode::from(result.clamp(0, 255) as u8))
        } else {
            None
        }
    }

    /// Spawn the chrome-side router. CEF itself runs in per-profile
    /// helper processes (`--engine`) so this iced process maps one window.
    fn spawn(app_id: &'static str, url: &str, width: u32, height: u32) -> Self {
        let handles = super::router::spawn_router(app_id, width, height);
        let cmd_tx = handles.cmd_tx.clone();
        if !url.is_empty() {
            let initial_id = TabId(
                handles
                    .next_id
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            );
            handles
                .active
                .store(initial_id.0, std::sync::atomic::Ordering::Relaxed);
            let _ = cmd_tx.send(Cmd::OpenTab {
                id: initial_id,
                url: url.to_string(),
                title: String::new(),
            });
            let _ = cmd_tx.send(Cmd::SetActiveTab(initial_id));
        }

        Self {
            worker: Some(handles.worker),
            cmd_tx,
            frames: handles.frames,
            cursor: handles.cursor,
            tabs: handles.tabs,
            active_tab: handles.active,
            next_id: handles.next_id,
            clipboard_out: handles.clipboard,
            ime: handles.ime,
            downloads: handles.downloads,
            passkeys: handles.passkeys,
        }
    }

    fn alloc_tab_id(&self) -> TabId {
        TabId(
            self.next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        )
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

    fn clipboard_handle(&self) -> ClipboardHandle {
        self.clipboard_out.clone()
    }

    fn ime_handle(&self) -> crate::engine::ImeHandle {
        self.ime.clone()
    }

    fn downloads_handle(&self) -> DownloadsHandle {
        self.downloads.clone()
    }

    fn passkeys_handle(&self) -> PasskeysHandle {
        self.passkeys.clone()
    }

    fn frames(&self) -> FrameReceiver<CefFrame> {
        self.frames.clone()
    }

    fn make_program(slot: std::sync::Arc<FrameSlot<Self>>) -> Self::Program {
        crate::cef::frame::CefProgram { slot }
    }

    fn shutdown(&mut self) {
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
    frames: FrameReceiver<CefFrame>,
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
    /// Live profile request context (cookies/storage). Unused when CEF
    /// root_cache_path is the active profile (global context).
    request_context: RefCell<Option<cef::RequestContext>>,
    /// Live profile id (for parking under the right key).
    live_profile_id: RefCell<String>,
    /// Parked profile workspaces — CEF browsers kept alive so switching
    /// back does not reload pages. Evicted by [`crate::tab_cache`] policy.
    /// Cleared on CEF recycle (profile-as-root cannot keep two user-data dirs).
    parked: RefCell<std::collections::HashMap<String, ParkedWorkspace>>,
    /// Leftover from a failed in-process CEF recycle (cannot re-init).
    #[allow(dead_code)]
    recycle: std::cell::Cell<bool>,
    #[allow(dead_code)]
    pending_recycle: RefCell<Option<PendingRecycle>>,
    /// Optional chrome IPC (headless helper). Ready + tab snapshots.
    ipc_events: Option<std::sync::mpsc::Sender<crate::cef::ipc::FromEngine>>,
    /// This helper is the painted profile. Parked helpers stay `false` so
    /// CEF stops compositing them.
    is_front: Cell<bool>,
    /// Cmds posted from the waiter thread; drained on the CEF UI thread.
    pending_cmds: Arc<Mutex<Vec<Cmd<CefEngine>>>>,
    drain_posted: Arc<AtomicBool>,
    shutting_down: Cell<bool>,
    /// Live CEF download cancel callbacks, keyed by download id.
    download_cbs: RefCell<std::collections::HashMap<u32, cef::DownloadItemCallback>>,
    /// Last emitted (monotonic_ms, percent) per download — throttle Progress.
    download_last: RefCell<std::collections::HashMap<u32, (u64, i32)>>,
}

/// Tabs to recreate after CEF recycle (new profile as root_cache_path).
#[allow(dead_code)]
struct PendingRecycle {
    profile_id: String,
    tabs: Vec<(TabId, String, String)>,
    active: TabId,
}

/// One profile's parked CEF state (hidden browsers + context).
struct ParkedWorkspace {
    tabs: Vec<CefTabState>,
    request_context: Option<cef::RequestContext>,
    #[allow(dead_code)]
    last_used: std::time::Instant,
    tab_count: usize,
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
    is_loading: Cell<bool>,
    can_go_back: Cell<bool>,
    can_go_forward: Cell<bool>,
    load_progress: Cell<f32>,
    /// Last CPU OSR frame for this tab. Replayed on `SetActiveTab` so
    /// chrome can paint immediately while CEF invalidates for a fresh
    /// frame (static pages often never re-`on_paint` without that).
    last_frame: RefCell<Option<CefFrame>>,
    /// Recycled pixel buffers so `on_paint` does not allocate 8 MiB/frame.
    paint_bufs: RefCell<PixelRing>,
    /// `<select>` / date-picker OSR popup (PET_POPUP). Blitted onto VIEW.
    popup: RefCell<OsrPopup>,
}

#[derive(Default)]
struct OsrPopup {
    visible: bool,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    pixels: Vec<u8>,
}

thread_local! {
    static CEF_STATE: OnceLock<Rc<CefThreadState>> = const { OnceLock::new() };
}

fn cef_state() -> Rc<CefThreadState> {
    CEF_STATE
        .with(|s| s.get().cloned())
        .expect("CEF_STATE not initialised on this thread")
}

pub(super) fn run_worker(
    app_id: &'static str,
    width: u32,
    height: u32,
    frames: FrameReceiver<CefFrame>,
    cmd_rx: Receiver<Cmd<CefEngine>>,
    cursor: Arc<std::sync::atomic::AtomicU32>,
    tabs_snapshot: Arc<Mutex<Vec<TabInfo>>>,
    active_atomic: Arc<std::sync::atomic::AtomicU64>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
    ipc_events: Option<std::sync::mpsc::Sender<crate::cef::ipc::FromEngine>>,
) {
    let state = Rc::new(CefThreadState {
        size: Mutex::new((width, height)),
        frames,
        cmd_rx: RefCell::new(Some(cmd_rx)),
        tabs: RefCell::new(Vec::new()),
        active: std::cell::Cell::new(TabId(u64::MAX)),
        cursor,
        tabs_snapshot,
        active_atomic,
        next_id,
        snapshot_dirty: std::cell::Cell::new(false),
        request_context: RefCell::new(None),
        live_profile_id: RefCell::new(String::new()),
        parked: RefCell::new(std::collections::HashMap::new()),
        recycle: std::cell::Cell::new(false),
        pending_recycle: RefCell::new(None),
        ipc_events,
        is_front: Cell::new(false),
        pending_cmds: Arc::new(Mutex::new(Vec::new())),
        drain_posted: Arc::new(AtomicBool::new(false)),
        shutting_down: Cell::new(false),
        download_cbs: RefCell::new(std::collections::HashMap::new()),
        download_last: RefCell::new(std::collections::HashMap::new()),
    });
    CEF_STATE.with(|s| {
        s.set(state.clone())
            .map_err(|_| ())
            .expect("CEF_STATE set twice");
    });

    initialize_cef(app_id);
    *state.request_context.borrow_mut() = None;
    *state.live_profile_id.borrow_mut() = crate::profiles::active().id;
    if let Some(tx) = &state.ipc_events {
        let _ = tx.send(crate::cef::ipc::FromEngine::Ready {
            tabs: Vec::new(),
            active: 0,
        });
    }

    let cmd_rx = state
        .cmd_rx
        .borrow_mut()
        .take()
        .expect("cmd_rx already taken");
    let pending = state.pending_cmds.clone();
    let drain_posted = state.drain_posted.clone();
    std::thread::Builder::new()
        .name("cef-cmd-wait".into())
        .spawn(move || cmd_waiter(cmd_rx, pending, drain_posted))
        .expect("spawn cef-cmd-wait");

    let mut flush = CookieFlushTask::new();
    cef::post_delayed_task(
        cef::ThreadId::from(cef::sys::cef_thread_id_t::TID_UI),
        Some(&mut flush),
        10_000,
    );

    tracing::info!("CEF engine entering run_message_loop");
    cef::run_message_loop();
    tracing::info!("CEF engine run_message_loop returned");

    teardown_all_browsers(&state);
    flush_all_cookie_stores(&state);
    cef::shutdown();
}

fn teardown_all_browsers(state: &CefThreadState) {
    hide_all_tabs(state);
    let live = std::mem::take(&mut *state.tabs.borrow_mut());
    for t in live {
        if let Some(host) = t.browser.host() {
            host.close_browser(1);
        }
    }
    let parked = std::mem::take(&mut *state.parked.borrow_mut());
    for (_id, park) in parked {
        destroy_parked(park);
    }
    *state.request_context.borrow_mut() = None;
}

// ── CEF App ───────────────────────────────────────────────────────

cef::wrap_render_process_handler! {
    pub struct BrowserRenderProcessHandler {}

    impl RenderProcessHandler {
        fn on_context_created(
            &self,
            _browser: Option<&mut cef::Browser>,
            frame: Option<&mut cef::Frame>,
            _context: Option<&mut cef::V8Context>,
        ) {
            // Document-start: run before page scripts can snapshot
            // navigator.credentials.get (Gemini Exchange does this).
            #[cfg(feature = "bitwarden")]
            inject_webauthn_frame(frame);
        }
    }
}

cef::wrap_app! {
    pub struct BrowserCefApp {
        app_id: &'static str,
        render_process_handler: cef::RenderProcessHandler,
    }

    impl App {
        fn render_process_handler(&self) -> Option<cef::RenderProcessHandler> {
            Some(self.render_process_handler.clone())
        }

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

                // Durable cookie encryption without a desktop keyring.
                // Sola runs from a bare TTY — libsecret/kwallet/portal are
                // unavailable, so Chromium's default OSCrypt leaves no
                // reusable key in Local State. Cookies still write to disk
                // (v10 encrypted) but cannot be decrypted after restart →
                // YouTube/Google look signed-out every launch. `basic` uses
                // Chromium's fixed obfuscation backend so auth cookies
                // round-trip. (Not a secret-store substitute; profile dir
                // permissions are the boundary.)
                let pw_key = CefString::from("password-store");
                let pw_val = CefString::from("basic");
                cmd.append_switch_with_value(Some(&pw_key), Some(&pw_val));
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

        fn on_popup_show(
            &self,
            browser: Option<&mut cef::Browser>,
            show: ::std::os::raw::c_int,
        ) {
            let state = cef_state();
            let Some(browser) = browser else { return };
            let Some(tab_id) = tab_by_browser_id(&state, browser.identifier()) else {
                return;
            };
            let Some(tab) = tab_state_by_id(&state, tab_id) else {
                return;
            };
            let mut popup = tab.popup.borrow_mut();
            if show == 0 {
                popup.visible = false;
                popup.pixels.clear();
                popup.w = 0;
                popup.h = 0;
                drop(popup);
                if let Some(host) = browser.host() {
                    host.invalidate(cef::PaintElementType::VIEW);
                }
            } else {
                popup.visible = true;
            }
        }

        fn on_popup_size(
            &self,
            browser: Option<&mut cef::Browser>,
            rect: Option<&cef::Rect>,
        ) {
            let Some(rect) = rect else { return };
            let state = cef_state();
            let Some(browser) = browser else { return };
            let Some(tab_id) = tab_by_browser_id(&state, browser.identifier()) else {
                return;
            };
            let Some(tab) = tab_state_by_id(&state, tab_id) else {
                return;
            };
            let mut popup = tab.popup.borrow_mut();
            popup.x = rect.x;
            popup.y = rect.y;
            popup.w = rect.width.max(0) as u32;
            popup.h = rect.height.max(0) as u32;
        }

        // CPU OSR. Fires when the browser was created with
        // `shared_texture_enabled = 0`. `buffer` is valid only for
        // the duration of this call — we memcpy out.
        fn on_paint(
            &self,
            browser: Option<&mut cef::Browser>,
            type_: cef::PaintElementType,
            dirty_rects: Option<&[cef::Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            if width <= 0 || height <= 0 || buffer.is_null() {
                return;
            }
            let is_popup = type_.get_raw() == cef::PaintElementType::POPUP.get_raw();
            let is_view = type_.get_raw() == cef::PaintElementType::VIEW.get_raw();
            if !is_view && !is_popup {
                return;
            }
            let state = cef_state();
            // Parked profile, or a background tab CEF still composited:
            // do not memcpy 8 MiB on the CEF UI thread (that stalls input).
            if !state.is_front.get() {
                return;
            }
            let Some(browser) = browser else { return };
            let Some(tab_id) = tab_by_browser_id(&state, browser.identifier()) else {
                return;
            };
            if state.active.get() != tab_id {
                return;
            }
            let w = width as u32;
            let h = height as u32;
            let len = (w as usize) * (h as usize) * 4;
            let src = unsafe { std::slice::from_raw_parts(buffer, len) };
            let dirty = dirty_from_cef(dirty_rects, w, h);
            let Some(tab) = tab_state_by_id(&state, tab_id) else {
                return;
            };
            if is_popup {
                publish_popup_paint(&state, &tab, src, w, h, dirty);
            } else {
                publish_view_paint(&state, &tab, src, w, h, dirty);
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

        fn on_ime_composition_range_changed(
            &self,
            _browser: Option<&mut cef::Browser>,
            _selected_range: Option<&cef::Range>,
            character_bounds: Option<&[cef::Rect]>,
        ) {
            // First glyph box is the caret / candidate-window anchor.
            let Some(bounds) = character_bounds.and_then(|b| b.first().cloned()) else {
                return;
            };
            let state = cef_state();
            if let Some(tx) = &state.ipc_events {
                let _ = tx.send(crate::cef::ipc::FromEngine::ImeCaret {
                    x: bounds.x,
                    y: bounds.y,
                    w: bounds.width.max(1),
                    h: bounds.height.max(1),
                });
            }
        }

        fn on_virtual_keyboard_requested(
            &self,
            _browser: Option<&mut cef::Browser>,
            input_mode: cef::TextInputMode,
        ) {
            // Desktop IME is enabled whenever the page owns keys (chrome
            // side). This callback is only used to drop a stale caret when
            // Chromium says there is no text field.
            if *input_mode.as_ref() == cef::sys::cef_text_input_mode_t::CEF_TEXT_INPUT_MODE_NONE {
                let state = cef_state();
                if let Some(tx) = &state.ipc_events {
                    let _ = tx.send(crate::cef::ipc::FromEngine::ImeCaret {
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                    });
                }
            }
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
            // Do NOT quit the message loop when the tab list empties —
            // chrome enforces ≥1 tab and may open a blank replacement.
            // Only Cmd::Quit / channel disconnect should stop the loop.
            if state.tabs.borrow().is_empty() {
                tracing::debug!("cef tab list empty (waiting for chrome to open a tab)");
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
            // ctrl/⌘-click and target=_blank arrive here (Chromium's Linux
            // new-tab disposition keys off CONTROL; ⌘ is mapped to CONTROL for
            // mouse events in `input::modifiers_to_cef_mouse`). Cancel the
            // native popup (return 1) and open the target as a background tab
            // on this same (CEF UI) thread.
            let url = target_url.map(|u| u.to_string()).unwrap_or_default();
            tracing::info!(
                url = %url,
                disposition = ?_target_disposition,
                user_gesture = _user_gesture,
                "on_before_popup fired"
            );
            if url.is_empty() {
                return 1;
            }
            let state = cef_state();
            let id = TabId(state.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            open_tab(&state, id, url, String::new()); // no SetActiveTab → background tab
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
            let kind = crate::cef::input::cef_cursor_to_kind(type_);
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
            if set_tab_url_title_by_browser_id(&state, bid, Some(s), None) {
                state.snapshot_dirty.set(true);
            }
        }

        fn on_loading_progress_change(
            &self,
            browser: Option<&mut cef::Browser>,
            progress: f64,
        ) {
            let state = cef_state();
            let Some(browser) = browser else { return };
            let bid = browser.identifier();
            if set_tab_load_progress_by_browser_id(&state, bid, progress as f32) {
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
            if set_tab_url_title_by_browser_id(&state, bid, None, Some(s)) {
                state.snapshot_dirty.set(true);
            }
        }

        fn on_console_message(
            &self,
            _browser: Option<&mut cef::Browser>,
            _level: cef::LogSeverity,
            message: Option<&cef::CefString>,
            _source: Option<&cef::CefString>,
            _line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let msg = message.map(|m| m.to_string()).unwrap_or_default();
            if let Some(rest) = msg.strip_prefix(crate::paste_js::COPY_PREFIX) {
                let text = crate::paste_js::parse_js_json_string(rest);
                tracing::info!(len = text.len(), "page copy selection extracted");
                if let Some(tx) = &cef_state().ipc_events {
                    let _ = tx.send(crate::cef::ipc::FromEngine::Clipboard(text));
                }
                return 1;
            }
            #[cfg(feature = "bitwarden")]
            {
                if let Some(origin) = msg.strip_prefix("__sola_webauthn_installed__") {
                    tracing::info!(origin = %origin, "webauthn hook installed in frame");
                    return 1;
                }
                const PREFIX: &str = "__sola_webauthn__";
                if let Some(rest) = msg.strip_prefix(PREFIX) {
                    // Credential assembly breadcrumb from the polyfill.
                    if let Some(detail) = rest.strip_prefix("_cred__") {
                        tracing::info!(%detail, "webauthn page credential assembled");
                        return 1;
                    }
                    handle_webauthn_payload(rest);
                    return 1; // suppress console noise
                }
                if let Some(detail) = msg.strip_prefix("__sola_webauthn_cred__") {
                    tracing::info!(detail = %detail.trim(), "webauthn page credential assembled");
                    return 1;
                }
                if let Some(detail) = msg.strip_prefix("__sola_webauthn_resolve_err__") {
                    tracing::warn!(detail = %detail.trim(), "webauthn page resolve failed");
                    return 1;
                }
                if let Some(rest) = msg.strip_prefix("__sola_vault_fill__:") {
                    let found = rest.trim().starts_with('1');
                    crate::vault::passkey_bridge::push_fill_result(found);
                    return 1;
                }
            }
            0
        }
    }
}

// ── LoadHandler (inject WebAuthn intercept when bitwarden enabled) ─

cef::wrap_load_handler! {
    pub struct BrowserLoadHandler {}

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            browser: Option<&mut cef::Browser>,
            is_loading: ::std::os::raw::c_int,
            can_go_back: ::std::os::raw::c_int,
            can_go_forward: ::std::os::raw::c_int,
        ) {
            let state = cef_state();
            let Some(browser) = browser else { return };
            let bid = browser.identifier();
            if set_tab_nav_state_by_browser_id(
                &state,
                bid,
                is_loading != 0,
                can_go_back != 0,
                can_go_forward != 0,
            ) {
                state.snapshot_dirty.set(true);
            }
        }

        fn on_load_start(
            &self,
            _browser: Option<&mut cef::Browser>,
            frame: Option<&mut cef::Frame>,
            _transition_type: cef::TransitionType,
        ) {
            #[cfg(feature = "bitwarden")]
            inject_webauthn_frame(frame);
        }

        fn on_load_end(
            &self,
            _browser: Option<&mut cef::Browser>,
            frame: Option<&mut cef::Frame>,
            _http_status_code: ::std::os::raw::c_int,
        ) {
            #[cfg(feature = "bitwarden")]
            inject_webauthn_frame(frame);
        }
    }
}

#[cfg(feature = "bitwarden")]
fn inject_webauthn_frame(frame: Option<&mut cef::Frame>) {
    let Some(frame) = frame else { return };
    // Every frame — Google sign-in (accounts.google.com iframe) and
    // Gemini Exchange 2FA both call WebAuthn off the top-level page.
    let url = cef_string_userfree_display(&frame.url());
    tracing::info!(%url, main = frame.is_main() != 0, "webauthn: inject frame");
    let code: cef::CefString = crate::vault::inject_webauthn_intercept_script().into();
    frame.execute_java_script(Some(&code), None, 0);
}

#[cfg(feature = "bitwarden")]
fn handle_webauthn_payload(raw: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        tracing::warn!(payload = %raw, "webauthn: payload not json");
        return;
    };
    let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
    let action = v
        .get("action")
        .and_then(|x| x.as_str())
        .unwrap_or("get")
        .to_string();
    let origin = v
        .get("origin")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let rp_id = v
        .get("rpId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let public_key_json = v
        .get("publicKey")
        .cloned()
        .map(|pk| pk.to_string())
        .unwrap_or_else(|| "{}".into());
    // One emit per (id, origin). Console + leftover beacon used to
    // fan the same click into chrome several times.
    thread_local! {
        static LAST: std::cell::RefCell<Option<(u64, String)>> =
            std::cell::RefCell::new(None);
    }
    let dup = LAST.with(|last| {
        let mut last = last.borrow_mut();
        if last.as_ref().is_some_and(|(i, o)| *i == id && *o == origin) {
            true
        } else {
            *last = Some((id, origin.clone()));
            false
        }
    });
    if dup {
        tracing::debug!(id, %origin, "webauthn intercept duplicate dropped");
        return;
    }
    tracing::info!(id, %origin, %rp_id, %action, "webauthn intercept from page");
    let ev = crate::cef::ipc::WebAuthnEvent {
        id,
        action,
        origin,
        rp_id,
        public_key_json,
    };
    if let Some(tx) = &cef_state().ipc_events {
        let _ = tx.send(crate::cef::ipc::FromEngine::WebAuthn(ev));
    }
}

const WEBAUTHN_BEACON: &str = "https://sola.invalid/__sola_webauthn__?";

fn take_webauthn_beacon(url: &str) -> Option<String> {
    let rest = url.strip_prefix(WEBAUTHN_BEACON)?;
    Some(percent_decode(rest))
}

fn percent_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        } else if b[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn eval_js_all_frames(browser: &cef::Browser, script: &str) {
    use cef::ImplBrowser;
    let code: cef::CefString = script.into();
    if let Some(frame) = browser.main_frame() {
        frame.execute_java_script(Some(&code), None, 0);
    }
    if let Some(frame) = browser.focused_frame() {
        frame.execute_java_script(Some(&code), None, 0);
    }
    let mut ids = cef::CefStringList::new();
    browser.frame_identifiers(Some(&mut ids));
    for id in ids {
        let ident = cef::CefString::from(id.as_str());
        if let Some(frame) = browser.frame_by_identifier(Some(&ident)) {
            frame.execute_java_script(Some(&code), None, 0);
        }
    }
}

// ── DownloadHandler ───────────────────────────────────────────────

cef::wrap_download_handler! {
    pub struct BrowserDownloadHandler {}

    impl DownloadHandler {
        fn can_download(
            &self,
            _browser: Option<&mut cef::Browser>,
            _url: Option<&cef::CefString>,
            _request_method: Option<&cef::CefString>,
        ) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut cef::Browser>,
            download_item: Option<&mut cef::DownloadItem>,
            suggested_name: Option<&cef::CefString>,
            callback: Option<&mut cef::BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            use cef::{ImplBeforeDownloadCallback, ImplDownloadItem};
            let Some(item) = download_item else { return 1 };
            if item.is_valid() == 0 {
                return 1;
            }
            let suggested = suggested_name
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| cef_string_userfree_display(&item.suggested_file_name()));
            let dest = crate::downloads::unique_dest(&suggested);
            let dest_s = dest.to_string_lossy().into_owned();
            if let Some(cb) = callback {
                let path = cef::CefString::from(dest_s.as_str());
                cb.cont(Some(&path), 0);
            }
            let ev = download_event_from_item(
                item,
                crate::cef::ipc::DownloadPhase::Progress,
                Some(&dest_s),
            );
            emit_download(ev);
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut cef::Browser>,
            download_item: Option<&mut cef::DownloadItem>,
            callback: Option<&mut cef::DownloadItemCallback>,
        ) {
            use crate::cef::ipc::DownloadPhase;
            use cef::ImplDownloadItem;
            let Some(item) = download_item else { return };
            if item.is_valid() == 0 {
                return;
            }
            let id = item.id();
            let state = cef_state();
            if let Some(cb) = callback {
                state.download_cbs.borrow_mut().insert(id, cb.clone());
            }
            let phase = if item.is_complete() != 0 {
                DownloadPhase::Complete
            } else if item.is_canceled() != 0 {
                DownloadPhase::Canceled
            } else if item.is_interrupted() != 0 {
                DownloadPhase::Failed
            } else {
                DownloadPhase::Progress
            };
            if phase == DownloadPhase::Progress && !should_emit_progress(&state, id, item.percent_complete()) {
                return;
            }
            if phase != DownloadPhase::Progress {
                state.download_cbs.borrow_mut().remove(&id);
                state.download_last.borrow_mut().remove(&id);
            }
            emit_download(download_event_from_item(item, phase, None));
        }
    }
}

use crate::cef::ipc::{DownloadEvent, DownloadPhase};

fn download_event_from_item(
    item: &mut cef::DownloadItem,
    state: DownloadPhase,
    path_override: Option<&str>,
) -> DownloadEvent {
    use cef::ImplDownloadItem;
    let path = path_override
        .map(str::to_string)
        .unwrap_or_else(|| cef_string_userfree_display(&item.full_path()));
    let filename = std::path::Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| cef_string_userfree_display(&item.suggested_file_name()));
    let filename = if filename.is_empty() {
        crate::downloads::sanitize_filename("download")
    } else {
        filename
    };
    DownloadEvent {
        id: item.id(),
        filename,
        path,
        url: cef_string_userfree_display(&item.url()),
        received: item.received_bytes(),
        total: item.total_bytes(),
        percent: item.percent_complete(),
        state,
    }
}

fn should_emit_progress(state: &CefThreadState, id: u32, percent: i32) -> bool {
    let now = crate::engine::monotonic_ms();
    let mut last = state.download_last.borrow_mut();
    match last.get(&id).copied() {
        Some((t, p)) if now.saturating_sub(t) < 150 && (percent < 0 || (percent - p).abs() < 2) => {
            false
        }
        _ => {
            last.insert(id, (now, percent));
            true
        }
    }
}

fn emit_download(ev: DownloadEvent) {
    let state = cef_state();
    if let Some(tx) = &state.ipc_events {
        let _ = tx.send(crate::cef::ipc::FromEngine::Download(ev));
    }
}

// ── Client ────────────────────────────────────────────────────────

cef::wrap_request_handler! {
    pub struct BrowserRequestHandler {}

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut cef::Browser>,
            _frame: Option<&mut cef::Frame>,
            request: Option<&mut cef::Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let Some(req) = request else { return 0 };
            use cef::ImplRequest;
            let url = cef_string_userfree_display(&req.url());
            if let Some(payload) = take_webauthn_beacon(&url) {
                tracing::info!(len = payload.len(), "webauthn beacon (browse)");
                #[cfg(feature = "bitwarden")]
                handle_webauthn_payload(&payload);
                return 1;
            }
            0
        }
    }
}

cef::wrap_client! {
    pub struct BrowserClient {
        pub render_handler: cef::RenderHandler,
        pub life_span_handler: cef::LifeSpanHandler,
        pub display_handler: cef::DisplayHandler,
        pub load_handler: cef::LoadHandler,
        pub download_handler: cef::DownloadHandler,
        pub request_handler: cef::RequestHandler,
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

        fn load_handler(&self) -> Option<cef::LoadHandler> {
            Some(self.load_handler.clone())
        }

        fn download_handler(&self) -> Option<cef::DownloadHandler> {
            Some(self.download_handler.clone())
        }

        fn request_handler(&self) -> Option<cef::RequestHandler> {
            Some(self.request_handler.clone())
        }
    }
}

// ── Cmd drain (posted immediately from the waiter thread) ─────────

fn cmd_waiter(
    rx: Receiver<Cmd<CefEngine>>,
    pending: Arc<Mutex<Vec<Cmd<CefEngine>>>>,
    drain_posted: Arc<AtomicBool>,
) {
    loop {
        let first = match rx.recv() {
            Ok(c) => c,
            Err(_) => {
                pending.lock().unwrap().push(Cmd::Quit);
                post_drain(&drain_posted);
                break;
            }
        };
        {
            let mut g = pending.lock().unwrap();
            g.push(first);
            while let Ok(c) = rx.try_recv() {
                g.push(c);
            }
        }
        post_drain(&drain_posted);
    }
}

fn post_drain(drain_posted: &AtomicBool) {
    if drain_posted
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        let mut task = CmdDrainTask::new();
        let _ = cef::post_task(
            cef::ThreadId::from(cef::sys::cef_thread_id_t::TID_UI),
            Some(&mut task),
        );
    }
}

fn publish_view_paint(
    state: &CefThreadState,
    tab: &CefTabState,
    src: &[u8],
    w: u32,
    h: u32,
    dirty: Vec<DirtyRect>,
) {
    let len = (w as usize) * (h as usize) * 4;
    let mut ring = tab.paint_bufs.borrow_mut();
    let mut bytes = ring.take(len);
    paint::apply_paint(&mut bytes, src, w, h, &dirty);
    paint::ensure_bgra_dirty(&mut bytes, w, h, &dirty);
    let dirty = composite_popup(tab, &mut bytes, w, h, dirty);
    let pixels = ring.publish(bytes);
    drop(ring);
    tracing::trace!(w, h, dirty = dirty.len(), ?tab.id, "CEF on_paint VIEW");
    let frame = CefFrame {
        pixels,
        width: w,
        height: h,
        dirty,
    };
    *tab.last_frame.borrow_mut() = Some(frame.clone());
    state.frames.push(TaggedFrame {
        tab_id: tab.id,
        frame,
    });
}

fn publish_popup_paint(
    state: &CefThreadState,
    tab: &CefTabState,
    src: &[u8],
    w: u32,
    h: u32,
    dirty: Vec<DirtyRect>,
) {
    {
        let mut popup = tab.popup.borrow_mut();
        popup.visible = true;
        if popup.w == 0 {
            popup.w = w;
        }
        if popup.h == 0 {
            popup.h = h;
        }
        paint::apply_paint(&mut popup.pixels, src, w, h, &dirty);
        paint::ensure_bgra_dirty(&mut popup.pixels, w, h, &dirty);
        popup.w = w;
        popup.h = h;
    }
    let Some(view) = tab.last_frame.borrow().clone() else {
        tracing::debug!(?tab.id, w, h, "PET_POPUP before first VIEW — waiting");
        return;
    };
    let need = (view.width as usize) * (view.height as usize) * 4;
    let mut ring = tab.paint_bufs.borrow_mut();
    let mut bytes = ring.take(need);
    if view.pixels.len() == need {
        bytes.copy_from_slice(&view.pixels);
    } else {
        bytes.clear();
        bytes.resize(need, 0);
        let n = view.pixels.len().min(need);
        bytes[..n].copy_from_slice(&view.pixels[..n]);
    }
    let dirty = composite_popup(tab, &mut bytes, view.width, view.height, Vec::new());
    let pixels = ring.publish(bytes);
    drop(ring);
    tracing::debug!(
        view_w = view.width,
        view_h = view.height,
        popup_w = w,
        popup_h = h,
        ?tab.id,
        "CEF on_paint POPUP"
    );
    let frame = CefFrame {
        pixels,
        width: view.width,
        height: view.height,
        dirty,
    };
    *tab.last_frame.borrow_mut() = Some(frame.clone());
    state.frames.push(TaggedFrame {
        tab_id: tab.id,
        frame,
    });
}

fn composite_popup(
    tab: &CefTabState,
    bytes: &mut [u8],
    view_w: u32,
    view_h: u32,
    mut dirty: Vec<DirtyRect>,
) -> Vec<DirtyRect> {
    let popup = tab.popup.borrow();
    if !popup.visible || popup.pixels.is_empty() || popup.w == 0 || popup.h == 0 {
        return dirty;
    }
    paint::blit_overlay(
        bytes,
        view_w,
        view_h,
        &popup.pixels,
        popup.w,
        popup.h,
        popup.x,
        popup.y,
    );
    if let Some(r) = paint::overlay_dirty(popup.x, popup.y, popup.w, popup.h, view_w, view_h) {
        dirty.push(r);
    }
    dirty
}

fn dirty_from_cef(rects: Option<&[cef::Rect]>, width: u32, height: u32) -> Vec<DirtyRect> {
    let Some(rects) = rects else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(rects.len());
    for r in rects {
        if r.width <= 0 || r.height <= 0 {
            continue;
        }
        let x = r.x.max(0) as u32;
        let y = r.y.max(0) as u32;
        let w = r.width as u32;
        let h = r.height as u32;
        if x == 0 && y == 0 && w >= width && h >= height {
            return Vec::new();
        }
        out.push(DirtyRect { x, y, w, h });
    }
    out
}

fn set_host_hidden(host: &cef::BrowserHost, hidden: bool) {
    host.set_focus(if hidden { 0 } else { 1 });
    host.was_hidden(if hidden { 1 } else { 0 });
    host.set_windowless_frame_rate(if hidden { 1 } else { 60 });
}

cef::wrap_task! {
    pub struct CmdDrainTask {}

    impl Task {
        fn execute(&self) {
            let state = cef_state();
            state.drain_posted.store(false, Ordering::Release);
            let cmds = std::mem::take(&mut *state.pending_cmds.lock().unwrap());
            let mut keep_running = true;
            for cmd in cmds {
                if !process_cmd(&state, cmd) {
                    keep_running = false;
                    break;
                }
            }
            if state.snapshot_dirty.replace(false) {
                rebuild_snapshot(&state);
            }
            if !keep_running {
                return;
            }
            // Cmds that landed while we ran: one more drain.
            if !state.pending_cmds.lock().unwrap().is_empty() {
                post_drain(&state.drain_posted);
            }
        }
    }
}

cef::wrap_task! {
    pub struct CookieFlushTask {}

    impl Task {
        fn execute(&self) {
            let state = cef_state();
            if state.shutting_down.get() {
                return;
            }
            flush_all_cookie_stores(&state);
            let mut next = CookieFlushTask::new();
            cef::post_delayed_task(
                cef::ThreadId::from(cef::sys::cef_thread_id_t::TID_UI),
                Some(&mut next),
                10_000,
            );
        }
    }
}

/// Process one Cmd from the main channel. Returns `false` for Quit
/// (caller stops the pump); `true` otherwise. `Cmd::Input` carries
/// CEF-native input (`engine::InputEvent`) and is dispatched to the
/// active tab here — no side-channel.
fn process_cmd(state: &CefThreadState, cmd: Cmd<CefEngine>) -> bool {
    match cmd {
        Cmd::Resize {
            width,
            height,
            scale: _,
        } => {
            let prev = *state.size.lock().unwrap();
            if prev == (width, height) {
                // Same widget size — do not was_resized/invalidate. A
                // no-op resize after profile switch was discarding the
                // parked compositor so tabs looked like they reloaded.
            } else {
                *state.size.lock().unwrap() = (width, height);
                // Keep last_frames (even if stale size). Chrome will not
                // display a mismatch; wiping here is what made every
                // tab switch wait on a fresh CEF paint.
                // Parked helpers only store the size — was_resized would
                // wake a hidden compositor for a profile we are not showing.
                if state.is_front.get() {
                    if let Some(tab) = active_tab(state) {
                        if let Some(host) = tab.browser.host() {
                            host.was_resized();
                        }
                    }
                }
            }
        }
        Cmd::SetFront(front) => {
            set_front(state, front);
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
                    // Caret / placeholder animation only dirty-rects after
                    // Chromium notices focus. Force a VIEW paint so the
                    // first blink is not stuck off until the next page
                    // change.
                    if focused {
                        host.invalidate(cef::PaintElementType::VIEW);
                    }
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
                    use crate::engine::EditCmd;
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
        Cmd::PasteText(_text) => {
            // Chromium OSR still has a clipboard path; use native paste.
            if let Some(tab) = active_tab(state) {
                if let Some(frame) = tab.browser.main_frame() {
                    frame.paste();
                }
            }
        }
        Cmd::EvaluateJs(script) => {
            // Vault fill + WebAuthn resolve. Google login (Gemini, etc.)
            // runs credentials.get in an accounts.google.com iframe — the
            // host can exec JS in every frame (no SOP). Main-only left
            // Chromium's native passkey window to open.
            if let Some(tab) = active_tab(state) {
                eval_js_all_frames(&tab.browser, &script);
            }
        }
        Cmd::OpenTab { id, url, title } => {
            open_tab(state, id, url, title);
        }
        Cmd::CloseTab(id) => {
            close_tab(state, id);
        }
        Cmd::SetActiveTab(id) => {
            activate_tab(state, id);
        }
        Cmd::SwitchProfileWorkspace {
            park_as_profile_id,
            resume_profile_id,
            cef_cache_path,
            create_tabs,
            active,
        } => {
            // Profile switch is exec_self (see App::switch_profile). This
            // cmd is unused; keep the arm so the enum stays compilable.
            let _ = (
                park_as_profile_id,
                resume_profile_id,
                cef_cache_path,
                create_tabs,
                active,
            );
        }
        Cmd::DropParkedProfile { profile_id } => {
            drop_parked_profile(state, &profile_id);
        }
        Cmd::CancelDownload { id, .. } => {
            if let Some(cb) = state.download_cbs.borrow_mut().remove(&id) {
                use cef::ImplDownloadItemCallback;
                cb.cancel();
            }
        }
        Cmd::HelperDied { .. } => {}
        Cmd::Quit => {
            state.shutting_down.set(true);
            // Close browsers first so network/cookie backends settle,
            // then flush every profile store (live + parked + global).
            hide_all_tabs(state);
            {
                let tabs = std::mem::take(&mut *state.tabs.borrow_mut());
                for t in tabs {
                    if let Some(host) = t.browser.host() {
                        host.close_browser(1);
                    }
                }
            }
            {
                let parked = std::mem::take(&mut *state.parked.borrow_mut());
                for (_id, park) in parked {
                    destroy_parked(park);
                }
            }
            flush_all_cookie_stores(state);
            *state.request_context.borrow_mut() = None;
            cef::quit_message_loop();
            return false;
        }
        // CEF uses CPU OSR (memcpy in on_paint) — no buffer recycle token.
        Cmd::Release { .. } | Cmd::FrameDone { .. } => {}
    }
    true
}

/// Borrowed lookup of the active tab. Returns `None` if the
/// active id doesn't match any open tab — e.g. between
/// SetActiveTab on the iced side and the corresponding cmd
/// landing on the worker.
fn active_tab(state: &CefThreadState) -> Option<std::cell::Ref<'_, CefTabState>> {
    tab_state_by_id(state, state.active.get())
}

fn tab_state_by_id(state: &CefThreadState, id: TabId) -> Option<std::cell::Ref<'_, CefTabState>> {
    let tabs = state.tabs.borrow();
    let idx = tabs.iter().position(|t| t.id == id)?;
    Some(std::cell::Ref::map(tabs, |v| &v[idx]))
}

/// Look up the tab that owns a given CEF Browser identifier.
/// Called from RenderHandler::on_paint and DisplayHandler
/// callbacks, both of which receive the browser and need to
/// route per-tab. Live strip first; parked workspaces next
/// (should be rare — parked browsers are was_hidden).
fn tab_by_browser_id(state: &CefThreadState, browser_id: i32) -> Option<TabId> {
    if let Some(id) = state
        .tabs
        .borrow()
        .iter()
        .find(|t| t.browser_id == browser_id)
        .map(|t| t.id)
    {
        return Some(id);
    }
    for park in state.parked.borrow().values() {
        if let Some(t) = park.tabs.iter().find(|t| t.browser_id == browser_id) {
            return Some(t.id);
        }
    }
    None
}

/// Update url and/or title for a browser id in the live strip or any park.
fn set_tab_url_title_by_browser_id(
    state: &CefThreadState,
    browser_id: i32,
    url: Option<String>,
    title: Option<String>,
) -> bool {
    for tab in state.tabs.borrow().iter() {
        if tab.browser_id == browser_id {
            if let Some(u) = url {
                *tab.url.lock().unwrap() = u;
            }
            if let Some(t) = title {
                *tab.title.lock().unwrap() = t;
            }
            return true;
        }
    }
    for park in state.parked.borrow().values() {
        for tab in &park.tabs {
            if tab.browser_id == browser_id {
                if let Some(u) = url {
                    *tab.url.lock().unwrap() = u;
                }
                if let Some(t) = title {
                    *tab.title.lock().unwrap() = t;
                }
                return true;
            }
        }
    }
    false
}

fn for_tab_by_browser_id(
    state: &CefThreadState,
    browser_id: i32,
    f: impl FnOnce(&CefTabState),
) -> bool {
    for tab in state.tabs.borrow().iter() {
        if tab.browser_id == browser_id {
            f(tab);
            return true;
        }
    }
    for park in state.parked.borrow().values() {
        for tab in &park.tabs {
            if tab.browser_id == browser_id {
                f(tab);
                return true;
            }
        }
    }
    false
}

fn set_tab_nav_state_by_browser_id(
    state: &CefThreadState,
    browser_id: i32,
    is_loading: bool,
    can_go_back: bool,
    can_go_forward: bool,
) -> bool {
    for_tab_by_browser_id(state, browser_id, |tab| {
        tab.is_loading.set(is_loading);
        tab.can_go_back.set(can_go_back);
        tab.can_go_forward.set(can_go_forward);
        if !is_loading {
            tab.load_progress.set(0.0);
        }
    })
}

fn set_tab_load_progress_by_browser_id(
    state: &CefThreadState,
    browser_id: i32,
    progress: f32,
) -> bool {
    for_tab_by_browser_id(state, browser_id, |tab| {
        tab.load_progress.set(progress.clamp(0.0, 1.0));
    })
}

/// Make `id` the OSR front tab: hide the previous browser, show + focus +
/// invalidate the new one, and immediately re-push a parked frame so iced
/// does not keep sampling the previous tab's texture while waiting for
/// CEF (static pages often produce no `on_paint` from `was_resized` alone).
fn activate_tab(state: &CefThreadState, id: TabId) {
    let exists = state.tabs.borrow().iter().any(|t| t.id == id);
    if !exists {
        tracing::warn!(?id, "SetActiveTab: unknown tab");
        return;
    }
    let prev = state.active.get();
    if prev != id {
        if let Some(prev_tab) = tab_state_by_id(state, prev) {
            if let Some(host) = prev_tab.browser.host() {
                // OSR multi-browser: hide inactive so CEF stops painting them
                // and treats the next show as needing a full frame.
                set_host_hidden(&host, true);
            }
        }
    }

    state.active.set(id);
    state
        .active_atomic
        .store(id.0, std::sync::atomic::Ordering::Relaxed);

    // Instant content: replay last composite only when it matches the
    // live widget. A 1280×800 park buffer stretched into a wide view
    // is the half-width "slender" flash on tab switch.
    if let Some(tab) = tab_state_by_id(state, id) {
        let want = *state.size.lock().unwrap();
        let replayed = if let Some(mut frame) = tab.last_frame.borrow().clone() {
            let ok = frame.width.abs_diff(want.0) <= 1 && frame.height.abs_diff(want.1) <= 1;
            if ok {
                // Parked buffer is a complete composite — never a dirty patch
                // against the previous tab's GPU texture.
                frame.dirty.clear();
                state.frames.push(TaggedFrame { tab_id: id, frame });
            }
            ok
        } else {
            false
        };
        if let Some(host) = tab.browser.host() {
            set_host_hidden(&host, false);
            // Same-size was_resized after profile switch was discarding
            // the parked compositor (tabs looked like they reloaded).
            if !replayed {
                host.was_resized();
            }
            host.invalidate(cef::PaintElementType::VIEW);
        }
    }
    tracing::debug!(?id, prev = ?prev, "CEF active tab");
}

/// Absolute path string for CEF cache dirs (must be absolute per CEF docs).
fn cef_abs_path(path: &std::path::Path) -> String {
    let _ = std::fs::create_dir_all(path);
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn cef_string_userfree_display(s: &cef::CefStringUserfree) -> String {
    cef::CefString::from(s).to_string()
}

fn log_request_context(tag: &str, ctx: &cef::RequestContext) {
    use cef::ImplRequestContext;
    let is_global = ctx.is_global() != 0;
    let path = cef_string_userfree_display(&ctx.cache_path());
    if path.is_empty() {
        tracing::error!(
            tag,
            is_global,
            "CEF request context has empty cache_path — cookies will not persist"
        );
    } else {
        tracing::info!(tag, is_global, path = %path, "CEF request context ok");
    }
}

#[allow(dead_code)]
fn make_profile_request_context(cache_path: &std::path::Path) -> cef::RequestContext {
    let abs = cef_abs_path(cache_path);
    // Leak settings for the process lifetime. cef-rs converts CefString via a
    // shallow Borrowed copy of the UTF-16 buffer; if CEF retains the path
    // pointer past the CreateContext call, a stack settings drop would free
    // the buffer and leave the context with an empty/incognito path.
    let settings = Box::leak(Box::new({
        let mut s = cef::RequestContextSettings::default();
        s.cache_path = cef::CefString::from(abs.as_str());
        s.persist_session_cookies = 1;
        s
    }));
    tracing::info!(path = %abs, "CEF request context (profile) creating");
    let ctx = cef::request_context_create_context(Some(settings), None)
        .expect("cef request_context_create_context failed");
    log_request_context("created", &ctx);
    ctx
}

fn flush_cookie_manager(ctx: &cef::RequestContext) {
    use cef::ImplCookieManager;
    use cef::ImplRequestContext;
    if let Some(cm) = ctx.cookie_manager(None) {
        let _ = cm.flush_store(None);
    }
}

fn flush_all_cookie_stores(state: &CefThreadState) {
    if let Some(ctx) = state.request_context.borrow().as_ref() {
        flush_cookie_manager(ctx);
    }
    for park in state.parked.borrow().values() {
        if let Some(ctx) = park.request_context.as_ref() {
            flush_cookie_manager(ctx);
        }
    }
    // Global context (installation default) — may hold cookies if a tab
    // fell back off the profile request context.
    if let Some(global) = cef::request_context_get_global_context() {
        flush_cookie_manager(&global);
    }
}

fn hide_all_tabs(state: &CefThreadState) {
    for tab in state.tabs.borrow().iter() {
        if let Some(host) = tab.browser.host() {
            set_host_hidden(&host, true);
        }
    }
}

fn set_front(state: &CefThreadState, front: bool) {
    let was = state.is_front.get();
    state.is_front.set(front);
    if !front {
        hide_all_tabs(state);
        tracing::debug!("helper parked (not front)");
        return;
    }
    if !was {
        tracing::debug!("helper front — showing active tab");
    }
    let id = state.active.get();
    if state.tabs.borrow().iter().any(|t| t.id == id) {
        activate_tab(state, id);
    }
}

#[allow(dead_code)]
fn switch_profile_workspace(
    state: &CefThreadState,
    park_as_profile_id: &str,
    resume_profile_id: &str,
    cef_cache_path: &str,
    create_tabs: Option<Vec<(TabId, String, String)>>,
    active: TabId,
) {
    // Durable cookies require the active profile to be CEF's root_cache_path.
    // That cannot change without recycle. Flush, drop live/parked browsers,
    // quit the message loop; worker_main re-inits CEF for the new profile
    // and reopens `create_tabs`. The iced window is untouched.
    let _ = park_as_profile_id;
    flush_all_cookie_stores(state);
    teardown_all_browsers(state);
    let tabs = create_tabs.unwrap_or_default();
    *state.pending_recycle.borrow_mut() = Some(PendingRecycle {
        profile_id: resume_profile_id.to_string(),
        tabs,
        active,
    });
    state.recycle.set(true);
    tracing::info!(
        profile = %resume_profile_id,
        path = %cef_cache_path,
        "recycling CEF for profile cookie store"
    );
    cef::quit_message_loop();
}

fn destroy_parked(park: ParkedWorkspace) {
    for t in park.tabs {
        if let Some(host) = t.browser.host() {
            host.close_browser(1);
        }
    }
    // request_context drops with park
}

fn drop_parked_profile(state: &CefThreadState, profile_id: &str) {
    if let Some(park) = state.parked.borrow_mut().remove(profile_id) {
        let n = park.tab_count;
        destroy_parked(park);
        tracing::info!(profile = %profile_id, tabs = n, "dropped parked profile workspace");
    }
}

#[allow(dead_code)]
fn cef_evict_parks(state: &CefThreadState) {
    use crate::engine::TabInfo;
    use crate::tab_cache::{WorkspaceSnapshot, eviction_victims};
    use std::collections::HashMap;
    use std::time::Instant;

    // Build lightweight snapshots for the policy (tab counts + last_used).
    let mut synthetic: HashMap<String, WorkspaceSnapshot> = HashMap::new();
    {
        let parked = state.parked.borrow();
        for (id, p) in parked.iter() {
            synthetic.insert(
                id.clone(),
                WorkspaceSnapshot {
                    tabs: (0..p.tab_count)
                        .map(|i| TabInfo {
                            id: TabId(i as u64),
                            url: String::new(),
                            title: String::new(),
                            is_loading: false,
                            can_go_back: false,
                            can_go_forward: false,
                            load_progress: 0.0,
                        })
                        .collect(),
                    active: TabId(0),
                    sidebar_w: 0.0,
                    last_used: p.last_used,
                },
            );
        }
    }
    let live = state.tabs.borrow().len();
    let victims = eviction_victims(&synthetic, live, Instant::now());
    for id in victims {
        drop_parked_profile(state, &id);
    }
}

fn open_tab(state: &CefThreadState, id: TabId, initial_url: String, initial_title: String) {
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
    let load_handler = BrowserLoadHandler::new();
    let download_handler = BrowserDownloadHandler::new();
    let request_handler = BrowserRequestHandler::new();
    let mut client = BrowserClient::new(
        render_handler,
        life_span_handler,
        display_handler,
        load_handler,
        download_handler,
        request_handler,
    );

    let url_c = cef::CefString::from(initial_url.as_str());
    // Global context: Settings.cache_path is the active profile cef dir.
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
    if let Some(host) = browser.host() {
        use cef::ImplBrowserHost;
        if let Some(ctx) = host.request_context() {
            log_request_context("browser", &ctx);
        }
    }
    let browser_id = browser.identifier();

    let url = Arc::new(Mutex::new(initial_url.clone()));
    let title = Arc::new(Mutex::new(initial_title));

    // Background tabs start hidden so only the active OSR surface paints.
    let is_active = state.active.get() == id;
    if let Some(host) = browser.host() {
        // Even the "active" tab stays hidden until this helper is front.
        // (Prewarm / parked profiles must not composite.)
        let show = is_active && state.is_front.get();
        set_host_hidden(&host, !show);
    }

    state.tabs.borrow_mut().push(CefTabState {
        id,
        browser_id,
        browser,
        url,
        title,
        is_loading: Cell::new(false),
        can_go_back: Cell::new(false),
        can_go_forward: Cell::new(false),
        load_progress: Cell::new(0.0),
        last_frame: RefCell::new(None),
        paint_bufs: RefCell::new(PixelRing::default()),
        popup: RefCell::new(OsrPopup::default()),
    });
    rebuild_snapshot(state);
    tracing::info!(?id, browser_id, url = %initial_url, active = is_active, "opened tab");
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
            is_loading: t.is_loading.get(),
            can_go_back: t.can_go_back.get(),
            can_go_forward: t.can_go_forward.get(),
            load_progress: t.load_progress.get(),
        })
        .collect();
    *state.tabs_snapshot.lock().unwrap() = new.clone();
    if let Some(tx) = &state.ipc_events {
        let _ = tx.send(crate::cef::ipc::FromEngine::Tabs(new));
        let _ = tx.send(crate::cef::ipc::FromEngine::Active(state.active.get().0));
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
        InputEvent::PointerButton {
            down,
            x,
            y,
            button,
            modifiers,
            click_count,
        } => {
            if down && modifiers != 0 {
                // Diagnostic for ⌘/Ctrl-click → new tab: confirms which
                // modifier bits CEF actually receives (CONTROL = 0x4 must be
                // set for Chromium to raise the new-tab popup on Linux).
                tracing::debug!(
                    button,
                    modifiers = format_args!("{modifiers:#x}"),
                    "pointer button down"
                );
            }
            let me = MouseEvent { x, y, modifiers };
            let bt = match button {
                1 => MouseButtonType::LEFT,
                2 => MouseButtonType::MIDDLE,
                3 => MouseButtonType::RIGHT,
                _ => return,
            };
            // OSR does not infer multi-click. Pass 1/2/3 so Chromium
            // can word-select (double) and line/all-select (triple).
            let n = click_count.max(1) as i32;
            host.send_mouse_click_event(Some(&me), bt, if down { 0 } else { 1 }, n);
        }
        InputEvent::Scroll {
            x,
            y,
            delta_x,
            delta_y,
            precise,
            modifiers,
        } => {
            let mut me = MouseEvent { x, y, modifiers };
            if precise {
                me.modifiers |= cef::sys::cef_event_flags_t::EVENTFLAG_PRECISION_SCROLLING_DELTA.0;
            }
            host.send_mouse_wheel_event(Some(&me), delta_x, delta_y);
        }
        InputEvent::PointerLeave { x, y, modifiers } => {
            let me = MouseEvent { x, y, modifiers };
            host.send_mouse_move_event(Some(&me), 1);
        }
        InputEvent::ImeSetComposition {
            text,
            selection_from,
            selection_to,
        } => {
            dispatch_ime_set(host, &text, selection_from, selection_to);
        }
        InputEvent::ImeCommit { text } => {
            let s = cef::CefString::from(text.as_str());
            host.ime_commit_text(Some(&s), None, 0);
        }
        InputEvent::ImeCancel => {
            host.ime_cancel_composition();
        }
        InputEvent::Key {
            down,
            vk,
            character,
            modifiers,
        } => {
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

fn dispatch_ime_set(host: &cef::BrowserHost, text: &str, selection_from: u32, selection_to: u32) {
    let s = cef::CefString::from(text);
    let utf16_len = text.encode_utf16().count() as u32;
    let mut underline = cef::CompositionUnderline::default();
    underline.range = cef::Range {
        from: 0,
        to: utf16_len,
    };
    underline.color = 0xFF_00_00_00;
    underline.background_color = 0x00_00_00_00;
    underline.thick = 0;
    underline.style = cef::CompositionUnderlineStyle::SOLID;
    let sel = cef::Range {
        from: selection_from.min(utf16_len),
        to: selection_to.min(utf16_len),
    };
    host.ime_set_composition(Some(&s), Some(&[underline]), None, Some(&sel));
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

    // Active profile owns the CEF user-data tree (cookies, localStorage).
    // Multi RequestContext under a shared process root does not persist
    // cookies (dogfood: YouTube login lost every restart). Profile switch
    // recycles CEF (shutdown + initialize) with this path; iced stays up.
    let cache_root = cef_abs_path(&crate::profiles::active().cef_user_data_dir());
    tracing::info!(
        path = %cache_root,
        profile = %crate::profiles::active().name,
        "CEF root_cache_path + cache_path (active profile)"
    );

    // Leak CEF Settings so CefString UTF-16 buffers for paths stay valid for
    // the whole process (cef-rs shallow-copies string structs on into()).
    let settings = Box::leak(Box::new({
        let mut settings = cef::Settings::default();
        settings.framework_dir_path = cef::CefString::from(&*release.to_string_lossy());
        settings.resources_dir_path = cef::CefString::from(&*resources.to_string_lossy());
        settings.locales_dir_path = cef::CefString::from(&*locales.to_string_lossy());
        settings.browser_subprocess_path = cef::CefString::from(&*exe.to_string_lossy());
        settings.root_cache_path = cef::CefString::from(cache_root.as_str());
        // Equal to root — this is the only layout that persists cookies.
        // (RequestContext under a shared process root never wrote Cookies.)
        settings.cache_path = cef::CefString::from(cache_root.as_str());
        settings.persist_session_cookies = 1;
        settings.no_sandbox = 1;
        settings.windowless_rendering_enabled = 1;
        settings.external_message_pump = 0;
        settings.multi_threaded_message_loop = 0;
        // Silence Chromium's WARNING/ERROR stderr noise (UPower probe,
        // first-run warnings, etc.). FATAL still surfaces.
        settings.log_severity = cef::LogSeverity::DISABLE;
        settings
    }));

    let args = cef::args::Args::new();
    let main_args = args.as_main_args();
    let mut app = BrowserCefApp::new(app_id, BrowserRenderProcessHandler::new());

    let rc = cef::initialize(
        Some(main_args),
        Some(settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if rc <= 0 {
        panic!("cef::initialize failed (return code {rc})");
    }
    tracing::info!("CEF initialized");
}
