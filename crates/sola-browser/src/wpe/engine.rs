//! WPE engine — implements `crate::Engine` for the WPE backend.
//!
//! Migrated from the libwpe + libwpe-fdo path to the WPE Platform
//! API (wpe-platform-2.0 + wpe-platform-headless-2.0). The Platform
//! API lets the consumer advertise modifier preferences via
//! `WPEDisplay::get_preferred_buffer_formats`, which is the
//! mechanism we need to ask for ARGB8888 + LINEAR so wgpu (without
//! VK_EXT_image_drm_format_modifier) can sample buffers correctly.
//!
//! See `src/sola_wpe.c` for the GObject vmethod hijack that makes
//! this work without subclassing the FINAL `WPEDisplayHeadless`.
//!
//! WebKit silently switches its internal renderer path to the
//! Platform API the first time any WPEDisplay subclass instance is
//! constructed (it checks via `g_type_class_peek(WPE_TYPE_DISPLAY)
//! != NULL`). Past that point `webkit_web_view_new(NULL)` builds a
//! WebView using the primary WPEDisplay.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::ffi::{CString, c_void};
use std::os::fd::{FromRawFd, OwnedFd};
use std::ptr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError, TrySendError, channel, sync_channel};
use std::thread::{self, JoinHandle};

use crate::{
    ActiveHandle, ClipboardHandle, Cmd, CursorHandle, EditCmd, Engine, FrameReceiver,
    FrameSlot, NavCmd, TabId, TabInfo, TabsHandle, TaggedFrame,
};

use super::wpe_sys as sys;

/// WPE-native input event. Uses GDK `keyval`/`keycode`, f64 coordinates
/// and millisecond timestamps — the shape libWPEWebKit expects. Carried
/// by `Cmd::Input` as `WpeEngine::Input`; produced by `input.rs` and
/// dispatched by `dispatch_input` below.
#[derive(Debug, Clone)]
pub enum InputEvent {
    PointerMove { x: f64, y: f64, delta_x: f64, delta_y: f64, modifiers: u32, time_ms: u32 },
    PointerButton { down: bool, x: f64, y: f64, button: u32, modifiers: u32, time_ms: u32 },
    Scroll { x: f64, y: f64, delta_x: f64, delta_y: f64, precise: bool, modifiers: u32, time_ms: u32 },
    Key { down: bool, keyval: u32, keycode: u32, modifiers: u32, time_ms: u32 },
}

/// One plane of a dma-buf (dup'd FD + layout).
pub struct DmabufPlane {
    pub fd: OwnedFd,
    pub stride: u32,
    pub offset: u32,
}

/// One frame as it crosses thread boundaries. The FD is dup'd by
/// the worker before sending so iced can own the lifetime
/// independent of WPE's buffer-recycle cycle.
///
/// **Drop recycles the buffer.** Any path that drops a `WpeFrame`
/// without `take_token()` sends `Cmd::Release` so WPE returns the
/// dma-buf to its pool (inactive-tab drops, pending overwrite,
/// import failure). CPU-converted YUV frames release the buffer in
/// the worker before send (`token` is already `None`).
pub struct WpeFrame {
    /// Single-plane dma-buf (ARGB8888 path). Taken by the importer.
    pub fd: Option<OwnedFd>,
    /// Tightly packed BGRA8 when we converted multi-plane YUV on CPU.
    pub rgba: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// DRM fourcc (e.g. `0x34325241` = ARGB8888).
    pub format: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
    /// Extra plane layouts (stride, offset) for multi-plane RGB modifiers
    /// sharing the primary FD. Empty for classic single-plane.
    pub extra_planes: Vec<(u32, u32)>,
    token: Option<ResourceToken>,
    pub(crate) release_tx: Sender<Cmd<WpeEngine>>,
}

impl WpeFrame {
    /// Take the recycle token for GPU-held ownership. After this,
    /// Drop no longer releases — the holder must send `Cmd::Release`
    /// (or drop a `HeldToken`) when the GPU is done.
    pub fn take_token(&mut self) -> Option<ResourceToken> {
        self.token.take()
    }

    pub fn take_fd(&mut self) -> Option<OwnedFd> {
        self.fd.take()
    }

    pub fn take_rgba(&mut self) -> Option<Vec<u8>> {
        self.rgba.take()
    }
}

impl Drop for WpeFrame {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            let _ = self.release_tx.send(Cmd::Release { token });
        }
    }
}

/// GPU-held buffer token: on Drop, sends `Cmd::Release` back to the worker.
pub struct HeldToken {
    token: Option<ResourceToken>,
    release_tx: Option<Sender<Cmd<WpeEngine>>>,
}

impl HeldToken {
    /// No WPE buffer to recycle (e.g. CPU-converted YUV frame).
    pub fn none() -> Self {
        Self {
            token: None,
            release_tx: None,
        }
    }

    pub fn new(token: ResourceToken, release_tx: Sender<Cmd<WpeEngine>>) -> Self {
        Self {
            token: Some(token),
            release_tx: Some(release_tx),
        }
    }
}

impl Drop for HeldToken {
    fn drop(&mut self) {
        if let (Some(token), Some(tx)) = (self.token.take(), self.release_tx.take()) {
            let _ = tx.send(Cmd::Release { token });
        }
    }
}

/// `Send + Sync`-safe wrapper around the raw `WPEView*` +
/// `WPEBuffer*` pair we get from the buffer-arrival callback.
/// Tagged with `tab_id` so late releases for closed tabs are ignored.
/// `epoch` must match the tab's `buffer_epoch` at release time or we
/// skip `wpe_view_buffer_released` (buffer may already be destroyed
/// after navigate — double-free / UAF SIGSEGV on Google sign-in).
#[derive(Clone, Copy, Debug)]
pub struct ResourceToken {
    pub tab_id: TabId,
    pub view: *mut c_void,
    pub buffer: *mut c_void,
    pub epoch: u64,
}

unsafe impl Send for ResourceToken {}
unsafe impl Sync for ResourceToken {}

/// Claim on a buffer pointer we still owe a release for.
#[derive(Clone, Copy, Debug)]
struct BufferClaim {
    tab_id: TabId,
    epoch: u64,
}

/// Max simultaneous `live_buffers` claims. active+retire+park×N+pending+channel
/// can stack; YouTube media + multi-tab exceeded WPE's pool and EMFILE'd.
/// When at cap, refuse new claims and release the presentation untracked.
/// Only the painted tab may hold buffers (active + channel + pending ≈ 3).
const MAX_LIVE_BUFFERS: usize = 3;

/// Paint lifecycle breadcrumb (P0 instrumentation).
#[derive(Clone, Copy)]
struct PaintTrace {
    kind: u8,
    tab: u64,
    epoch: u64,
    buf: usize,
}

const TRACE_CLAIM: u8 = 1;
const TRACE_IGNORE: u8 = 2;
const TRACE_RELEASE: u8 = 3;
const TRACE_SKIP: u8 = 4;
const TRACE_REFUSE: u8 = 5;
const TRACE_CAP: u8 = 6;
const TRACE_RING: usize = 64;

pub struct WpeEngine {
    worker: Option<JoinHandle<()>>,
    cmd_tx: Sender<Cmd<WpeEngine>>,
    /// Receiver of (tab_id, frame) tuples. iced filters by active
    /// tab before importing.
    frames: Arc<Mutex<Receiver<TaggedFrame<WpeFrame>>>>,
    /// Latest CSS cursor name (encoded as `CursorKind`) WebKit
    /// asked us to display for the active tab.
    cursor: Arc<std::sync::atomic::AtomicU32>,
    /// Snapshot of all open tabs (id/url/title). Worker rebuilds
    /// this whenever tabs are opened/closed or URL/title changes.
    /// Read by the chrome to render the tab strip + URL bar.
    tabs: Arc<Mutex<Vec<TabInfo>>>,
    /// Currently active tab id (or u64::MAX if no tabs). Atomic
    /// so the iced subscription can filter frames without
    /// acquiring a mutex per frame.
    active_tab: Arc<std::sync::atomic::AtomicU64>,
    /// Monotonic counter for assigning tab ids — kept on the
    /// chrome side so it can mint ids before sending
    /// `Cmd::OpenTab` without waiting for an ack.
    next_id: Arc<std::sync::atomic::AtomicU64>,
    /// Page-copy handoff: the worker fills this with the page's selected
    /// text (from the JS bridge), the chrome drains it onto the system
    /// clipboard. See [`ClipboardHandle`].
    clipboard_out: ClipboardHandle,
}

