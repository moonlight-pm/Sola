//! WPE engine wrapper used by the main browser binary.
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

use crate::wpe_sys as sys;

/// One frame as it crosses thread boundaries. The FD is dup'd by
/// the worker before sending so iced can own the lifetime
/// independent of WPE's buffer-recycle cycle.
pub struct WpeFrame {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    /// DRM fourcc (e.g. `0x34325241` = ARGB8888).
    pub format: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
    /// Opaque tokens the consumer hands back via `Cmd::Release`
    /// when it's done with the frame. The pair (view, buffer) is
    /// what `wpe_view_buffer_released` needs.
    pub token: ResourceToken,
}

/// `Send + Sync`-safe wrapper around the raw `WPEView*` +
/// `WPEBuffer*` pair we get from the buffer-arrival callback.
/// Always treated as opaque off the worker thread.
#[derive(Clone, Copy, Debug)]
pub struct ResourceToken {
    pub view: *mut c_void,
    pub buffer: *mut c_void,
}

unsafe impl Send for ResourceToken {}
unsafe impl Sync for ResourceToken {}

pub enum Cmd {
    /// Request a new viewport size for the active tab. The shader
    /// Program sends this when its widget bounds change.
    Resize { width: u32, height: u32 },
    /// Hand a DMA-BUF back to WPE (any tab — `Release` carries
    /// the WPE view + buffer pointer in the token).
    Release { token: ResourceToken },
    /// Forward a user input event to the active tab.
    Input(InputEvent),
    /// Toggle CEF focus on the active tab.
    Focus(bool),
    /// Navigation (back/forward/reload/etc) on the active tab.
    Nav(NavCmd),
    /// Open a new tab with `id` and load `url`. The chrome picks
    /// the id (monotonic counter on the iced side) so it knows
    /// what to call the tab before the worker has acked.
    OpenTab { id: TabId, url: String },
    /// Close a tab by id. The chrome must keep the engine's
    /// `active_tab` in sync — call `SetActiveTab` first if you're
    /// closing the active tab.
    CloseTab(TabId),
    /// Switch which tab the engine considers active. Subsequent
    /// `Resize` / `Input` / `Nav` / `Focus` cmds target this tab,
    /// and the iced subscription only forwards its frames.
    SetActiveTab(TabId),
    Quit,
}

/// Navigation actions the chrome triggers. Each maps 1:1 to a
/// `webkit_web_view_*` call.
#[derive(Debug, Clone)]
pub enum NavCmd {
    Back,
    Forward,
    Reload,
    Stop,
    LoadUrl(String),
}


/// Per-tab identifier. Allocated by the engine (monotonic
/// counter) when a tab is opened. Stable for the tab's lifetime;
/// the iced chrome uses it to drive cmds and to key the tab strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

/// Per-tab metadata visible to iced. Snapshot only — engine owns
/// the live state and pushes a fresh `Vec<TabInfo>` into the
/// shared `Arc<Mutex<Vec<TabInfo>>>` whenever anything changes.
#[derive(Debug, Clone)]
pub struct TabInfo {
    pub id: TabId,
    pub url: String,
    pub title: String,
}

/// Frame plus the tab that produced it. The iced subscription
/// drops frames whose `tab_id` isn't the active tab so we don't
/// burn import work on hidden tabs.
pub struct TaggedFrame {
    pub tab_id: TabId,
    pub frame: WpeFrame,
}

/// A user input event in a thread-safe shape. Sent over the cmd
/// channel from iced (main thread) to the WPE worker, which turns
/// it into a `WPEEvent` and dispatches via `wpe_view_event`.
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// Pointer motion. `x`/`y` are in WPE view-local pixels;
    /// `delta_x`/`delta_y` are the change since the previous
    /// PointerMove (0.0 for the first move). `modifiers` MUST
    /// include `WPE_MODIFIER_POINTER_BUTTON*` bits for any
    /// currently-held buttons — that's how WebKit distinguishes
    /// a drag from a plain hover.
    PointerMove {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        modifiers: u32,
        time_ms: u32,
    },
    /// Pointer button down/up. `button` is the X11/WPE convention:
    /// 1 = left, 2 = middle, 3 = right. `press_count` is filled in
    /// by the worker via `wpe_view_compute_press_count`.
    PointerButton {
        down: bool,
        x: f64,
        y: f64,
        button: u32,
        modifiers: u32,
        time_ms: u32,
    },
    /// Scroll. `delta_x` / `delta_y` are in CSS pixels (or wheel
    /// "ticks" if `precise` is false).
    Scroll {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        precise: bool,
        modifiers: u32,
        time_ms: u32,
    },
    /// Keyboard key down/up. `keyval` is the X11 keysym
    /// (`XK_*`); `keycode` is the hardware scancode (0 if we
    /// can't determine it — WebKit primarily uses `keyval`).
    Key {
        down: bool,
        keyval: u32,
        keycode: u32,
        modifiers: u32,
        time_ms: u32,
    },
}

pub struct WpeEngine {
    worker: Option<JoinHandle<()>>,
    cmd_tx: Sender<Cmd>,
    /// Receiver of (tab_id, frame) tuples. iced filters by active
    /// tab before importing.
    frames: Arc<Mutex<Receiver<TaggedFrame>>>,
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
}

