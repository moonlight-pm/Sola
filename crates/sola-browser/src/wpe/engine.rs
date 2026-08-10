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

use std::ffi::{CString, c_void};
use std::os::fd::{FromRawFd, OwnedFd};
use std::ptr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
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

/// One frame as it crosses thread boundaries. The FD is dup'd by
/// the worker before sending so iced can own the lifetime
/// independent of WPE's buffer-recycle cycle.
///
/// **Drop recycles the buffer.** Any path that drops a `WpeFrame`
/// without `take_token()` sends `Cmd::Release` so WPE returns the
/// dma-buf to its pool (inactive-tab drops, pending overwrite,
/// import failure).
pub struct WpeFrame {
    /// Taken by the importer; `None` after successful import handoff.
    pub fd: Option<OwnedFd>,
    pub width: u32,
    pub height: u32,
    /// DRM fourcc (e.g. `0x34325241` = ARGB8888).
    pub format: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
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
    pub token: ResourceToken,
    release_tx: Sender<Cmd<WpeEngine>>,
}

impl HeldToken {
    pub fn new(token: ResourceToken, release_tx: Sender<Cmd<WpeEngine>>) -> Self {
        Self { token, release_tx }
    }
}

impl Drop for HeldToken {
    fn drop(&mut self) {
        let token = ResourceToken {
            tab_id: self.token.tab_id,
            view: self.token.view,
            buffer: self.token.buffer,
        };
        let _ = self.release_tx.send(Cmd::Release { token });
    }
}

/// `Send + Sync`-safe wrapper around the raw `WPEView*` +
/// `WPEBuffer*` pair we get from the buffer-arrival callback.
/// Tagged with `tab_id` so late releases for closed tabs are ignored.
#[derive(Clone, Copy, Debug)]
pub struct ResourceToken {
    pub tab_id: TabId,
    pub view: *mut c_void,
    pub buffer: *mut c_void,
}

unsafe impl Send for ResourceToken {}
unsafe impl Sync for ResourceToken {}



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
        // Restored after `spawn_inner` returns; iced sees it on the main thread.
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
        let (frame_tx, frame_rx) = channel::<TaggedFrame<WpeFrame>>();
        let (ready_tx, ready_rx) = channel::<()>();
        let cursor = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let tabs_snapshot = Arc::new(Mutex::new(Vec::<TabInfo>::new()));
        let active_atomic = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let next_id = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let clipboard_out: ClipboardHandle = Arc::new(Mutex::new(None));

        let initial_id = TabId(next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        active_atomic.store(initial_id.0, std::sync::atomic::Ordering::Relaxed);

        // Queue: set initial size, then open the first tab, then
        // activate it. The pump processes these in order on the
        // worker thread.
        let _ = cmd_tx.send(Cmd::Resize { width, height });
        let _ = cmd_tx.send(Cmd::OpenTab {
            id: initial_id,
            url: url.to_string(),
        });
        let _ = cmd_tx.send(Cmd::SetActiveTab(initial_id));

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
    frame_tx: Sender<TaggedFrame<WpeFrame>>,
    cmd_rx: Receiver<Cmd<WpeEngine>>,
    /// Clone used when emitting frames so Drop can `Cmd::Release`.
    release_tx: Sender<Cmd<WpeEngine>>,
    tabs: Vec<TabState>,
    active: TabId,
    last_size: (u32, u32),
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
}