impl Engine for WpeEngine {
    type Frame = WpeFrame;
    type Token = ResourceToken;
    type Input = InputEvent;
    type Program = super::frame::WpeProgram;

    fn spawn(_app_id: &'static str, url: &str, w: u32, h: u32) -> Self {
        // SAFETY: single-threaded program startup, before any thread spawn.
        // Set the WPE helper binary path baked in at build time.
        unsafe { std::env::set_var("WEBKIT_EXEC_PATH", env!("WEBKIT_EXEC_PATH")) };

        // Hide WAYLAND_DISPLAY from libWPEWebKit's init so its bundled
        // wpe-platform-wayland module doesn't open a phantom toplevel.
        // Restored after `spawn_inner` so iced can connect; chrome then
        // seals it again on WindowReady *before* creating any WebView
        // (WebProcess inherits env and would otherwise map org.webkit.*).
        //
        // SAFETY: single-threaded between log init and spawn_inner call.
        let saved = std::env::var("WAYLAND_DISPLAY").ok();
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };

        let engine = WpeEngine::spawn_inner(url, w, h);

        if let Some(d) = saved {
            unsafe { std::env::set_var("WAYLAND_DISPLAY", d) };
        }
        engine
    }

    fn alloc_tab_id(&self) -> TabId {
        TabId(self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    fn cmd_sender(&self) -> Sender<Cmd<WpeEngine>> {
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

    fn frames(&self) -> FrameReceiver<WpeFrame> {
        self.frames.clone()
    }

    fn make_program(slot: std::sync::Arc<FrameSlot<Self>>) -> Self::Program {
        super::frame::WpeProgram { slot }
    }

    fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Quit);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

impl WpeEngine {
    /// Inner spawn — the actual WPE worker bring-up. Called from
    /// `Engine::spawn` after the env dance is done.
    fn spawn_inner(url: &str, width: u32, height: u32) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd<WpeEngine>>();
        // Bound the frame pipeline hard. Unbounded + multi-plane CPU work
        // queued multi‑MP frames until the process hit "Too many open files".
        let (frame_tx, frame_rx) = sync_channel::<TaggedFrame<WpeFrame>>(1);
        let (ready_tx, ready_rx) = channel::<()>();
        let cursor = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let tabs_snapshot = Arc::new(Mutex::new(Vec::<TabInfo>::new()));
        let active_atomic = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let next_id = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let clipboard_out: ClipboardHandle = Arc::new(Mutex::new(None));

        // Tabs are opened by chrome after session restore (see `App::bootstrap`).
        // Only queue the initial viewport size here.
        active_atomic.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
        let _ = cmd_tx.send(Cmd::Resize {
            width,
            height,
            scale: 1.0,
        });
        let _ = url; // session/argv URLs applied by chrome bootstrap

        let cursor_w = cursor.clone();
        let snapshot_w = tabs_snapshot.clone();
        let active_w = active_atomic.clone();
        let next_id_w = next_id.clone();
        let clipboard_w = clipboard_out.clone();
        let release_tx = cmd_tx.clone();
        let worker = thread::Builder::new()
            .name("wpe-engine".into())
            .spawn(move || unsafe {
                worker_main(
                    width, height, frame_tx, cmd_rx, release_tx, ready_tx, cursor_w, snapshot_w,
                    active_w, next_id_w, clipboard_w,
                )
            })
            .expect("spawn wpe-engine thread");
        if ready_rx.recv().is_err() {
            panic!("wpe-engine worker exited before becoming ready");
        }

        Self {
            worker: Some(worker),
            cmd_tx,
            frames: Arc::new(Mutex::new(frame_rx)),
            cursor,
            tabs: tabs_snapshot,
            active_tab: active_atomic,
            next_id,
            clipboard_out,
        }
    }

}

// ---- worker thread ------------------------------------------------

struct WorkerCtx {
    main_loop: *mut sys::GMainLoop,
    /// Capacity 1: under load drop new frames instead of unbounded queue
    /// (was multi‑MB backlog → freeze + "Too many open files").
    frame_tx: SyncSender<TaggedFrame<WpeFrame>>,
    cmd_rx: Receiver<Cmd<WpeEngine>>,
    /// Clone used when emitting frames so Drop can `Cmd::Release`.
    release_tx: Sender<Cmd<WpeEngine>>,
    /// Shared persistent profile (cookies / cache / storage). All tabs use this.
    network_session: *mut sys::WebKitNetworkSession,
    tabs: Vec<TabState>,
    active: TabId,
    /// Last CSS/layout size sent to WPE resize.
    last_css: (u32, u32),
    /// Expected physical dma-buf size (CSS × scale).
    last_size: (u32, u32),
    /// Compositor / iced scale factor last applied to the active view.
    last_scale: f64,
    cursor: Arc<std::sync::atomic::AtomicU32>,
    tabs_snapshot: Arc<Mutex<Vec<TabInfo>>>,
    active_atomic: Arc<std::sync::atomic::AtomicU64>,
    /// Shared monotonic tab-id counter (also held chrome-side). The
    /// decide-policy callback mints a background-tab id from this without
    /// involving the chrome.
    next_id: Arc<std::sync::atomic::AtomicU64>,
    /// Page-copy handoff: `on_selection` fills this with the page's selected
    /// text; the chrome drains it onto the system clipboard on the next tick.
    clipboard_out: ClipboardHandle,
    /// Per-tab signal callbacks (`notify::uri`, `notify::title`)
    /// set this flag whenever they update a tab's URL or title.
    /// The cmd pump checks it each tick and rebuilds the
    /// shared `Vec<TabInfo>` snapshot. Cheap to check; spares us
    /// from having to rebuild on every iced poll.
    snapshot_dirty: Arc<std::sync::atomic::AtomicBool>,
    /// Outstanding buffer tokens not yet released (instrumentation).
    outstanding_tokens: std::sync::atomic::AtomicU64,
    /// Buffers we still owe `wpe_view_buffer_released` for (ptr → claim).
    /// Release only calls WPE when claim.epoch still matches the tab.
    live_buffers: HashMap<usize, BufferClaim>,
    /// Ring of recent paint lifecycle events (crash diagnosis).
    paint_trace: [PaintTrace; TRACE_RING],
    paint_trace_i: usize,
}

/// Per-tab state living on the worker thread. The webview ptr is
/// owned by us (we never `g_object_unref` it — WebKit recycles
/// via the WPEDisplay lifecycle). `wpe_view` is the
/// `webkit_web_view_get_wpe_view()` result, latched at create
/// time so input + resize have a stable handle.
struct TabState {
    id: TabId,
    webview: *mut sys::WebKitWebView,
    wpe_view: *mut sys::WPEView,
    /// Shared with the iced chrome for the URL bar. Updated on
    /// the `notify::uri` signal.
    url: Arc<Mutex<String>>,
    /// Shared with the iced chrome for the tab strip. Updated
    /// on the `notify::title` signal.
    title: Arc<Mutex<String>>,
    /// Shared with chrome for reload/stop toggle. Updated on `load-changed`.
    is_loading: Arc<std::sync::atomic::AtomicBool>,
    /// Last size we asked the view for (skip no-op resizes; headless
    /// `wpe_toplevel_resize` returns FALSE for equal sizes).
    view_size: (u32, u32),
    /// Last buffer dimensions from `on_buffer_rendered`.
    last_frame_size: (u32, u32),
    /// Last compositor scale we pushed to WPE (skip no-op scale_changed).
    applied_scale: f64,
    /// Instant of last size-mismatch heal nudge (rate-limit blank flashes).
    last_size_heal: Option<std::time::Instant>,
    /// Bumped on each load-started so in-flight HeldTokens become stale and
    /// must not call `wpe_view_buffer_released` after WebKit tears down the
    /// previous document's buffers.
    buffer_epoch: Arc<AtomicU64>,
}

unsafe fn worker_main(
    width: u32,
    height: u32,
    frame_tx: SyncSender<TaggedFrame<WpeFrame>>,
    cmd_rx: Receiver<Cmd<WpeEngine>>,
    release_tx: Sender<Cmd<WpeEngine>>,
    ready_tx: Sender<()>,
    cursor: Arc<std::sync::atomic::AtomicU32>,
    tabs_snapshot: Arc<Mutex<Vec<TabInfo>>>,
    active_atomic: Arc<std::sync::atomic::AtomicU64>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
    clipboard_out: ClipboardHandle,
) {
    let display = sys::sola_wpe_display_new();
    if display.is_null() {
        panic!("sola_wpe_display_new returned null");
    }
    let mut err: *mut sys::GError = ptr::null_mut();
    if sys::wpe_display_connect(display, &mut err) == 0 {
        let msg = if !err.is_null() {
            std::ffi::CStr::from_ptr((*err).message)
                .to_string_lossy()
                .into_owned()
        } else {
            "(no error)".into()
        };
        panic!("wpe_display_connect failed: {msg}");
    }
    sys::wpe_display_set_primary(display);
    tracing::info!("WPE platform display ready (subclassed for LINEAR-only modifier)");

    // D8: WebKit data/cache under active profile (share/profiles/<uuid>/).
    // ensure_active runs in run() before spawn; if tests call spawn alone,
    // ensure again so paths exist.
    let network_session = {
        let profile = crate::profiles::ensure_active();
        let data = profile.data_dir.clone();
        let cache = profile.cache_dir.clone();
        let _ = std::fs::create_dir_all(&data);
        let _ = std::fs::create_dir_all(&cache);
        let data_c = CString::new(data.to_string_lossy().as_ref()).unwrap();
        let cache_c = CString::new(cache.to_string_lossy().as_ref()).unwrap();
        let session = sys::sola_wpe_network_session_new(data_c.as_ptr(), cache_c.as_ptr());
        if session.is_null() {
            tracing::error!(
                data = %data.display(),
                cache = %cache.display(),
                profile = %profile.id,
                "failed to create WebKitNetworkSession — cookies will not persist"
            );
        } else {
            tracing::info!(
                data = %data.display(),
                cache = %cache.display(),
                profile = %profile.id,
                name = %profile.name,
                "WebKit network session ready (profile cookies)"
            );
        }
        session
    };

    let ctx = Box::into_raw(Box::new(WorkerCtx {
        main_loop: ptr::null_mut(),
        frame_tx,
        cmd_rx,
        release_tx,
        network_session,
        tabs: Vec::new(),
        active: TabId(u64::MAX),
        last_css: (width, height),
        last_size: (width, height),
        last_scale: 1.0,
        cursor,
        tabs_snapshot,
        active_atomic,
        next_id,
        clipboard_out,
        snapshot_dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        outstanding_tokens: std::sync::atomic::AtomicU64::new(0),
        live_buffers: HashMap::new(),
        paint_trace: [PaintTrace {
            kind: 0,
            tab: 0,
            epoch: 0,
            buf: 0,
        }; TRACE_RING],
        paint_trace_i: 0,
    }));
    sys::sola_wpe_set_buffer_callback(Some(on_buffer_rendered), ctx as *mut c_void);
    sys::sola_wpe_set_cursor_callback(Some(on_cursor_changed), ctx as *mut c_void);
    sys::sola_wpe_set_selection_callback(Some(on_selection), ctx as *mut c_void);

    // Drain the queued cmds that `spawn` enqueued (Resize +
    // OpenTab + SetActiveTab) before entering the main loop, so
    // the first tab exists and has a viewport size by the time
    // iced starts presenting. Anything that arrives after the
    // initial drain gets picked up by the timer pump.
    drain_initial_cmds(&mut *ctx);

    let _ = ready_tx.send(());

    let main_loop = sys::g_main_loop_new(ptr::null_mut(), 0);
    (*ctx).main_loop = main_loop;
    sys::g_timeout_add(16, Some(cb_pump_cmds), ctx as *mut c_void);

    tracing::info!("WPE engine entering GMainLoop");
    sys::g_main_loop_run(main_loop);
    tracing::info!("WPE engine GMainLoop exited");

    sys::sola_wpe_set_buffer_callback(None, ptr::null_mut());
    sys::sola_wpe_set_cursor_callback(None, ptr::null_mut());
    sys::sola_wpe_set_selection_callback(None, ptr::null_mut());
    let _ = Box::from_raw(ctx);
}

/// Process every command currently in the channel synchronously,
/// without entering the GMainLoop. Used during init so the
/// queued `OpenTab` / `SetActiveTab` cmds run *before* we signal
/// the spawner ready (i.e. before iced starts).
unsafe fn drain_initial_cmds(ctx: &mut WorkerCtx) {
    while let Ok(cmd) = ctx.cmd_rx.try_recv() {
        process_cmd(ctx, cmd);
    }
}



/// Called from C when WebKit changes the CSS cursor. `name` is
/// a borrowed UTF-8 cstr; we copy the bytes briefly to translate,
/// then store the discriminant in the shared atomic. iced's
/// `mouse_interaction` polls that atomic each frame.
unsafe extern "C" fn on_cursor_changed(
    user_data: *mut c_void,
    name: *const std::os::raw::c_char,
) {
    let ctx = &*(user_data as *mut WorkerCtx);
    let kind = if name.is_null() {
        crate::CursorKind::Default
    } else {
        let s = std::ffi::CStr::from_ptr(name).to_string_lossy();
        super::input::parse_cursor_name(&s)
    };
    ctx.cursor.store(
        kind as u32,
        std::sync::atomic::Ordering::Relaxed,
    );
}


/// Called from C with the page's current text selection (extracted via
/// `window.getSelection().toString()`), triggered by `sola_wpe_copy_selection`
/// on a Copy. Stashes it in the shared slot for the chrome to drain onto the
/// system clipboard — the headless WPE display has no Wayland clipboard, so
/// WebKit's own "Copy" never leaves the browser.
unsafe extern "C" fn on_selection(
    user_data: *mut c_void,
    text: *const std::os::raw::c_char,
) {
    if user_data.is_null() || text.is_null() {
        return;
    }
    let s = std::ffi::CStr::from_ptr(text).to_string_lossy().into_owned();
    if s.is_empty() {
        return;
    }
    let ctx = &*(user_data as *mut WorkerCtx);
    if let Ok(mut slot) = ctx.clipboard_out.lock() {
        *slot = Some(s);
    }
}

/// `decide-policy` handler:
/// - middle / ⌘ / Ctrl-click → background tab
/// - `window.open` / target=_blank (NEW_WINDOW) → new **focused** tab
///   (Google Sign-In etc. would otherwise spawn an unhandled webview and die)
///
/// Returns TRUE only when handled; FALSE lets WebKit apply default policy.
unsafe extern "C" fn on_decide_policy(
    _web_view: *mut sys::WebKitWebView,
    decision: *mut sys::WebKitPolicyDecision,
    decision_type: sys::WebKitPolicyDecisionType,
    user_data: *mut c_void,
) -> sys::gboolean {
    let is_nav = decision_type
        == sys::WebKitPolicyDecisionType_WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION;
    let is_new_window = decision_type
        == sys::WebKitPolicyDecisionType_WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION;
    if !is_nav && !is_new_window {
        return 0; // FALSE — response etc.
    }

    let nav = decision as *mut sys::WebKitNavigationPolicyDecision;
    let action = sys::webkit_navigation_policy_decision_get_navigation_action(nav);
    if action.is_null() {
        return 0;
    }
    let request = sys::webkit_navigation_action_get_request(action);
    if request.is_null() {
        return 0;
    }
    let uri_ptr = sys::webkit_uri_request_get_uri(request);
    if uri_ptr.is_null() {
        return 0;
    }
    let uri = std::ffi::CStr::from_ptr(uri_ptr).to_string_lossy().into_owned();
    if uri.is_empty() {
        return 0;
    }

    let ctx = &mut *(user_data as *mut WorkerCtx);

    if is_new_window {
        // Popup / target=_blank (Google Sign-In often uses window.open).
        // We have no multi-window chrome: load in the requesting tab instead
        // of letting WebKit spawn an unhandled webview (that path died hard).
        sys::webkit_policy_decision_ignore(decision);
        if !_web_view.is_null() {
            if let Ok(c) = CString::new(uri.as_str()) {
                sys::webkit_web_view_load_uri(_web_view, c.as_ptr());
                tracing::info!(%uri, "new-window policy → load in same tab");
            }
        }
        return 1;
    }

    let button = sys::webkit_navigation_action_get_mouse_button(action);
    let mods = sys::webkit_navigation_action_get_modifiers(action);
    let ctrl = (mods & sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_CONTROL) != 0;
    let super_key = (mods & sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_META) != 0;
    if !crate::util::is_new_tab_click(button, ctrl, super_key) {
        return 0; // ordinary click — navigate in place.
    }
    // Suppress the in-place navigation; open a background tab instead.
    sys::webkit_policy_decision_ignore(decision);
    let id = TabId(ctx.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    open_tab(ctx, id, uri, String::new());
    1 // TRUE — handled.
}

unsafe extern "C" fn on_buffer_rendered(
    user_data: *mut c_void,
    view: *mut sys::WPEView,
    buffer: *mut sys::WPEBufferDMABuf,
) {
    let ctx = &mut *(user_data as *mut WorkerCtx);

    // Identify which tab this WPEView belongs to. We capture
    // wpe_view at tab-create time via
    // `webkit_web_view_get_wpe_view`, so the lookup is just
    // pointer-equality on a short list.
    let buffer_base = buffer as *mut sys::WPEBuffer;

    let tab_id = match find_tab_by_view(ctx, view) {
        Some(t) => t.id,
        None => {
            tracing::warn!("buffer-rendered for unknown WPEView; releasing");
            // Not tracked — release once here.
            release_untracked(ctx, view, buffer_base, "unknown_view");
            return;
        }
    };

    let width = sys::wpe_buffer_get_width(buffer_base);
    let height = sys::wpe_buffer_get_height(buffer_base);
    // Record actual buffer size so Resize can detect stretch/zoom mismatch.
    if let Some(tab) = ctx.tabs.iter_mut().find(|t| t.id == tab_id) {
        let new_sz = (width as u32, height as u32);
        if tab.last_frame_size != new_sz {
            tracing::debug!(
                ?tab_id,
                frame_w = new_sz.0,
                frame_h = new_sz.1,
                css = ?tab.view_size,
                scale = tab.applied_scale,
                "wpe frame size"
            );
        }
        tab.last_frame_size = new_sz;
    }
    let n_planes = sys::wpe_buffer_dma_buf_get_n_planes(buffer);
    let format = sys::wpe_buffer_dma_buf_get_format(buffer);
    let modifier = sys::wpe_buffer_dma_buf_get_modifier(buffer);
    let width_u = width as u32;
    let height_u = height as u32;

    // DRM ARGB/XRGB — including multi-plane modifiers (NVIDIA often reports
    // n_planes>1 for a single logical RGB buffer). NV12/YUV still skipped
    // (GPU YUV path not wired; CPU convert froze YouTube).
    const AR24: u32 = 0x3432_5241;
    const XR24: u32 = 0x3432_5258;
    let is_rgb = format == AR24 || format == XR24;
    let is_yuv = format == super::yuv::DRM_FORMAT_NV12 || format == super::yuv::DRM_FORMAT_NV21;

    if n_planes == 0 || (!is_rgb && n_planes != 1) || (is_yuv && n_planes >= 2) {
        static LOGGED_SKIP: std::sync::Once = std::sync::Once::new();
        LOGGED_SKIP.call_once(|| {
            tracing::warn!(
                n_planes,
                format = format!("{:#x}", format),
                modifier,
                "wpe: skip non-RGB multi-plane buffer (released; last RGB kept)"
            );
        });
        paint_trace_push(
            ctx,
            TRACE_SKIP,
            tab_id.0,
            0,
            buffer_base as usize,
        );
        release_untracked(ctx, view, buffer_base, "multiplane_skip");
        return;
    }

    if n_planes > 1 && is_rgb {
        static LOGGED_MP: std::sync::Once = std::sync::Once::new();
        LOGGED_MP.call_once(|| {
            tracing::info!(
                n_planes,
                format = format!("{:#x}", format),
                modifier,
                "wpe: multi-plane RGB import (plane layouts)"
            );
        });
    }

    // Collect plane layouts. Prefer a single FD (dup plane 0); if later
    // planes use a different FD we still import plane 0 only (common for
    // some producers that list auxiliary planes we don't need).
    let mut planes: Vec<(i32, u32, u32)> = Vec::with_capacity(n_planes as usize);
    for i in 0..n_planes {
        let fd = sys::wpe_buffer_dma_buf_get_fd(buffer, i);
        let stride = sys::wpe_buffer_dma_buf_get_stride(buffer, i);
        let offset = sys::wpe_buffer_dma_buf_get_offset(buffer, i);
        if fd < 0 || stride == 0 {
            release_untracked(ctx, view, buffer_base, "bad_plane");
            return;
        }
        planes.push((fd, stride, offset));
    }
    let primary_fd = planes[0].0;
    let same_fd = planes.iter().all(|(fd, _, _)| *fd == primary_fd);

    let dup_fd = libc::fcntl(primary_fd, libc::F_DUPFD_CLOEXEC, 0);
    if dup_fd < 0 {
        tracing::warn!(err = ?std::io::Error::last_os_error(), "dup of dmabuf fd failed");
        release_untracked(ctx, view, buffer_base, "dup_fail");
        return;
    }

    // Build plane list for import: all planes if shared FD, else plane 0 only.
    let plane_meta: Vec<(u32, u32)> = if same_fd {
        planes.iter().map(|(_, s, o)| (*s, *o)).collect()
    } else {
        vec![(planes[0].1, planes[0].2)]
    };
    let stride = plane_meta[0].0;
    let offset = plane_meta[0].1;

    let buf_key = buffer as *mut c_void as usize;
    let epoch = ctx
        .tabs
        .iter()
        .find(|t| t.id == tab_id)
        .map(|t| t.buffer_epoch.load(AtomicOrdering::Relaxed))
        .unwrap_or(0);

    // Same pointer still claimed for this epoch → WebKit re-presented while
    // we still hold. We must not release (GPU may sample), but we also must
    // not leave WebKit without a recycle path forever: the held token will
    // release when iced swaps frames. Just drop the dup.
    if let Some(claim) = ctx.live_buffers.get(&buf_key) {
        if claim.tab_id == tab_id && claim.epoch == epoch {
            paint_trace_push(ctx, TRACE_IGNORE, tab_id.0, epoch, buf_key);
            drop(OwnedFd::from_raw_fd(dup_fd));
            // Do not call buffer_released — claim still owns the loan.
            return;
        }
    }

    // Only one live claim per tab: if this tab already holds another buffer,
    // still allow the new claim (old token will Release when Drop runs). Cap
    // is global MAX_LIVE_BUFFERS.

    // Hard cap: do not grow claims under media storm (YouTube EMFILE / pool death).
    if ctx.live_buffers.len() >= MAX_LIVE_BUFFERS
        && !ctx.live_buffers.contains_key(&buf_key)
    {
        paint_trace_push(ctx, TRACE_CAP, tab_id.0, epoch, buf_key);
        static LOGGED_CAP: std::sync::Once = std::sync::Once::new();
        LOGGED_CAP.call_once(|| {
            tracing::warn!(
                max = MAX_LIVE_BUFFERS,
                "wpe: live_buffers at cap — dropping frame (release untracked)"
            );
        });
        if ctx.live_buffers.len() >= MAX_LIVE_BUFFERS {
            dump_paint_trace(ctx, "live_buffers_cap");
        }
        drop(OwnedFd::from_raw_fd(dup_fd));
        release_untracked(ctx, view, buffer_base, "live_cap");
        return;
    }

    // First claim for this pointer: hold a GObject ref so release is safe.
    // Steal (same key, new epoch) already holds a ref from the prior claim.
    let had_claim = ctx.live_buffers.contains_key(&buf_key);
    ctx.live_buffers.insert(
        buf_key,
        BufferClaim {
            tab_id,
            epoch,
        },
    );
    if !had_claim {
        sys::sola_wpe_buffer_ref(buffer_base);
    }
    paint_trace_push(ctx, TRACE_CLAIM, tab_id.0, epoch, buf_key);

    let frame = WpeFrame {
        fd: Some(OwnedFd::from_raw_fd(dup_fd)),
        rgba: None,
        width: width_u,
        height: height_u,
        format,
        modifier,
        stride,
        offset,
        // Extra planes (shared FD) for multi-plane RGB modifiers.
        extra_planes: plane_meta
            .into_iter()
            .skip(1)
            .map(|(s, o)| (s, o))
            .collect(),
        token: Some(ResourceToken {
            tab_id,
            view: view as *mut c_void,
            buffer: buffer as *mut c_void,
            epoch,
        }),
        release_tx: ctx.release_tx.clone(),
    };
    ctx.outstanding_tokens
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Bounded channel: if iced is behind, drop this frame (Drop → Release)
    // instead of unbounded queue growth.
    match ctx.frame_tx.try_send(TaggedFrame { tab_id, frame }) {
        Ok(()) => {}
        Err(TrySendError::Full(tagged)) => {
            // Prefer the newer frame: the Full payload is the one we
            // couldn't enqueue. Drop it (releases) and leave the older
            // in-channel frame; under load we skip. Better than OOM.
            drop(tagged);
        }
        Err(TrySendError::Disconnected(_)) => {
            tracing::info!("frame channel closed, quitting GMainLoop");
            sys::g_main_loop_quit(ctx.main_loop);
        }
    }
}

unsafe extern "C" fn cb_pump_cmds(data: *mut c_void) -> sys::gboolean {
    let ctx = &mut *(data as *mut WorkerCtx);
    loop {
        match ctx.cmd_rx.try_recv() {
            Ok(cmd) => {
                if !process_cmd(ctx, cmd) {
                    return 0; /* G_SOURCE_REMOVE — Quit */
                }
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                sys::g_main_loop_quit(ctx.main_loop);
                return 0;
            }
        }
    }
    // Tabs' notify::uri / notify::title callbacks set this flag
    // when they update per-tab state. We rebuild the shared
    // Vec<TabInfo> snapshot here, on the pump's cadence (≤16 ms),
    // so the chrome's poll loop sees fresh URL/title.
    if ctx.snapshot_dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
        rebuild_snapshot(&*ctx);
    }
    // Reap WebKit child zombies (WPENetworkProcess often dies without
    // GLib child-watch draining). Leaving them piles process slots and
    // confuses the process model under YouTube media load.
    reap_zombie_children();
    1 /* G_SOURCE_CONTINUE */
}

/// Non-blocking wait for any exited children. Safe alongside GLib: we only
/// collect already-dead PIDs; live WebKit process watches still fire.
fn reap_zombie_children() {
    loop {
        // SAFETY: WNOHANG — never blocks the GLib pump.
        let mut status: libc::c_int = 0;
        let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if pid <= 0 {
            break;
        }
    }
}

/// Process one Cmd. Returns `false` to signal "stop pumping"
/// (Quit); `true` to continue. Centralises the cmd handling so
/// both the initial drain and the GLib timer pump share logic.
unsafe fn process_cmd(ctx: &mut WorkerCtx, cmd: Cmd<WpeEngine>) -> bool {
    match cmd {
        Cmd::Resize {
            width,
            height,
            scale,
        } => {
            // `width`/`height` are CSS/layout pixels; `scale` is device DPR.
            // Expected dma-buf size is width*scale × height*scale.
            let scale = if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            };
            let phys = (
                ((width as f64) * scale).round().max(1.0) as u32,
                ((height as f64) * scale).round().max(1.0) as u32,
            );
            ctx.last_css = (width, height);
            ctx.last_size = phys;
            ctx.last_scale = scale;
            // Active tab only — resizing every tab every frame exhausted the
            // WPE buffer pool and led to invalid buffer_released / SIGSEGV.
            if let Some(tab) = active_tab_mut(ctx) {
                if !tab.wpe_view.is_null() {
                    // Apply scale first so resize uses the new DPR for buffer alloc.
                    apply_view_scale(tab, scale);
                    ensure_view_size(tab, width, height);
                    tracing::debug!(
                        css_w = width,
                        css_h = height,
                        ?phys,
                        scale,
                        frame = ?tab.last_frame_size,
                        "resize css+scale"
                    );
                }
            }
        }
        Cmd::Release { token } => {
            let left = ctx
                .outstanding_tokens
                .fetch_sub(1, AtomicOrdering::Relaxed)
                .saturating_sub(1);
            let key = token.buffer as usize;
            // Only call WPE when this token still owns the claim *and* the
            // tab's buffer epoch matches (no navigation since import).
            let own_claim = ctx.live_buffers.get(&key).is_some_and(|c| {
                c.tab_id == token.tab_id && c.epoch == token.epoch
            });
            if key == 0 || !own_claim {
                paint_trace_push(ctx, TRACE_SKIP, token.tab_id.0, token.epoch, key);
                tracing::debug!(
                    ?token.tab_id,
                    epoch = token.epoch,
                    outstanding = left,
                    "skip release (stale/unknown claim)"
                );
            } else {
                ctx.live_buffers.remove(&key);
                let buf = token.buffer as *mut sys::WPEBuffer;
                let view = token.view as *mut sys::WPEView;
                let epoch_ok = ctx.tabs.iter().any(|t| {
                    t.id == token.tab_id
                        && t.buffer_epoch.load(AtomicOrdering::Relaxed) == token.epoch
                });
                if !epoch_ok {
                    // Navigated or closed — still drop our GObject ref; do not
                    // call buffer_released if we can avoid it (WebKit may have
                    // torn down). Safe helper no-ops on dead GObjects when
                    // possible; our ref keeps the type check valid.
                    paint_trace_push(ctx, TRACE_SKIP, token.tab_id.0, token.epoch, key);
                    tracing::debug!(
                        ?token.tab_id,
                        epoch = token.epoch,
                        "skip WPE release (epoch advanced); unref held buffer"
                    );
                    if !buf.is_null() {
                        // Tell WPE while we still hold a ref (protocol), then unref.
                        if !view.is_null() {
                            sys::sola_wpe_view_buffer_released_safe(view, buf);
                        }
                        sys::sola_wpe_buffer_unref(buf);
                    }
                } else if !token.view.is_null() {
                    paint_trace_push(ctx, TRACE_RELEASE, token.tab_id.0, token.epoch, key);
                    release_owned(view, buf);
                } else if !buf.is_null() {
                    sys::sola_wpe_buffer_unref(buf);
                }
            }
            if left > 16 || ctx.live_buffers.len() > MAX_LIVE_BUFFERS {
                tracing::warn!(
                    outstanding = left,
                    live = ctx.live_buffers.len(),
                    "wpe buffer pressure"
                );
                dump_paint_trace(ctx, "buffer_pressure");
            }
        }
        Cmd::Input(ev) => {
            if let Some(tab) = active_tab(ctx) {
                if !tab.wpe_view.is_null() {
                    dispatch_input(tab.wpe_view, ev);
                }
            }
        }
        Cmd::Focus(focused) => {
            // Need mut for force_view_repaint on focus-in (black after
            // app switch when WPE stopped presenting).
            let (w, h) = ctx.last_css;
            if let Some(tab) = active_tab_mut(ctx) {
                if !tab.wpe_view.is_null() {
                    if focused {
                        sys::wpe_view_focus_in(tab.wpe_view);
                        // Same-size resize is a no-op; nudge so static pages
                        // and post-suspend views emit a fresh buffer.
                        force_view_repaint(tab, w, h);
                    } else {
                        sys::wpe_view_focus_out(tab.wpe_view);
                    }
                }
            }
        }
        Cmd::Nav(nav) => {
            if let Some(tab) = active_tab(ctx) {
                if !tab.webview.is_null() {
                    // Optimistic is_loading for reload↔stop chrome.
                    match &nav {
                        NavCmd::Reload | NavCmd::LoadUrl(_) | NavCmd::Back | NavCmd::Forward => {
                            tab.is_loading
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                            ctx.snapshot_dirty
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        NavCmd::Stop => {
                            tab.is_loading
                                .store(false, std::sync::atomic::Ordering::Relaxed);
                            ctx.snapshot_dirty
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    dispatch_nav(tab.webview, nav);
                }
            }
        }
        Cmd::Edit(edit) => {
            if let Some(tab) = active_tab(ctx) {
                if !tab.webview.is_null() {
                    match edit {
                        // Copy is bridged to the system clipboard: WebKit's own
                        // "Copy" only reaches its internal clipboard (the
                        // headless display has no Wayland clipboard), so instead
                        // extract the selection and let the chrome write it via
                        // iced's Wayland-backed clipboard. Async — the result
                        // lands in `clipboard_out` via `on_selection`.
                        EditCmd::Copy => {
                            sys::sola_wpe_copy_selection(tab.webview as *mut _);
                        }
                        // Paste without text is a no-op on headless WPE — use
                        // Cmd::PasteText with chrome-read clipboard content.
                        EditCmd::Paste => {
                            tracing::debug!("EditCmd::Paste ignored; use PasteText");
                        }
                        _ => {
                            let name = crate::util::editing_command_name(edit);
                            let name_c = std::ffi::CString::new(name).unwrap();
                            sys::webkit_web_view_execute_editing_command(
                                tab.webview as *mut _,
                                name_c.as_ptr(),
                            );
                        }
                    }
                }
            }
        }
        Cmd::PasteText(text) => {
            if let Some(tab) = active_tab(ctx) {
                if !tab.webview.is_null() && !text.is_empty() {
                    // InsertText with argument — WebKit has no Wayland clipboard
                    // on the headless display, so the chrome ships the string.
                    let cmd = CString::new("InsertText").unwrap();
                    let arg = CString::new(text.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
                    sys::webkit_web_view_execute_editing_command_with_argument(
                        tab.webview as *mut _,
                        cmd.as_ptr(),
                        arg.as_ptr(),
                    );
                }
            }
        }
        Cmd::EvaluateJs(script) => {
            if let Some(tab) = active_tab(ctx) {
                if !tab.webview.is_null() && !script.is_empty() {
                    match CString::new(script) {
                        Ok(c) => {
                            sys::sola_wpe_evaluate_js(tab.webview as *mut _, c.as_ptr());
                        }
                        Err(_) => {
                            tracing::warn!("vault fill: script contained interior NUL — skipped");
                        }
                    }
                }
            }
        }
        Cmd::OpenTab { id, url, title } => {
            open_tab(ctx, id, url, title);
        }
        Cmd::CloseTab(id) => {
            close_tab(ctx, id);
        }
        Cmd::SetActiveTab(id) => {
            // Tab must exist (chrome should never send a SetActiveTab
            // for an unknown id, but tolerate it by ignoring).
            if let Some(idx) = ctx.tabs.iter().position(|t| t.id == id) {
                ctx.active = id;
                ctx.active_atomic
                    .store(id.0, std::sync::atomic::Ordering::Relaxed);
                let (w, h) = ctx.last_css;
                let scale = ctx.last_scale;
                let tab = &mut ctx.tabs[idx];
                if !tab.wpe_view.is_null() {
                    // Blank / new-tab: leave focus OUT of the webview so the
                    // omnibox caret stays visible (⌘T focuses the URL bar).
                    // Real pages take focus so typing goes into content.
                    let url = tab.url.lock().unwrap().clone();
                    let blank = url.is_empty() || url == "about:blank";
                    if blank {
                        sys::wpe_view_focus_out(tab.wpe_view);
                    } else {
                        sys::wpe_view_focus_in(tab.wpe_view);
                    }
                    apply_view_scale(tab, scale);
                    // Force a fresh buffer after backgrounding. Same-size
                    // `wpe_toplevel_resize` returns FALSE (no-op) so static
                    // pages (example.org) never repaint without a 1px nudge.
                    force_view_repaint(tab, w, h);
                }
            }
        }
        Cmd::Quit => {
            sys::g_main_loop_quit(ctx.main_loop);
            return false;
        }
    }
    true
}

/// Linear scan for the active tab. Stable as long as we keep tab
/// count low (a handful) — no need for a HashMap.
fn active_tab(ctx: &WorkerCtx) -> Option<&TabState> {
    ctx.tabs.iter().find(|t| t.id == ctx.active)
}

fn active_tab_mut(ctx: &mut WorkerCtx) -> Option<&mut TabState> {
    let id = ctx.active;
    ctx.tabs.iter_mut().find(|t| t.id == id)
}

fn find_tab_by_view<'a>(ctx: &'a WorkerCtx, view: *mut sys::WPEView) -> Option<&'a TabState> {
    ctx.tabs.iter().find(|t| t.wpe_view == view)
}


/// Per-tab signal-callback context. We Box::into_raw one of these
/// per webview and pass it as `user_data` to
/// `g_signal_connect_data`. The closure-notify free fn at
/// `free_tab_signal_ctx` drops the Box when the webview is
/// destroyed.
struct TabSignalCtx {
    tab_id: TabId,
    url: Arc<Mutex<String>>,
    title: Arc<Mutex<String>>,
    is_loading: Arc<std::sync::atomic::AtomicBool>,
    buffer_epoch: Arc<AtomicU64>,
    snapshot: Arc<Mutex<Vec<TabInfo>>>,
    /// Snapshot rebuild needs *all* tabs' current url/title/loading; we
    /// can't see them from here. Set this flag and the pump-tick
    /// rebuilds on its next iteration.
    snapshot_dirty: Arc<std::sync::atomic::AtomicBool>,
}

unsafe extern "C" fn free_tab_signal_ctx(data: *mut c_void, _closure: *mut sys::_GClosure) {
    let _ = Box::from_raw(data as *mut TabSignalCtx);
}

unsafe fn open_tab(ctx: &mut WorkerCtx, id: TabId, initial_url: String, initial_title: String) {
    // Defense in depth: never let a WebProcess inherit a compositor socket.
    // (Chrome also seals WAYLAND_DISPLAY on WindowReady before first open.)
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        tracing::warn!(
            "WAYLAND_DISPLAY still set while opening WebView — clearing to avoid phantom toplevel"
        );
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    let webview = sys::sola_wpe_web_view_new(ctx.network_session);
    if webview.is_null() {
        tracing::warn!(?id, "sola_wpe_web_view_new returned null; tab not opened");
        return;
    }
    let wpe_view = sys::webkit_web_view_get_wpe_view(webview);
    if wpe_view.is_null() {
        tracing::warn!(?id, "webkit_web_view_get_wpe_view returned null");
    }

    // Match sola dark chrome (#0a0a0b) so blank / pre-paint frames are not white.
    let mut bg = sys::WebKitColor {
        red: 0.039_215_686,   // 0x0a
        green: 0.039_215_686, // 0x0a
        blue: 0.043_137_255,  // 0x0b
        alpha: 1.0,
    };
    sys::webkit_web_view_set_background_color(webview as *mut _, &mut bg);

    let url = Arc::new(Mutex::new(initial_url.clone()));
    // Session restore seeds title; WebKit overwrites when the page sets one.
    let title = Arc::new(Mutex::new(initial_title));
    // Will load immediately when URL non-empty.
    let is_loading = Arc::new(std::sync::atomic::AtomicBool::new(!initial_url.is_empty()));
    let buffer_epoch = Arc::new(AtomicU64::new(0));

    // Per-tab signal context for notify::uri / title / load-changed.
    // Separate Boxes so the destroy-notify on each signal frees its own.
    let dirty = ctx.snapshot_dirty.clone();
    let snap = ctx.tabs_snapshot.clone();
    let url_arc = url.clone();
    let title_arc = title.clone();
    let loading_arc = is_loading.clone();
    let epoch_arc = buffer_epoch.clone();
    let make_sig_ctx = || {
        Box::into_raw(Box::new(TabSignalCtx {
            tab_id: id,
            url: url_arc.clone(),
            title: title_arc.clone(),
            is_loading: loading_arc.clone(),
            buffer_epoch: epoch_arc.clone(),
            snapshot: snap.clone(),
            snapshot_dirty: dirty.clone(),
        })) as *mut c_void
    };

    let uri_signal = CString::new("notify::uri").unwrap();
    sys::g_signal_connect_data(
        webview as *mut c_void,
        uri_signal.as_ptr(),
        Some(std::mem::transmute::<
            unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void),
            unsafe extern "C" fn(),
        >(on_notify_uri_tab)),
        make_sig_ctx(),
        Some(free_tab_signal_ctx),
        0,
    );

    let title_signal = CString::new("notify::title").unwrap();
    sys::g_signal_connect_data(
        webview as *mut c_void,
        title_signal.as_ptr(),
        Some(std::mem::transmute::<
            unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void),
            unsafe extern "C" fn(),
        >(on_notify_title_tab)),
        make_sig_ctx(),
        Some(free_tab_signal_ctx),
        0,
    );

    let load_signal = CString::new("load-changed").unwrap();
    sys::g_signal_connect_data(
        webview as *mut c_void,
        load_signal.as_ptr(),
        Some(std::mem::transmute::<
            unsafe extern "C" fn(*mut c_void, sys::WebKitLoadEvent, *mut c_void),
            unsafe extern "C" fn(),
        >(on_load_changed_tab)),
        make_sig_ctx(),
        Some(free_tab_signal_ctx),
        0,
    );

    // Intercept link clicks so middle / ⌘ / Ctrl-click opens a background
    // tab instead of navigating in place. The worker ctx (stable for the
    // GMainLoop's lifetime) is the user-data, so the callback can mint a tab
    // id and open the tab on this same thread.
    let policy_signal = CString::new("decide-policy").unwrap();
    sys::g_signal_connect_data(
        webview as *mut c_void,
        policy_signal.as_ptr(),
        Some(std::mem::transmute::<
            unsafe extern "C" fn(
                *mut sys::WebKitWebView,
                *mut sys::WebKitPolicyDecision,
                sys::WebKitPolicyDecisionType,
                *mut c_void,
            ) -> sys::gboolean,
            unsafe extern "C" fn(),
        >(on_decide_policy)),
        ctx as *mut WorkerCtx as *mut c_void,
        None,
        0,
    );

    // Always load a URI so WebKit has a complete document/state (skipping
    // about:blank left tabs half-initialized: no strip highlight race,
    // no reliable focus handoff). Dark background (above) prevents the
    // default opaque-white about:blank flash.
    if !initial_url.is_empty() {
        let url_c = CString::new(initial_url.as_str()).unwrap();
        sys::webkit_web_view_load_uri(webview as *mut _, url_c.as_ptr());
    }

    let mut tab = TabState {
        id,
        webview,
        wpe_view,
        url,
        title,
        is_loading,
        view_size: (0, 0),
        last_frame_size: (0, 0),
        applied_scale: 0.0,
        last_size_heal: None,
        buffer_epoch,
    };

    // Size/scale to match the active iced scissor (CSS + DPR).
    if !wpe_view.is_null() {
        apply_view_scale(&mut tab, ctx.last_scale);
        ensure_view_size(&mut tab, ctx.last_css.0, ctx.last_css.1);
        // New blank tabs start unfocused so chrome can own the omnibox caret.
        let blank = initial_url.is_empty() || initial_url == "about:blank";
        if blank {
            sys::wpe_view_focus_out(wpe_view);
        }
    }

    ctx.tabs.push(tab);
    rebuild_snapshot(ctx);
    tracing::info!(?id, url = %initial_url, tabs = ctx.tabs.len(), "opened tab");
}

