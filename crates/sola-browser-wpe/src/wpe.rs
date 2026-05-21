//! WPE engine wrapper used by the main browser binary.
//!
//! WPE's data structures (exportable backend, GMainLoop, WebKitWebView)
//! are not thread-safe — they live on a single thread, the one that
//! runs `g_main_loop_run`. We dedicate a worker thread to that loop
//! and shuttle data over channels:
//!
//! - **Outbound** (worker → main): `FrameChan` carries each new
//!   `WpeFrame` (FD ownership transferred, plus a token identifying
//!   the original `wl_resource*` so the main thread can ask us to
//!   release it later).
//! - **Inbound** (main → worker): `CmdChan` carries control messages
//!   — currently just `Release { token }` (consumer is done with a
//!   frame and WPE may recycle the buffer) and `Quit` (shutdown).
//!
//! The worker thread polls the inbound channel from a GLib idle
//! source so commands are processed alongside WPE's own events.
//!
//! For the spike: hardcoded URL, hardcoded view size, no input
//! forwarding yet, no profile / cookies setup.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{CString, c_void};
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::ptr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::{self, JoinHandle};

// bindgen-generated FFI re-exported from main.rs's `wpe` module so
// every binary in the crate sees the same symbol set.
use crate::wpe_sys as sys;

/// One frame as it crosses thread boundaries. The FD is dup'd by the
/// worker before sending; the main thread owns it and closes it
/// (via the imported `VkDeviceMemory`'s lifetime) when done.
pub struct WpeFrame {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    /// DRM fourcc (e.g. `0x34325241` = ARGB8888).
    pub format: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
    /// Opaque token the consumer hands back via `Cmd::Release` when
    /// it's done with the frame. Internally the raw `wl_resource*`
    /// the WPE callback gave us — never dereferenced off the worker
    /// thread.
    pub token: ResourceToken,
}

/// `Send + Sync`-safe wrapper around the raw `wl_resource*` we get
/// from the WPE callback. Always treated as opaque off-worker.
#[derive(Clone, Copy, Debug)]
pub struct ResourceToken(pub *mut c_void);
unsafe impl Send for ResourceToken {}
unsafe impl Sync for ResourceToken {}

pub enum Cmd {
    Release { token: ResourceToken },
    Quit,
}

pub struct WpeEngine {
    /// Worker thread handle. Joined on `WpeEngine::shutdown`.
    worker: Option<JoinHandle<()>>,
    cmd_tx: Sender<Cmd>,
    /// Frame receiver wrapped in `Arc<Mutex<_>>` so the iced
    /// subscription stream can borrow it across awaits without
    /// owning the engine itself.
    frames: Arc<Mutex<Receiver<WpeFrame>>>,
}