impl WpeEngine {
    /// Spawn the WPE worker. **Blocks** until the worker has
    /// finished the parts of WPE init that consult
    /// `WAYLAND_DISPLAY` (display creation + initial-tab
    /// creation + URL load kick-off). After this returns, the
    /// worker thread is running the GMainLoop and the caller can
    /// safely restore `WAYLAND_DISPLAY` if it manipulated it
    /// (see main.rs).
    pub fn spawn(url: &str, width: u32, height: u32) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (frame_tx, frame_rx) = channel::<TaggedFrame>();
        let (ready_tx, ready_rx) = channel::<()>();
        let cursor = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let tabs_snapshot = Arc::new(Mutex::new(Vec::<TabInfo>::new()));
        let active_atomic = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let next_id = Arc::new(std::sync::atomic::AtomicU64::new(1));

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
        let worker = thread::Builder::new()
            .name("wpe-engine".into())
            .spawn(move || unsafe {
                worker_main(
                    width, height, frame_tx, cmd_rx, ready_tx, cursor_w, snapshot_w, active_w,
                )
            })
            .expect("spawn wpe-engine thread");
        let _ = ready_rx.recv();

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

    /// Mint a fresh tab id. Send it back via `Cmd::OpenTab` to
    /// have the worker actually create the WebKitWebView.
    pub fn alloc_tab_id(&self) -> TabId {
        TabId(self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    /// Shared snapshot of all open tabs. The worker rewrites this
    /// whenever a tab opens/closes/url-changes/title-changes.
    pub fn tabs_handle(&self) -> Arc<Mutex<Vec<TabInfo>>> {
        self.tabs.clone()
    }

    /// Atomic id of the currently-active tab. iced reads to filter
    /// frames in its subscription.
    pub fn active_tab_handle(&self) -> Arc<std::sync::atomic::AtomicU64> {
        self.active_tab.clone()
    }

    /// Shared handle to the current cursor shape. Reads are
    /// non-blocking; safe to call from iced's render thread.
    pub fn cursor_handle(&self) -> Arc<std::sync::atomic::AtomicU32> {
        self.cursor.clone()
    }

    pub fn cmd_sender(&self) -> Sender<Cmd> {
        self.cmd_tx.clone()
    }

    pub fn frames(&self) -> Arc<Mutex<Receiver<TaggedFrame>>> {
        self.frames.clone()
    }

    pub fn shutdown(mut self) {
        let _ = self.cmd_tx.send(Cmd::Quit);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

// ---- worker thread ------------------------------------------------

struct WorkerCtx {
    main_loop: *mut sys::GMainLoop,
    frame_tx: Sender<TaggedFrame>,
    cmd_rx: Receiver<Cmd>,
    tabs: Vec<TabState>,
    active: TabId,
    last_size: (u32, u32),
    cursor: Arc<std::sync::atomic::AtomicU32>,
    tabs_snapshot: Arc<Mutex<Vec<TabInfo>>>,
    active_atomic: Arc<std::sync::atomic::AtomicU64>,
    /// Per-tab signal callbacks (`notify::uri`, `notify::title`)
    /// set this flag whenever they update a tab's URL or title.
    /// The cmd pump checks it each tick and rebuilds the
    /// shared `Vec<TabInfo>` snapshot. Cheap to check; spares us
    /// from having to rebuild on every iced poll.
    snapshot_dirty: Arc<std::sync::atomic::AtomicBool>,
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
    frame_tx: Sender<TaggedFrame>,
    cmd_rx: Receiver<Cmd>,
    ready_tx: Sender<()>,
    cursor: Arc<std::sync::atomic::AtomicU32>,
    tabs_snapshot: Arc<Mutex<Vec<TabInfo>>>,
    active_atomic: Arc<std::sync::atomic::AtomicU64>,
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
        tabs: Vec::new(),
        active: TabId(u64::MAX),
        last_size: (width, height),
        cursor,
        tabs_snapshot,
        active_atomic,
        snapshot_dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }));
    sys::sola_wpe_set_buffer_callback(Some(on_buffer_rendered), ctx as *mut c_void);
    sys::sola_wpe_set_cursor_callback(Some(on_cursor_changed), ctx as *mut c_void);

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
        crate::input::CursorKind::Default
    } else {
        let s = std::ffi::CStr::from_ptr(name).to_string_lossy();
        crate::input::parse_cursor_name(&s)
    };
    ctx.cursor.store(
        kind as u32,
        std::sync::atomic::Ordering::Relaxed,
    );
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
        fd: OwnedFd::from_raw_fd(dup_fd),
        width: width as u32,
        height: height as u32,
        format,
        modifier,
        stride,
        offset,
        token: ResourceToken {
            view: view as *mut c_void,
            buffer: buffer as *mut c_void,
        },
    };
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
unsafe fn process_cmd(ctx: &mut WorkerCtx, cmd: Cmd) -> bool {
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
            sys::wpe_view_buffer_released(
                token.view as *mut sys::WPEView,
                token.buffer as *mut sys::WPEBuffer,
            );
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
                // Force the newly-active tab to (re-)render at the
                // current viewport size. Background tabs may have
                // been resized to a different size during their
                // previous turn as active, or never resized at all
                // if they were just opened — either way the user
                // should see a fresh, correctly-sized frame
                // immediately.
                if !tab.wpe_view.is_null() {
                    apply_resize(tab.wpe_view, ctx.last_size.0, ctx.last_size.1);
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

#[allow(dead_code)]
fn find_tab_by_webview<'a>(
    ctx: &'a WorkerCtx,
    webview: *mut sys::WebKitWebView,
) -> Option<&'a TabState> {
    ctx.tabs.iter().find(|t| t.webview == webview)
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
    tracing::info!(?id, remaining = ctx.tabs.len(), "closed tab");
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