unsafe fn close_tab(ctx: &mut WorkerCtx, id: TabId) {
    let pos = match ctx.tabs.iter().position(|t| t.id == id) {
        Some(p) => p,
        None => return,
    };
    let tab = ctx.tabs.remove(pos);
    // Drop our reference to the WebKitWebView. We never explicitly
    // `g_object_ref`'d it (webkit_web_view_new returns a floating
    // ref that sinks into the platform's view), so a single
    // g_object_unref balances it.
    // Late HeldToken Releases for this tab will hit the closed-tab path
    // and only clear live_buffers — never call into a dead view.
    if !tab.webview.is_null() {
        sys::g_object_unref(tab.webview as *mut c_void);
    }
    rebuild_snapshot(ctx);
    let outstanding = ctx
        .outstanding_tokens
        .load(std::sync::atomic::Ordering::Relaxed);
    tracing::info!(
        ?id,
        remaining = ctx.tabs.len(),
        outstanding_tokens = outstanding,
        live_buffers = ctx.live_buffers.len(),
        "closed tab"
    );
}

/// Call WPE release after claim removed; drops the GObject ref we took on claim.
unsafe fn release_owned(view: *mut sys::WPEView, buffer: *mut sys::WPEBuffer) {
    if buffer.is_null() {
        return;
    }
    if !view.is_null() {
        sys::sola_wpe_view_buffer_released_safe(view, buffer);
    }
    sys::sola_wpe_buffer_unref(buffer);
}