unsafe fn worker_main(
    width: u32,
    height: u32,
    frame_tx: Sender<TaggedFrame<WpeFrame>>,
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

    let ctx = Box::into_raw(Box::new(WorkerCtx {
        main_loop: ptr::null_mut(),
        frame_tx,
        cmd_rx,
        release_tx,
        tabs: Vec::new(),
        active: TabId(u64::MAX),
        last_size: (width, height),
        cursor,
        tabs_snapshot,
        active_atomic,
        next_id,
        clipboard_out,
        snapshot_dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        outstanding_tokens: std::sync::atomic::AtomicU64::new(0),
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

/// `decide-policy` handler: middle / ⌘ / Ctrl-click on a link opens a
/// background tab instead of navigating in place. Returns TRUE only when it
/// has handled (ignored) the navigation; FALSE lets WebKit apply default
/// policy. User-data is the worker `WorkerCtx` pointer (stable for the
/// GMainLoop's lifetime).
unsafe extern "C" fn on_decide_policy(
    _web_view: *mut sys::WebKitWebView,
    decision: *mut sys::WebKitPolicyDecision,
    decision_type: sys::WebKitPolicyDecisionType,
    user_data: *mut c_void,
) -> sys::gboolean {
    // Only ordinary navigations (link clicks) are interesting; let WebKit
    // apply default policy to everything else (new-window, response, …).
    if decision_type
        != sys::WebKitPolicyDecisionType_WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION
    {
        return 0; // FALSE
    }
    let nav = decision as *mut sys::WebKitNavigationPolicyDecision;
    let action = sys::webkit_navigation_policy_decision_get_navigation_action(nav);
    if action.is_null() {
        return 0;
    }
    let button = sys::webkit_navigation_action_get_mouse_button(action);
    let mods = sys::webkit_navigation_action_get_modifiers(action);
    let ctrl = (mods & sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_CONTROL) != 0;
    let super_key = (mods & sys::WPEModifiers_WPE_MODIFIER_KEYBOARD_META) != 0;
    if !crate::util::is_new_tab_click(button, ctrl, super_key) {
        return 0; // ordinary click — navigate in place.
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
    // Suppress the in-place navigation; open a background tab instead.
    sys::webkit_policy_decision_ignore(decision);
    let ctx = &mut *(user_data as *mut WorkerCtx);
    let id = TabId(ctx.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    open_tab(ctx, id, uri); // no SetActiveTab → background tab
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
    let tab_id = match find_tab_by_view(ctx, view) {
        Some(t) => t.id,
        None => {
            tracing::warn!("buffer-rendered for unknown WPEView; dropping");
            return;
        }
    };

    let buffer_base = buffer as *mut sys::WPEBuffer;
    let width = sys::wpe_buffer_get_width(buffer_base);
    let height = sys::wpe_buffer_get_height(buffer_base);
    let n_planes = sys::wpe_buffer_dma_buf_get_n_planes(buffer);
    if n_planes != 1 {
        tracing::warn!(n_planes, "ignoring multi-plane frame");
        return;
    }
    let format = sys::wpe_buffer_dma_buf_get_format(buffer);
    let modifier = sys::wpe_buffer_dma_buf_get_modifier(buffer);
    let stride = sys::wpe_buffer_dma_buf_get_stride(buffer, 0);
    let offset = sys::wpe_buffer_dma_buf_get_offset(buffer, 0);
    let raw_fd = sys::wpe_buffer_dma_buf_get_fd(buffer, 0);

    let dup_fd = libc::fcntl(raw_fd, libc::F_DUPFD_CLOEXEC, 0);
    if dup_fd < 0 {
        tracing::warn!(err = ?std::io::Error::last_os_error(), "dup of dmabuf fd failed");
        return;
    }

    let frame = WpeFrame {
        fd: Some(OwnedFd::from_raw_fd(dup_fd)),
        width: width as u32,
        height: height as u32,
        format,
        modifier,
        stride,
        offset,
        token: Some(ResourceToken {
            tab_id,
            view: view as *mut c_void,
            buffer: buffer as *mut c_void,
        }),
        release_tx: ctx.release_tx.clone(),
    };
    ctx.outstanding_tokens
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if ctx.frame_tx.send(TaggedFrame { tab_id, frame }).is_err() {
        tracing::info!("frame channel closed, quitting GMainLoop");
        sys::g_main_loop_quit(ctx.main_loop);
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
    1 /* G_SOURCE_CONTINUE */
}

/// Process one Cmd. Returns `false` to signal "stop pumping"
/// (Quit); `true` to continue. Centralises the cmd handling so
/// both the initial drain and the GLib timer pump share logic.
unsafe fn process_cmd(ctx: &mut WorkerCtx, cmd: Cmd<WpeEngine>) -> bool {
    match cmd {
        Cmd::Resize { width, height } => {
            ctx.last_size = (width, height);
            if let Some(tab) = active_tab(ctx) {
                if !tab.wpe_view.is_null() {
                    apply_resize(tab.wpe_view, width, height);
                }
            }
        }
        Cmd::Release { token } => {
            let left = ctx
                .outstanding_tokens
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed)
                .saturating_sub(1);
            // Closed-tab quarantine: never call into a freed view.
            if !ctx.tabs.iter().any(|t| t.id == token.tab_id) {
                tracing::debug!(
                    ?token.tab_id,
                    outstanding = left,
                    "dropping release for closed tab"
                );
            } else {
                sys::wpe_view_buffer_released(
                    token.view as *mut sys::WPEView,
                    token.buffer as *mut sys::WPEBuffer,
                );
            }
            if left > 8 {
                tracing::warn!(outstanding = left, "wpe buffer tokens outstanding");
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
            if let Some(tab) = active_tab(ctx) {
                if !tab.wpe_view.is_null() {
                    if focused {
                        sys::wpe_view_focus_in(tab.wpe_view);
                    } else {
                        sys::wpe_view_focus_out(tab.wpe_view);
                    }
                }
            }
        }
        Cmd::Nav(nav) => {
            if let Some(tab) = active_tab(ctx) {
                if !tab.webview.is_null() {
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
        Cmd::OpenTab { id, url } => {
            open_tab(ctx, id, url);
        }
        Cmd::CloseTab(id) => {
            close_tab(ctx, id);
        }
        Cmd::SetActiveTab(id) => {
            // Tab must exist (chrome should never send a SetActiveTab
            // for an unknown id, but tolerate it by ignoring).
            if let Some(tab) = ctx.tabs.iter().find(|t| t.id == id) {
                ctx.active = id;
                ctx.active_atomic
                    .store(id.0, std::sync::atomic::Ordering::Relaxed);
                if !tab.wpe_view.is_null() {
                    // Focus the view so WebKit routes input correctly.
                    sys::wpe_view_focus_in(tab.wpe_view);
                    // Force a new buffer even when the size is unchanged.
                    // `wpe_toplevel_resize` is idempotent for equal sizes, so
                    // a static page that stopped painting while backgrounded
                    // would otherwise leave the chrome stuck on the previous
                    // tab's last texture forever.
                    force_view_repaint(tab.wpe_view, ctx.last_size.0, ctx.last_size.1);
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

fn find_tab_by_view<'a>(ctx: &'a WorkerCtx, view: *mut sys::WPEView) -> Option<&'a TabState> {
    ctx.tabs.iter().find(|t| t.wpe_view == view)
}


/// Per-tab signal-callback context. We Box::into_raw one of these
/// per webview and pass it as `user_data` to
/// `g_signal_connect_data`. The closure-notify free fn at
/// `free_tab_signal_ctx` drops the Box when the webview is
/// destroyed.
struct TabSignalCtx {
    url: Arc<Mutex<String>>,
    title: Arc<Mutex<String>>,
    snapshot: Arc<Mutex<Vec<TabInfo>>>,
    /// Snapshot rebuild needs *all* tabs' current url/title; we
    /// can't see them from here. Set this flag and the pump-tick
    /// rebuilds on its next iteration.
    snapshot_dirty: Arc<std::sync::atomic::AtomicBool>,
}

unsafe extern "C" fn free_tab_signal_ctx(data: *mut c_void, _closure: *mut sys::_GClosure) {
    let _ = Box::from_raw(data as *mut TabSignalCtx);
}

unsafe fn open_tab(ctx: &mut WorkerCtx, id: TabId, initial_url: String) {
    let webview = sys::webkit_web_view_new(ptr::null_mut());
    if webview.is_null() {
        tracing::warn!(?id, "webkit_web_view_new returned null; tab not opened");
        return;
    }
    let wpe_view = sys::webkit_web_view_get_wpe_view(webview);
    if wpe_view.is_null() {
        tracing::warn!(?id, "webkit_web_view_get_wpe_view returned null");
    }

    let url = Arc::new(Mutex::new(initial_url.clone()));
    let title = Arc::new(Mutex::new(String::new()));

    // Per-tab signal context for notify::uri and notify::title.
    // Two separate Boxes so the destroy-notify on each signal
    // frees its own — both can be safely dropped independently.
    let dirty = ctx.snapshot_dirty.clone();
    let snap = ctx.tabs_snapshot.clone();
    let url_arc = url.clone();
    let title_arc = title.clone();
    let make_sig_ctx = || {
        Box::into_raw(Box::new(TabSignalCtx {
            url: url_arc.clone(),
            title: title_arc.clone(),
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

    let url_c = CString::new(initial_url.as_str()).unwrap();
    sys::webkit_web_view_load_uri(webview as *mut _, url_c.as_ptr());

    // Resize the new tab to whatever iced is currently displaying.
    if !wpe_view.is_null() {
        apply_resize(wpe_view, ctx.last_size.0, ctx.last_size.1);
    }

    ctx.tabs.push(TabState {
        id,
        webview,
        wpe_view,
        url,
        title,
    });
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
        "closed tab"
    );
}

/// Rewrite the shared `Vec<TabInfo>` from the current tab state.
/// Called whenever tabs are opened/closed or a per-tab URL/title
/// changes (via the snapshot_dirty flag, checked at pump time).
fn rebuild_snapshot(ctx: &WorkerCtx) {
    let new: Vec<TabInfo> = ctx
        .tabs
        .iter()
        .map(|t| TabInfo {
            id: t.id,
            url: t.url.lock().unwrap().clone(),
            title: t.title.lock().unwrap().clone(),
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

/// Nudge the view through a 1px size change and back so WebKit emits a
/// fresh frame after tab reactivation. Same-size `apply_resize` alone is
/// a no-op and static pages produce no further buffers while backgrounded.
unsafe fn force_view_repaint(view: *mut sys::WPEView, width: u32, height: u32) {
    if width == 0 || height == 0 {
        apply_resize(view, width, height);
        return;
    }
    let nudge_w = if width > 1 { width - 1 } else { width + 1 };
    apply_resize(view, nudge_w, height);
    apply_resize(view, width, height);
}

/// Resize the view's toplevel. WPE's WebProcess picks this up and
/// produces subsequent buffers at the new size. Idempotent — calling
/// with the same size as before is a no-op inside WPE.
unsafe fn apply_resize(view: *mut sys::WPEView, width: u32, height: u32) {
    if view.is_null() {
        return;
    }
    let toplevel = sys::wpe_view_get_toplevel(view);
    if toplevel.is_null() {
        tracing::warn!("wpe_view_get_toplevel returned null; cannot resize");
        return;
    }
    let ok = sys::wpe_toplevel_resize(toplevel, width as i32, height as i32);
    if ok == 0 {
        tracing::warn!(
            width,
            height,
            "wpe_toplevel_resize returned FALSE — backend rejected the size",
        );
        return;
    }
    // On Wayland backends `wpe_toplevel_resize` requests a size from
    // the compositor and the actual size lands later via a configure
    // event, which then triggers `wpe_toplevel_resized` /
    // `wpe_view_resized` internally. The headless backend has no
    // compositor round-trip — without calling the resized notifiers
    // ourselves, the size sticks at the WPEView level but the
    // WebProcess never gets told to re-render at the new size, so
    // frames keep coming at the headless default (1024x768).
    sys::wpe_toplevel_resized(toplevel, width as i32, height as i32);
    sys::wpe_view_resized(view, width as i32, height as i32);
    tracing::info!(width, height, "wpe_toplevel_resize accepted + notified");
}
