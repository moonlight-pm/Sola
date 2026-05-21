//! Phase-0b probe — answer the question: **can we load WPE, point it
//! at a URL, and receive DMA-BUF frames containing the rendered
//! page?**
//!
//! No wgpu, no iced, no IPC, no chrome — just the engine in
//! isolation. If WPE hands us DMA-BUFs that decode to a recognizable
//! `example.com` page, the WPE side of phase 0 is known-good and
//! 0c can focus purely on wiring those frames into the wgpu import
//! path that 0a validated.
//!
//! ## Flow
//!
//! 1. `wpe_loader_init("libWPEBackend-fdo-1.0.so")` — tell libwpe
//!    which backend implementation to use. The FDO backend gives us
//!    the exportable view-backend that produces DMA-BUFs.
//! 2. `wpe_fdo_initialize_for_egl_display(EGL_NO_DISPLAY)` — the FDO
//!    backend uses EGL internally for its own rendering surfaces;
//!    we pass `EGL_NO_DISPLAY` to let it pick the default.
//! 3. `wpe_view_backend_exportable_fdo_create(&client, ...)` — the
//!    exportable backend. `client.export_dmabuf_resource` is the
//!    callback we care about.
//! 4. Wrap as `WebKitWebViewBackend`, build a `WebKitWebView` from
//!    a default `WebKitWebContext`, `webkit_web_view_load_uri(...)`.
//! 5. `g_main_loop_run(...)` until we've collected enough frames
//!    or hit a timeout.
//!
//! In the callback: mmap the FD, swap BGRA→RGBA, write PNG.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
// The whole probe is FFI-heavy; we mark setup blocks `unsafe fn` and
// don't bother re-marking each call. The Rust 2024 lint that flags
// individual unsafe calls inside unsafe fns is noise here.
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::os::fd::{BorrowedFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::ptr;
use std::sync::Mutex;

// bindgen-generated FFI for everything WPE / WebKit / GLib.
mod wpe {
    #![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]
    include!(concat!(env!("OUT_DIR"), "/wpe_bindings.rs"));
}

const URL: &str = "https://example.com";
const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;
const FRAMES_TO_CAPTURE: u32 = 5;
const OUT_DIR: &str = "/tmp";
const TIMEOUT_SECONDS: u32 = 20;

/// State the GMainLoop callbacks read and write. Held inside a
/// `Mutex` purely for `Send + Sync` ergonomics — the GLib loop is
/// single-threaded so lock contention is impossible. Raw pointers
/// to opaque WPE/GLib types aren't Send+Sync by default; we assert
/// it manually since this binary only ever touches them from the
/// GMainLoop thread.
struct ProbeState {
    frames_seen: u32,
    exportable: *mut wpe::wpe_view_backend_exportable_fdo,
    main_loop: *mut wpe::GMainLoop,
}
unsafe impl Send for ProbeState {}
unsafe impl Sync for ProbeState {}

static STATE: Mutex<Option<ProbeState>> = Mutex::new(None);

fn main() {
    sola_core::log::init("wpe-probe");
    tracing::info!("wpe-probe starting; will load {URL} and capture {FRAMES_TO_CAPTURE} frames to {OUT_DIR}");

    unsafe { run() }
}

unsafe fn run() {
    // 1. Load the FDO backend implementation library. Use the
    //    absolute path baked in by build.rs — libwpe's dlopen
    //    can't resolve a bare name in our nix-store layout.
    let backend_so = CString::new(env!("WPE_BACKEND_FDO_SO")).unwrap();
    if !wpe::wpe_loader_init(backend_so.as_ptr()) {
        eprintln!(
            "wpe_loader_init failed for {}",
            env!("WPE_BACKEND_FDO_SO")
        );
        std::process::exit(1);
    }

    // 2. Initialize the FDO backend for EGL. We get the default EGL
    //    display via eglGetDisplay(EGL_DEFAULT_DISPLAY=NULL) — cog
    //    does the same. The dmabuf-only init path
    //    (wpe_fdo_initialize_dmabuf) trips the WebProcess into
    //    "EGL bad attribute" when it tries to wrap its render output;
    //    going through the EGL display path keeps the engine's
    //    internal pipeline coherent.
    let egl_dpy = wpe::eglGetDisplay(ptr::null_mut());
    if egl_dpy.is_null() {
        eprintln!("eglGetDisplay returned EGL_NO_DISPLAY");
        std::process::exit(1);
    }
    if !wpe::wpe_fdo_initialize_for_egl_display(egl_dpy) {
        eprintln!("wpe_fdo_initialize_for_egl_display failed");
        std::process::exit(1);
    }

    // 3. Build the exportable backend with our callbacks.
    let client = wpe::wpe_view_backend_exportable_fdo_client {
        export_buffer_resource: Some(noop_export_buffer_resource),
        export_dmabuf_resource: Some(on_export_dmabuf_resource),
        export_shm_buffer: Some(noop_export_shm_buffer),
        _wpe_reserved0: None,
        _wpe_reserved1: None,
    };
    let exportable = wpe::wpe_view_backend_exportable_fdo_create(
        &client as *const _ as *mut _,
        ptr::null_mut(),
        WIDTH,
        HEIGHT,
    );
    if exportable.is_null() {
        eprintln!("wpe_view_backend_exportable_fdo_create returned null");
        std::process::exit(1);
    }
    tracing::info!("created exportable view backend");

    let view_backend = wpe::wpe_view_backend_exportable_fdo_get_view_backend(exportable);
    if view_backend.is_null() {
        eprintln!("wpe_view_backend_exportable_fdo_get_view_backend returned null");
        std::process::exit(1);
    }

    // 4. Wrap as WebKitWebViewBackend, create the view, load the URL.
    let wk_backend = wpe::webkit_web_view_backend_new(view_backend, None, ptr::null_mut());
    let view = wpe::webkit_web_view_new(wk_backend);
    if view.is_null() {
        eprintln!("webkit_web_view_new returned null");
        std::process::exit(1);
    }
    tracing::info!("created WebKitWebView; loading {URL}");

    let url_c = CString::new(URL).unwrap();
    wpe::webkit_web_view_load_uri(view as *mut _, url_c.as_ptr());

    // 5. Run the GMainLoop until the callback decides we're done.
    let main_loop = wpe::g_main_loop_new(ptr::null_mut(), 0);
    *STATE.lock().unwrap() = Some(ProbeState {
        frames_seen: 0,
        exportable,
        main_loop,
    });

    // Safety net: bail after TIMEOUT_SECONDS even if no frames arrive.
    wpe::g_timeout_add_seconds(TIMEOUT_SECONDS, Some(on_timeout), ptr::null_mut());

    tracing::info!("entering GMainLoop");
    wpe::g_main_loop_run(main_loop);
    tracing::info!("GMainLoop exited");
}

unsafe extern "C" fn on_export_dmabuf_resource(
    _data: *mut c_void,
    res: *mut wpe::wpe_view_backend_exportable_fdo_dmabuf_resource,
) {
    let res = unsafe { &*res };
    let mut guard = STATE.lock().unwrap();
    let state = guard.as_mut().expect("STATE init");

    tracing::info!(
        frame = state.frames_seen,
        w = res.width,
        h = res.height,
        format = format!("{:#x}", res.format),
        modifier = format!("{:#x}", res.modifiers[0]),
        n_planes = res.n_planes,
        stride = res.strides[0],
        offset = res.offsets[0],
        fd = res.fds[0],
        "received DMA-BUF frame",
    );

    if let Err(e) = dump_frame_as_png(res, state.frames_seen) {
        tracing::warn!("frame dump failed: {e}");
    }

    state.frames_seen += 1;

    // Release the buffer back to WPE so it can recycle it.
    unsafe {
        wpe::wpe_view_backend_exportable_fdo_dispatch_release_buffer(
            state.exportable,
            res.buffer_resource,
        );
        wpe::wpe_view_backend_exportable_fdo_dispatch_frame_complete(state.exportable);
    }

    if state.frames_seen >= FRAMES_TO_CAPTURE {
        tracing::info!("captured {FRAMES_TO_CAPTURE} frames; quitting main loop");
        unsafe { wpe::g_main_loop_quit(state.main_loop) };
    }
}

unsafe extern "C" fn on_timeout(_user_data: *mut c_void) -> wpe::gboolean {
    let mut guard = STATE.lock().unwrap();
    let state = guard.as_mut().expect("STATE init");
    eprintln!(
        "timeout after {TIMEOUT_SECONDS}s — only saw {} frames",
        state.frames_seen
    );
    unsafe { wpe::g_main_loop_quit(state.main_loop) };
    0 /* G_SOURCE_REMOVE */
}

unsafe extern "C" fn noop_export_buffer_resource(
    _data: *mut c_void,
    _resource: *mut wpe::wl_resource,
) {
    tracing::debug!("export_buffer_resource (ignored, dmabuf path is what we want)");
}

unsafe extern "C" fn noop_export_shm_buffer(
    _data: *mut c_void,
    _buf: *mut wpe::wpe_fdo_shm_exported_buffer,
) {
    tracing::debug!("export_shm_buffer (ignored)");
}

/// mmap the DMA-BUF FD, swap BGRA→RGBA per pixel, encode PNG.
/// Only handles single-plane LINEAR-modifier buffers; tiled/multi-
/// plane will produce visibly-corrupt PNGs and we'll know.
fn dump_frame_as_png(
    res: &wpe::wpe_view_backend_exportable_fdo_dmabuf_resource,
    seq: u32,
) -> std::io::Result<()> {
    if res.n_planes != 1 {
        return Err(std::io::Error::other(format!(
            "expected 1 plane, got {}",
            res.n_planes
        )));
    }
    let fd = res.fds[0];
    if fd < 0 {
        return Err(std::io::Error::other("invalid fd"));
    }
    let stride = res.strides[0] as usize;
    let h = res.height as usize;
    let len = stride * h;

    // SAFETY: WPE owns the FD until we call dispatch_release_buffer
    // after the encode returns. mmap'd region is read-only.
    let map = unsafe {
        let ptr = libc::mmap(
            ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            0,
        );
        if ptr == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        std::slice::from_raw_parts(ptr as *const u8, len)
    };

    let mut rgba = vec![0u8; (res.width as usize * 4) * h];
    for y in 0..h {
        let src_row = &map[y * stride..y * stride + res.width as usize * 4];
        let dst_row = &mut rgba[y * res.width as usize * 4..(y + 1) * res.width as usize * 4];
        for x in 0..res.width as usize {
            // BGRA in memory → RGBA on disk.
            dst_row[x * 4] = src_row[x * 4 + 2]; // R = src.B
            dst_row[x * 4 + 1] = src_row[x * 4 + 1]; // G
            dst_row[x * 4 + 2] = src_row[x * 4]; // B = src.R
            dst_row[x * 4 + 3] = src_row[x * 4 + 3]; // A
        }
    }

    // SAFETY: same lifetime — unmap before WPE recycles the FD.
    unsafe { libc::munmap(map.as_ptr() as *mut _, len) };

    let path: PathBuf = [OUT_DIR, &format!("wpe-probe-frame-{seq:03}.png")]
        .iter()
        .collect();
    let file = std::fs::File::create(&path)?;
    let mut encoder = png::Encoder::new(file, res.width, res.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer
        .write_image_data(&rgba)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    tracing::info!(path = %path.display(), "wrote PNG");
    Ok(())
}