/// Immediate release for never-claimed buffers (skip/dup fail). No claim-ref.
///
/// Refuses if still claimed — double release SEGV (YouTube coredumps).
unsafe fn release_untracked(
    ctx: &mut WorkerCtx,
    view: *mut sys::WPEView,
    buffer: *mut sys::WPEBuffer,
    reason: &'static str,
) {
    if view.is_null() || buffer.is_null() {
        return;
    }
    let key = buffer as *mut c_void as usize;
    if ctx.live_buffers.contains_key(&key) {
        paint_trace_push(ctx, TRACE_REFUSE, 0, 0, key);
        tracing::error!(
            key = format!("{:#x}", key),
            reason,
            live = ctx.live_buffers.len(),
            "REFUSE wpe_view_buffer_released — buffer still claimed (would SEGV)"
        );
        dump_paint_trace(ctx, "refuse_claimed_release");
        return;
    }
    sys::sola_wpe_view_buffer_released_safe(view, buffer);
}

fn paint_trace_push(ctx: &mut WorkerCtx, kind: u8, tab: u64, epoch: u64, buf: usize) {
    let i = ctx.paint_trace_i % TRACE_RING;
    ctx.paint_trace[i] = PaintTrace {
        kind,
        tab,
        epoch,
        buf,
    };
    ctx.paint_trace_i = ctx.paint_trace_i.wrapping_add(1);
}