impl WpeEngine {
    /// Spawn the worker, initialize WPE, load `url`, start producing
    /// frames at `width × height`. Returns once the worker is up and
    /// the GMainLoop is running.
    pub fn spawn(url: &str, width: u32, height: u32) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (frame_tx, frame_rx) = channel::<WpeFrame>();
        let url = url.to_string();
        let worker = thread::Builder::new()
            .name("wpe-engine".into())
            .spawn(move || unsafe { worker_main(url, width, height, frame_tx, cmd_rx) })
            .expect("spawn wpe-engine thread");
        Self {
            worker: Some(worker),
            cmd_tx,
            frames: Arc::new(Mutex::new(frame_rx)),
        }
    }

    /// Sender end of the command channel — clone and hand to the
    /// shader pipeline so it can post `Cmd::Release` when a new
    /// frame replaces an old one.
    pub fn cmd_sender(&self) -> Sender<Cmd> {
        self.cmd_tx.clone()
    }

    /// Shared handle to the frame receiver. The subscription stream
    /// locks this to call `recv()` on a blocking task.
    pub fn frames(&self) -> Arc<Mutex<Receiver<WpeFrame>>> {
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

/// State the GLib callbacks read and write. Held inside the worker
/// thread's frame; pointer captured by callback closures as a typed
/// `*mut WorkerCtx`.
struct WorkerCtx {
    exportable: *mut sys::wpe_view_backend_exportable_fdo,
    main_loop: *mut sys::GMainLoop,
    frame_tx: Sender<WpeFrame>,
    cmd_rx: Receiver<Cmd>,
}

unsafe fn worker_main(
    url: String,
    width: u32,
    height: u32,
    frame_tx: Sender<WpeFrame>,
    cmd_rx: Receiver<Cmd>,
) {
    // Same init sequence as the standalone wpe-probe: backend lib by
    // absolute path → GBM-platform EGL display → wpe_fdo_initialize
    // → exportable view backend → WebKitWebView → load URL → run
    // GMainLoop.
    let backend_so = CString::new(env!("WPE_BACKEND_FDO_SO")).unwrap();
    if !sys::wpe_loader_init(backend_so.as_ptr()) {
        panic!("wpe_loader_init failed for {}", env!("WPE_BACKEND_FDO_SO"));
    }

    let render_node = CString::new("/dev/dri/renderD128").unwrap();
    let drm_fd = libc::open(render_node.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
    if drm_fd < 0 {
        panic!(
            "open(/dev/dri/renderD128) failed: {}",
            std::io::Error::last_os_error()
        );
    }
    let gbm_dev = sys::gbm_create_device(drm_fd);
    if gbm_dev.is_null() {
        panic!("gbm_create_device returned null");
    }

    type EglGetPlatformDisplayFn = unsafe extern "C" fn(
        platform: sys::EGLenum,
        native_display: *mut c_void,
        attrib_list: *const sys::EGLAttrib,
    ) -> sys::EGLDisplay;
    let name = CString::new("eglGetPlatformDisplay").unwrap();
    let func = sys::eglGetProcAddress(name.as_ptr())
        .expect("eglGetProcAddress(eglGetPlatformDisplay) returned null");
    let get_pdpy: EglGetPlatformDisplayFn = std::mem::transmute(func);
    let egl_dpy = get_pdpy(
        sys::EGL_PLATFORM_GBM_KHR,
        gbm_dev as *mut c_void,
        ptr::null(),
    );
    if egl_dpy.is_null() {
        panic!("eglGetPlatformDisplay(GBM) returned EGL_NO_DISPLAY");
    }
    let mut major = 0;
    let mut minor = 0;
    if sys::eglInitialize(egl_dpy, &mut major, &mut minor) == 0 {
        panic!("eglInitialize failed");
    }
    tracing::info!(major, minor, "EGL display initialized on GBM platform");

    if !sys::wpe_fdo_initialize_for_egl_display(egl_dpy) {
        panic!("wpe_fdo_initialize_for_egl_display failed");
    }

    let ctx = Box::into_raw(Box::new(WorkerCtx {
        exportable: ptr::null_mut(),
        main_loop: ptr::null_mut(),
        frame_tx,
        cmd_rx,
    }));

    let client = sys::wpe_view_backend_exportable_fdo_client {
        export_buffer_resource: Some(cb_export_buffer_resource),
        export_dmabuf_resource: Some(cb_export_dmabuf_resource),
        export_shm_buffer: Some(cb_export_shm_buffer),
        _wpe_reserved0: None,
        _wpe_reserved1: None,
    };
    let exportable = sys::wpe_view_backend_exportable_fdo_create(
        &client as *const _ as *mut _,
        ctx as *mut c_void,
        width,
        height,
    );
    if exportable.is_null() {
        panic!("wpe_view_backend_exportable_fdo_create returned null");
    }
    (*ctx).exportable = exportable;
    tracing::info!("created exportable view backend");

    let view_backend = sys::wpe_view_backend_exportable_fdo_get_view_backend(exportable);
    let wk_backend = sys::webkit_web_view_backend_new(view_backend, None, ptr::null_mut());
    let view = sys::webkit_web_view_new(wk_backend);
    if view.is_null() {
        panic!("webkit_web_view_new returned null");
    }
    tracing::info!("created WebKitWebView");

    let url_c = CString::new(url.as_str()).unwrap();
    sys::webkit_web_view_load_uri(view as *mut _, url_c.as_ptr());
    tracing::info!(url = %url, "kicked off URL load");

    let main_loop = sys::g_main_loop_new(ptr::null_mut(), 0);
    (*ctx).main_loop = main_loop;

    // Poll the inbound command channel from a GLib idle source so
    // Cmd::Release / Cmd::Quit are handled on the worker thread
    // alongside WPE's own events. Low frequency (60 Hz) is fine for
    // release-buffer latency since we already produce a frame at a
    // time.
    sys::g_timeout_add(16, Some(cb_pump_cmds), ctx as *mut c_void);

    tracing::info!(url = url, "WPE engine entering GMainLoop");
    sys::g_main_loop_run(main_loop);
    tracing::info!("WPE engine GMainLoop exited");

    let _ = Box::from_raw(ctx);
}

unsafe extern "C" fn cb_export_dmabuf_resource(
    data: *mut c_void,
    res: *mut sys::wpe_view_backend_exportable_fdo_dmabuf_resource,
) {
    let ctx = &mut *(data as *mut WorkerCtx);
    let res = &*res;
    tracing::info!(
        w = res.width,
        h = res.height,
        format = format!("{:#x}", res.format),
        modifier = format!("{:#x}", res.modifiers[0]),
        n_planes = res.n_planes,
        stride = res.strides[0],
        fd = res.fds[0],
        "WPE produced DMA-BUF frame",
    );
    if res.n_planes != 1 {
        tracing::warn!(n_planes = res.n_planes, "ignoring multi-plane frame");
        sys::wpe_view_backend_exportable_fdo_dispatch_release_buffer(
            ctx.exportable,
            res.buffer_resource,
        );
        sys::wpe_view_backend_exportable_fdo_dispatch_frame_complete(ctx.exportable);
        return;
    }
    let dup_fd = libc::fcntl(res.fds[0], libc::F_DUPFD_CLOEXEC, 0);
    if dup_fd < 0 {
        tracing::warn!(err = ?std::io::Error::last_os_error(), "dup of dmabuf fd failed");
        return;
    }
    let frame = WpeFrame {
        fd: OwnedFd::from_raw_fd(dup_fd),
        width: res.width,
        height: res.height,
        format: res.format,
        modifier: res.modifiers[0],
        stride: res.strides[0],
        offset: res.offsets[0],
        token: ResourceToken(res.buffer_resource as *mut c_void),
    };
    if ctx.frame_tx.send(frame).is_err() {
        // Consumer is gone — shut down the loop.
        tracing::info!("frame channel closed, quitting GMainLoop");
        sys::g_main_loop_quit(ctx.main_loop);
    }
    // dispatch_frame_complete tells WPE we're ready for the next
    // frame on this slot. The original buffer_resource will be
    // released when the consumer sends Cmd::Release back.
    sys::wpe_view_backend_exportable_fdo_dispatch_frame_complete(ctx.exportable);
}

unsafe extern "C" fn cb_export_buffer_resource(
    _data: *mut c_void,
    _resource: *mut sys::wl_resource,
) {
    // wl_resource buffers are the non-dmabuf shape we don't use.
}

unsafe extern "C" fn cb_export_shm_buffer(
    _data: *mut c_void,
    _buf: *mut sys::wpe_fdo_shm_exported_buffer,
) {
    // SHM buffers indicate WPE fell back to software rendering —
    // worth knowing if it happens but not actionable here.
    tracing::warn!("WPE produced SHM (software-render) buffer; expected DMA-BUF");
}

unsafe extern "C" fn cb_pump_cmds(data: *mut c_void) -> sys::gboolean {
    let ctx = &mut *(data as *mut WorkerCtx);
    loop {
        match ctx.cmd_rx.try_recv() {
            Ok(Cmd::Release { token }) => {
                sys::wpe_view_backend_exportable_fdo_dispatch_release_buffer(
                    ctx.exportable,
                    token.0 as *mut _,
                );
            }
            Ok(Cmd::Quit) => {
                sys::g_main_loop_quit(ctx.main_loop);
                return 0; /* G_SOURCE_REMOVE */
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                sys::g_main_loop_quit(ctx.main_loop);
                return 0;
            }
        }
    }
    1 /* G_SOURCE_CONTINUE */
}