fn dump_paint_trace(ctx: &WorkerCtx, why: &str) {
    let n = ctx.paint_trace_i.min(TRACE_RING);
    if n == 0 {
        return;
    }
    let start = ctx.paint_trace_i.saturating_sub(n);
    let mut parts = Vec::with_capacity(n);
    for k in 0..n {
        let e = ctx.paint_trace[(start + k) % TRACE_RING];
        if e.kind == 0 {
            continue;
        }
        let name = match e.kind {
            TRACE_CLAIM => "claim",
            TRACE_IGNORE => "ignore",
            TRACE_RELEASE => "release",
            TRACE_SKIP => "skip",
            TRACE_REFUSE => "refuse",
            TRACE_CAP => "cap",
            _ => "?",
        };
        parts.push(format!(
            "{name}:t{}:e{}:{:#x}",
            e.tab, e.epoch, e.buf
        ));
    }
    tracing::warn!(
        why,
        live = ctx.live_buffers.len(),
        outstanding = ctx
            .outstanding_tokens
            .load(std::sync::atomic::Ordering::Relaxed),
        ring = %parts.join(" | "),
        "paint lifecycle dump"
    );
}

/// Rewrite the shared `Vec<TabInfo>` from the current tab state.
/// Called whenever tabs are opened/closed or a per-tab URL/title
/// changes (via the snapshot_dirty flag, checked at pump time).
fn rebuild_snapshot(ctx: &WorkerCtx) {
    let new: Vec<TabInfo> = ctx
        .tabs
        .iter()
        .map(|t| {
            let (can_back, can_fwd) = if t.webview.is_null() {
                (false, false)
            } else {
                unsafe {
                    (
                        sys::webkit_web_view_can_go_back(t.webview) != 0,
                        sys::webkit_web_view_can_go_forward(t.webview) != 0,
                    )
                }
            };
            TabInfo {
                id: t.id,
                url: t.url.lock().unwrap().clone(),
                title: t.title.lock().unwrap().clone(),
                is_loading: t
                    .is_loading
                    .load(std::sync::atomic::Ordering::Relaxed),
                can_go_back: can_back,
                can_go_forward: can_fwd,
            }
        })
        .collect();
    *ctx.tabs_snapshot.lock().unwrap() = new;
}

/// `notify::uri` callback. user_data is a Box<TabSignalCtx>; we
/// read the new URI off the webview and update the tab's url
/// Arc, then flag the snapshot dirty so the next pump tick
/// rebuilds the chrome-visible Vec.
unsafe extern "C" fn on_notify_uri_tab(
    object: *mut c_void,
    _pspec: *mut c_void,
    user_data: *mut c_void,
) {
    let cb = &*(user_data as *const TabSignalCtx);
    let uri_ptr = sys::webkit_web_view_get_uri(object as *mut sys::WebKitWebView);
    let uri = if uri_ptr.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(uri_ptr)
            .to_string_lossy()
            .into_owned()
    };
    *cb.url.lock().unwrap() = uri;
    cb.snapshot_dirty
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = &cb.snapshot; // keep the shared Arc alive without warning
}

unsafe extern "C" fn on_notify_title_tab(
    object: *mut c_void,
    _pspec: *mut c_void,
    user_data: *mut c_void,
) {
    let cb = &*(user_data as *const TabSignalCtx);
    let title_ptr = sys::webkit_web_view_get_title(object as *mut sys::WebKitWebView);
    let title = if title_ptr.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(title_ptr)
            .to_string_lossy()
            .into_owned()
    };
    *cb.title.lock().unwrap() = title;
    cb.snapshot_dirty
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = &cb.snapshot;
}

/// `load-changed` — drive chrome reload/stop toggle via `is_loading`.
/// On STARTED, bump `buffer_epoch` so in-flight HeldTokens become stale
/// and must not call `wpe_view_buffer_released` after WebKit tears down
/// the previous page's dma-bufs (Google / YouTube sign-in crash).
unsafe extern "C" fn on_load_changed_tab(
    _object: *mut c_void,
    load_event: sys::WebKitLoadEvent,
    user_data: *mut c_void,
) {
    let cb = &*(user_data as *const TabSignalCtx);
    let loading = load_event != sys::WebKitLoadEvent_WEBKIT_LOAD_FINISHED;
    cb.is_loading
        .store(loading, AtomicOrdering::Relaxed);
    if load_event == sys::WebKitLoadEvent_WEBKIT_LOAD_STARTED {
        let new_epoch = cb.buffer_epoch.fetch_add(1, AtomicOrdering::Relaxed) + 1;
        tracing::debug!(?cb.tab_id, new_epoch, "buffer epoch bump (load started)");
    }
    cb.snapshot_dirty
        .store(true, AtomicOrdering::Relaxed);
    let _ = &cb.snapshot;
}

/// Materialize an `InputEvent` as a WPEEvent GObject and dispatch
/// to the view. Each event constructor takes ownership of the
/// new event ref; `wpe_view_event` consumes it (we don't need to
/// `wpe_event_unref` ourselves).
unsafe fn dispatch_input(view: *mut sys::WPEView, ev: InputEvent) {
    let mouse_src = sys::WPEInputSource_WPE_INPUT_SOURCE_MOUSE;
    let kbd_src = sys::WPEInputSource_WPE_INPUT_SOURCE_KEYBOARD;
    let event = match ev {
        InputEvent::PointerMove { x, y, delta_x, delta_y, modifiers, time_ms } => {
            sys::wpe_event_pointer_move_new(
                sys::WPEEventType_WPE_EVENT_POINTER_MOVE,
                view,
                mouse_src,
                time_ms,
                modifiers,
                x,
                y,
                delta_x,
                delta_y,
            )
        }
        InputEvent::PointerButton { down, x, y, button, modifiers, time_ms } => {
            let type_ = if down {
                sys::WPEEventType_WPE_EVENT_POINTER_DOWN
            } else {
                sys::WPEEventType_WPE_EVENT_POINTER_UP
            };
            // press_count drives single/double/triple-click on the
            // web side. WPE keeps the per-view bookkeeping for us.
            // UP events MUST use press_count=0; otherwise WebKit's
            // click-synthesis path (mousedown+mouseup → click) won't
            // fire, which breaks link navigation and form submit.
            // See `WPEWaylandSeat.cpp:178` for the upstream reference.
            let press_count = if down {
                sys::wpe_view_compute_press_count(view, x, y, button, time_ms)
            } else {
                0
            };
            sys::wpe_event_pointer_button_new(
                type_,
                view,
                mouse_src,
                time_ms,
                modifiers,
                button,
                x,
                y,
                press_count,
            )
        }
        InputEvent::Scroll { x, y, delta_x, delta_y, precise, modifiers, time_ms } => {
            sys::wpe_event_scroll_new(
                view,
                mouse_src,
                time_ms,
                modifiers,
                delta_x,
                delta_y,
                if precise { 1 } else { 0 },
                0, /* is_stop — only relevant for kinetic/touchpad scroll */
                x,
                y,
            )
        }
        InputEvent::Key { down, keyval, keycode, modifiers, time_ms } => {
            let type_ = if down {
                sys::WPEEventType_WPE_EVENT_KEYBOARD_KEY_DOWN
            } else {
                sys::WPEEventType_WPE_EVENT_KEYBOARD_KEY_UP
            };
            sys::wpe_event_keyboard_new(
                type_,
                view,
                kbd_src,
                time_ms,
                modifiers,
                keycode,
                keyval,
            )
        }
    };
    if event.is_null() {
        tracing::warn!("wpe_event_*_new returned null; dropping input");
        return;
    }
    sys::wpe_view_event(view, event);
}


/// Dispatch a `NavCmd` to the WebKitWebView. Runs on the worker
/// thread (the only thread allowed to touch WebKit APIs).
unsafe fn dispatch_nav(webview: *mut sys::WebKitWebView, nav: NavCmd) {
    match nav {
        NavCmd::Back => sys::webkit_web_view_go_back(webview),
        NavCmd::Forward => sys::webkit_web_view_go_forward(webview),
        NavCmd::Reload => sys::webkit_web_view_reload(webview),
        NavCmd::Stop => sys::webkit_web_view_stop_loading(webview),
        NavCmd::LoadUrl(url) => {
            let c = match CString::new(url.as_str()) {
                Ok(c) => c,
                Err(_) => {
                    tracing::warn!(url = %url, "url contains NUL byte, ignoring");
                    return;
                }
            };
            sys::webkit_web_view_load_uri(webview, c.as_ptr());
            tracing::info!(url = %url, "Nav::LoadUrl");
        }
    }
}

/// Ask the view to present after tab reactivation.
///
/// `width`/`height` are CSS pixels (same as Resize). One 1px nudge on
/// **activate only** — not on every Resize (that caused black flash thrash).
unsafe fn force_view_repaint(tab: &mut TabState, width: u32, height: u32) {
    if width == 0 || height == 0 || tab.wpe_view.is_null() {
        return;
    }
    let nudge_w = width.saturating_sub(1).max(1);
    tab.view_size = (0, 0);
    apply_resize_tab(tab, nudge_w, height);
    apply_resize_tab(tab, width, height);
    tab.last_size_heal = Some(std::time::Instant::now());
}

/// Bring the view to CSS size `(width, height)`. Rare heal if dma-buf
/// physical size ≠ CSS×scale (3s cooldown).
unsafe fn ensure_view_size(tab: &mut TabState, width: u32, height: u32) {
    if tab.wpe_view.is_null() || width == 0 || height == 0 {
        return;
    }
    let want_css = (width, height);
    if tab.view_size != want_css {
        apply_resize_tab(tab, width, height);
        return;
    }
    let scale = tab.applied_scale.max(1.0);
    let want_phys = (
        ((width as f64) * scale).round().max(1.0) as u32,
        ((height as f64) * scale).round().max(1.0) as u32,
    );
    let frame = tab.last_frame_size;
    if frame == (0, 0) || frame == want_phys {
        return;
    }
    const HEAL_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(3);
    let due = tab
        .last_size_heal
        .map(|t| t.elapsed() >= HEAL_COOLDOWN)
        .unwrap_or(true);
    if !due {
        return;
    }
    tracing::info!(
        ?want_css,
        ?want_phys,
        ?frame,
        scale,
        "buffer size ≠ CSS×scale — one heal nudge (3s cooldown)"
    );
    tab.last_size_heal = Some(std::time::Instant::now());
    tab.last_frame_size = (0, 0);
    tab.view_size = (0, 0);
    let nudge_w = width.saturating_sub(1).max(1);
    apply_resize_tab(tab, nudge_w, height);
    apply_resize_tab(tab, width, height);
}

/// Apply a new size to one tab's WPE view. No-ops only when `view_size`
/// already equals the target (callers that need a re-apply clear
/// `view_size` first).
unsafe fn apply_resize_tab(tab: &mut TabState, width: u32, height: u32) {
    if tab.wpe_view.is_null() || width == 0 || height == 0 {
        return;
    }
    if tab.view_size == (width, height) {
        return;
    }
    let view = tab.wpe_view;
    let toplevel = sys::wpe_view_get_toplevel(view);
    if toplevel.is_null() {
        tracing::warn!("wpe_view_get_toplevel returned null; cannot resize");
        return;
    }
    let ok = sys::wpe_toplevel_resize(toplevel, width as i32, height as i32);
    if ok == 0 {
        tracing::debug!(
            width,
            height,
            prev = ?tab.view_size,
            "wpe_toplevel_resize returned FALSE; notifying resized anyway",
        );
    }
    // Headless has no compositor configure — we must fire resized ourselves
    // or WebProcess keeps the default size.
    sys::wpe_toplevel_resized(toplevel, width as i32, height as i32);
    sys::wpe_view_resized(view, width as i32, height as i32);
    tab.view_size = (width, height);
    tracing::debug!(width, height, "view resized + notified");
}

/// Tell WPE/WebKit the compositor scale so HiDPI text is not rendered soft.
/// No-ops when scale is unchanged (avoids re-layout thrash).
unsafe fn apply_view_scale(tab: &mut TabState, scale: f64) {
    if tab.wpe_view.is_null() || !(scale.is_finite() && scale > 0.0) {
        return;
    }
    if (tab.applied_scale - scale).abs() < 0.001 {
        return;
    }
    let toplevel = sys::wpe_view_get_toplevel(tab.wpe_view);
    if toplevel.is_null() {
        return;
    }
    sys::wpe_toplevel_scale_changed(toplevel, scale);
    tab.applied_scale = scale;
}
