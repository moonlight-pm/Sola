//! Screenshot + live pixel sample via `wlr-screencopy-unstable-v1`.
//!
//! Call-plane screenshot (`compositor.screenshot`) writes a PNG, or
//! packed RGBA8 when `format=rgba` (selection freeze — no PNG encode).
//! Call-plane sample (`compositor.sample`) returns a small RGBA patch
//! around the current pointer — no disk, no PNG — for sola-scope.
//!
//! ## Why not grim / grim-rs?
//!
//! Spawning `grim` needs an external binary. `grim-rs` hardcodes 4 bpp and
//! breaks on River's 3-bpp `Bgr888`. We use the event's stride and convert
//! per-format into RGBA8 for the `png` crate.
//!
//! ## Flow
//!
//! 1. Resolve path (default `/tmp/sola/screenshots/<ms>.png`) and region.
//! 2. Full/region: `zwlr_screencopy` `capture_output[_region]` on the first
//!    `wl_output`. Window (`--app`): `ext-image-copy-capture` of that
//!    toplevel's scene — no raise, works when occluded or composition-hidden.
//! 3. On `Buffer` + `BufferDone`: allocate SHM (`memfd` + event stride),
//!    `frame.copy(buffer)`.
//! 4. On `Ready`: **copy** SHM bytes off the mmap, destroy Wayland
//!    resources, and spawn a worker thread for convert+PNG. The result is
//!    polled from `bus_tick` and completed as the call reply.
//!    On `Failed` / any error: reply `Err(msg)`.
//!
//! ## Why off-thread encode?
//!
//! A 5120×2160 capture is ~44 MB RGBA. Default (`Balanced`) PNG encode
//! was ~10 s on the desk. Shell hotkeys skip this file and Fast-encode
//! in sola-shell. `solactl` PNG still encodes here with
//! `Compression::Fast` on a worker thread. River disconnects the
//! window-management client if it is unresponsive for **>3 s**. Doing
//! encode on the calloop/Wayland thread froze the desktop.
//!
//! V1 concurrency: one screenshot (Wayland flight **or** encode worker)
//! and one live sample may run at once. A second screenshot while one
//! is running gets `Err("screenshot already in progress")`. A second
//! sample while one is in flight gets `Err("sample already in progress")`.

use std::ffi::c_void;
use std::fs::{self, File};
use std::io::BufWriter;
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use sola_bus::topics::{CaptureFormat, CaptureScreenPayload, CaptureTarget};
use tracing::{debug, info, warn};
use wayland_client::protocol::{wl_buffer, wl_output, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use crate::client::AppData;
use crate::protocol::wlr_screencopy_unstable_v1::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};
use crate::registry::Entry;

/// Globals + single-flight capture state owned by `AppData`.
#[derive(Default)]
pub struct ScreenshotState {
    pub manager: Option<ZwlrScreencopyManagerV1>,
    pub shm: Option<wl_shm::WlShm>,
    /// Bound `wl_output` globals (first is used for V1 single-output capture).
    pub outputs: Vec<wl_output::WlOutput>,
    /// At most one screenshot in flight (Wayland phase).
    pub flight: Option<CaptureFlight>,
    /// Live pixel sample (independent of screenshot). Small region; no PNG.
    pub sample: Option<CaptureFlight>,
    /// Encode-worker result channel. While `Some`, a capture is still in
    /// progress even if `flight` is already cleared.
    result_rx: Option<Receiver<Result<ShotDone, String>>>,
    /// When the request came from sola-call, complete this after encode.
    pending_reply: Option<sola_call::ReplyTx>,
    /// Sample call reply, completed on the Wayland thread (tiny buffer).
    sample_reply: Option<sola_call::ReplyTx>,
    /// Keep the foreign-toplevel list proxy alive so events keep flowing.
    pub foreign_list: Option<
        crate::protocol::ext_foreign_toplevel_list_v1::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
    >,
    /// `ext_image_copy_capture_manager_v1` — window (toplevel) capture.
    pub copy_manager: Option<
        crate::protocol::ext_image_copy_capture_v1::ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
    >,
    /// `ext_foreign_toplevel_image_capture_source_manager_v1`.
    pub toplevel_source_manager: Option<
        crate::protocol::ext_image_capture_source_v1::ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    >,
    /// Mapped toplevels from `ext_foreign_toplevel_list_v1`.
    pub toplevels: Vec<crate::client::screenshot_window::ForeignToplevel>,
    /// In-flight window capture (ext-image-copy-capture), not screencopy.
    pub window_flight: Option<crate::client::screenshot_window::WindowFlight>,
}

/// Why this screencopy frame exists.
#[derive(Clone)]
pub(crate) enum CaptureKind {
    Png {
        path: PathBuf,
    },
    /// Packed RGBA8 dump (no PNG). Selection freeze / fast picker.
    Rgba {
        path: PathBuf,
    },
    Sample {
        pointer: (i32, i32),
        origin: (i32, i32),
        hot_x: i32,
        hot_y: i32,
    },
}

/// Finished screenshot (PNG or RGBA dump) from the worker thread.
struct ShotDone {
    path: PathBuf,
    width: u32,
    height: u32,
    format: CaptureFormat,
}

#[derive(Clone, Copy)]
enum Slot {
    Shot,
    Sample,
}

/// In-flight screencopy state machine.
pub struct CaptureFlight {
    kind: CaptureKind,
    frame: ZwlrScreencopyFrameV1,
    /// Set by the `buffer` event.
    format: Option<wl_shm::Format>,
    width: u32,
    height: u32,
    stride: u32,
    /// Set by the `flags` event (before `ready`).
    y_invert: bool,
    /// SHM resources kept alive until Ready/Failed.
    memfd: Option<OwnedFd>,
    pool: Option<wl_shm_pool::WlShmPool>,
    buffer: Option<wl_buffer::WlBuffer>,
    map_ptr: Option<*mut c_void>,
    map_len: usize,
    /// True once we've called `frame.copy` (avoid double-copy).
    copied: bool,
}

// Safety: CaptureFlight is only touched from the single-threaded calloop
// Wayland dispatch path. The mmap pointer is not shared across threads.
unsafe impl Send for CaptureFlight {}

/// True while either the Wayland screencopy or the encode worker is active.
pub(crate) fn in_progress(state: &AppData) -> bool {
    state.screenshot.flight.is_some()
        || state.screenshot.window_flight.is_some()
        || state.screenshot.result_rx.is_some()
}

/// Poll the encode worker from `bus_tick` (must not run on the worker thread).
pub fn poll_results(state: &mut AppData) {
    let Some(rx) = state.screenshot.result_rx.as_ref() else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(done)) => {
            state.screenshot.result_rx = None;
            emit_ok(state, done);
        }
        Ok(Err(msg)) => {
            state.screenshot.result_rx = None;
            emit_err(state, Slot::Shot, msg);
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            state.screenshot.result_rx = None;
            emit_err(state, Slot::Shot, "screenshot encode thread died");
        }
    }
}

/// Handle a sola-call screenshot. Completes `reply` when encode finishes.
pub fn handle_call(state: &mut AppData, req: CaptureScreenPayload, reply: sola_call::ReplyTx) {
    if in_progress(state) {
        reply.err("screenshot already in progress");
        return;
    }
    state.screenshot.pending_reply = Some(reply);
    start_capture(state, req);
}

/// Odd side length, clamped. Live sample stays tiny on purpose.
const SAMPLE_SIZE_DEFAULT: i32 = 15;
const SAMPLE_SIZE_MAX: i32 = 65;

/// Handle `compositor.sample`. Completes `reply` with RGBA around the pointer.
pub fn handle_sample(state: &mut AppData, size: i32, reply: sola_call::ReplyTx) {
    if state.screenshot.sample.is_some() {
        reply.err("sample already in progress");
        return;
    }
    let Some((px, py)) = state.pointer_pos else {
        reply.err("no pointer position yet");
        return;
    };
    let Some(manager) = state.screenshot.manager.clone() else {
        reply.err("zwlr_screencopy_manager_v1 not available");
        return;
    };
    let Some(_shm) = state.screenshot.shm.clone() else {
        reply.err("wl_shm not available");
        return;
    };
    let Some(output) = state.screenshot.outputs.first().cloned() else {
        reply.err("no wl_output bound yet");
        return;
    };
    let Some(qh) = state.qh.clone() else {
        reply.err("wayland queue handle not ready");
        return;
    };
    let (ow, oh) = state.output_size.unwrap_or((1920, 1080));
    let (ox, oy) = state.output_origin.unwrap_or((0, 0));
    let rect = match sample_region(px, py, size, ox, oy, ow, oh) {
        Ok(r) => r,
        Err(e) => {
            reply.err(e);
            return;
        }
    };
    debug!(
        x = rect.x,
        y = rect.y,
        width = rect.width,
        height = rect.height,
        px,
        py,
        "sample: region around pointer"
    );
    let frame =
        manager.capture_output_region(0, &output, rect.x, rect.y, rect.width, rect.height, &qh, ());
    state.screenshot.sample_reply = Some(reply);
    state.screenshot.sample = Some(CaptureFlight {
        kind: CaptureKind::Sample {
            pointer: (px, py),
            origin: (rect.x, rect.y),
            hot_x: rect.hot_x,
            hot_y: rect.hot_y,
        },
        frame,
        format: None,
        width: 0,
        height: 0,
        stride: 0,
        y_invert: false,
        memfd: None,
        pool: None,
        buffer: None,
        map_ptr: None,
        map_len: 0,
        copied: false,
    });
    if let Some(conn) = state.conn.as_ref() {
        if let Err(e) = conn.flush() {
            warn!(%e, "wayland flush after sample start failed");
        }
    }
}

/// Captured rectangle plus the pointer's column/row inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SampleRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub hot_x: i32,
    pub hot_y: i32,
}

/// Odd `size` centered on global `(px, py)`, converted to output-local
/// and clamped to the output.
pub(crate) fn sample_region(
    px: i32,
    py: i32,
    size: i32,
    origin_x: i32,
    origin_y: i32,
    out_w: i32,
    out_h: i32,
) -> Result<SampleRect, String> {
    if out_w <= 0 || out_h <= 0 {
        return Err(format!("invalid output size: {out_w}×{out_h}"));
    }
    let mut size = size.clamp(1, SAMPLE_SIZE_MAX);
    if size % 2 == 0 {
        size = (size + 1).min(SAMPLE_SIZE_MAX);
    }
    size = size.min(out_w).min(out_h).max(1);
    // pointer_position is compositor-global; screencopy is output-local.
    let px = (px - origin_x).clamp(0, out_w - 1);
    let py = (py - origin_y).clamp(0, out_h - 1);
    let half = size / 2;
    let mut x = px - half;
    let mut y = py - half;
    if x < 0 {
        x = 0;
    }
    if y < 0 {
        y = 0;
    }
    if x + size > out_w {
        x = out_w - size;
    }
    if y + size > out_h {
        y = out_h - size;
    }
    Ok(SampleRect {
        x,
        y,
        width: size,
        height: size,
        hot_x: px - x,
        hot_y: py - y,
    })
}

pub(crate) fn clamp_sample_size(size: i32) -> i32 {
    let mut n = if size <= 0 {
        SAMPLE_SIZE_DEFAULT
    } else {
        size.clamp(1, SAMPLE_SIZE_MAX)
    };
    if n % 2 == 0 {
        n = (n + 1).min(SAMPLE_SIZE_MAX);
    }
    n
}

fn start_capture(state: &mut AppData, req: CaptureScreenPayload) {
    let Some(manager) = state.screenshot.manager.clone() else {
        emit_err(
            state,
            Slot::Shot,
            "zwlr_screencopy_manager_v1 not available",
        );
        return;
    };
    let Some(_shm) = state.screenshot.shm.clone() else {
        emit_err(state, Slot::Shot, "wl_shm not available");
        return;
    };
    let Some(output) = state.screenshot.outputs.first().cloned() else {
        emit_err(state, Slot::Shot, "no wl_output bound yet");
        return;
    };
    let Some(qh) = state.qh.clone() else {
        emit_err(state, Slot::Shot, "wayland queue handle not ready");
        return;
    };

    let path = match req.format {
        CaptureFormat::Png => resolve_path(req.path),
        CaptureFormat::Rgba => resolve_rgba_path(req.path),
    };
    let path = match path {
        Ok(p) => p,
        Err(e) => {
            emit_err(state, Slot::Shot, e);
            return;
        }
    };

    let frame = match &req.target {
        CaptureTarget::FullOutput => {
            info!(path = %path.display(), "screenshot: full output");
            manager.capture_output(0, &output, &qh, ())
        }
        CaptureTarget::Window { app_id, title } => {
            crate::client::screenshot_window::start(
                state,
                path,
                req.format,
                app_id,
                title.as_deref(),
            );
            return;
        }
        CaptureTarget::Region {
            x,
            y,
            width,
            height,
        } => {
            let (x, y, width, height) = (*x, *y, *width, *height);
            if width <= 0 || height <= 0 {
                emit_err(
                    state,
                    Slot::Shot,
                    format!("invalid region size: {width}×{height}"),
                );
                return;
            }
            info!(
                path = %path.display(),
                x,
                y,
                width,
                height,
                "screenshot: explicit region"
            );
            manager.capture_output_region(0, &output, x, y, width, height, &qh, ())
        }
    };

    let kind = match req.format {
        CaptureFormat::Png => CaptureKind::Png { path },
        CaptureFormat::Rgba => CaptureKind::Rgba { path },
    };
    state.screenshot.flight = Some(CaptureFlight {
        kind,
        frame,
        format: None,
        width: 0,
        height: 0,
        stride: 0,
        y_invert: false,
        memfd: None,
        pool: None,
        buffer: None,
        map_ptr: None,
        map_len: 0,
        copied: false,
    });

    if let Some(conn) = state.conn.as_ref() {
        if let Err(e) = conn.flush() {
            warn!(%e, "wayland flush after screencopy start failed");
        }
    }
}

/// Pick a capture rectangle for a window (output-region crop).
///
/// Kept for unit tests. Product window shots copy the toplevel scene
/// (`ext-image-copy-capture`) instead.
///
/// Live River placement (`position` + `size`) wins when both are known and
/// positive. The shell's last `Topic::Frame` is a fallback. A non-positive
/// frame is ignored so a poisoned 0×0 float restore cannot block capture.
#[allow(dead_code)]
fn capture_rect(entry: &Entry) -> Option<(i32, i32, i32, i32)> {
    if let (Some((x, y)), Some((w, h))) = (entry.position, entry.size) {
        if w > 0 && h > 0 {
            return Some((x, y, w, h));
        }
    }
    match entry.frame {
        Some((x, y, w, h)) if w > 0 && h > 0 => Some((x, y, w, h)),
        _ => None,
    }
}

fn resolve_path(path: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = match path {
        Some(p) => p,
        None => {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            PathBuf::from(format!("/tmp/sola/screenshots/{ms}.png"))
        }
    };
    ensure_parent(&path)?;
    Ok(path)
}

/// RGBA freeze dump: tmpfs first so Super+Shift+4 does not wait on disk.
fn resolve_rgba_path(path: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = match path {
        Some(p) => p,
        None => {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let shm = PathBuf::from(format!("/dev/shm/sola-freeze-{ms}.rgba"));
            if std::path::Path::new("/dev/shm").is_dir() {
                shm
            } else {
                PathBuf::from(format!("/tmp/sola/screenshots/{ms}.rgba"))
            }
        }
    };
    ensure_parent(&path)?;
    Ok(path)
}

fn ensure_parent(path: &PathBuf) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;
        }
    }
    Ok(())
}

/// Copy SHM pixels off the Wayland thread and encode PNG/RGBA.
pub(crate) fn complete_shot(
    state: &mut AppData,
    raw: Vec<u8>,
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
    y_invert: bool,
    kind: CaptureKind,
) {
    let rgba_only = matches!(&kind, CaptureKind::Rgba { .. });
    let path = match kind {
        CaptureKind::Png { path } | CaptureKind::Rgba { path } => path,
        CaptureKind::Sample { .. } => {
            emit_shot_err(state, "internal: sample passed to complete_shot");
            return;
        }
    };
    info!(
        width,
        height,
        stride,
        bytes = raw.len(),
        rgba_only,
        "screenshot: SHM copied; convert offloaded to worker"
    );
    let (tx, rx) = mpsc::channel();
    state.screenshot.result_rx = Some(rx);
    let thread_name = if rgba_only {
        "sola-screenshot-rgba"
    } else {
        "sola-screenshot-encode"
    };
    if let Err(e) = std::thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let t0 = Instant::now();
            let result = (|| {
                let rgba = pixels_to_rgba8(&raw, format, width, height, stride, y_invert)?;
                let convert_ms = t0.elapsed().as_millis();
                let t1 = Instant::now();
                if rgba_only {
                    write_rgba(&path, &rgba)?;
                } else {
                    write_png(&path, width, height, &rgba)?;
                }
                let write_ms = t1.elapsed().as_millis();
                info!(
                    path = %path.display(),
                    convert_ms,
                    write_ms,
                    rgba_only,
                    total_ms = t0.elapsed().as_millis(),
                    "screenshot: worker finished"
                );
                Ok(ShotDone {
                    path,
                    width,
                    height,
                    format: if rgba_only {
                        CaptureFormat::Rgba
                    } else {
                        CaptureFormat::Png
                    },
                })
            })();
            let _ = tx.send(result);
        })
    {
        state.screenshot.result_rx = None;
        emit_shot_err(state, format!("failed to spawn screenshot worker: {e}"));
    }
}

pub(crate) fn emit_shot_err(state: &mut AppData, msg: impl Into<String>) {
    emit_err(state, Slot::Shot, msg);
}

fn emit_ok(state: &mut AppData, done: ShotDone) {
    info!(
        path = %done.path.display(),
        width = done.width,
        height = done.height,
        ?done.format,
        "screenshot saved"
    );
    if let Some(reply) = state.screenshot.pending_reply.take() {
        match done.format {
            CaptureFormat::Png => {
                reply.ok(serde_json::json!({ "path": done.path }));
            }
            CaptureFormat::Rgba => {
                reply.ok(serde_json::json!({
                    "path": done.path,
                    "width": done.width,
                    "height": done.height,
                    "format": "rgba8",
                }));
            }
        }
    }
}

fn emit_err(state: &mut AppData, slot: Slot, msg: impl Into<String>) {
    let msg = msg.into();
    match slot {
        Slot::Shot => {
            warn!(%msg, "screenshot failed");
            if let Some(reply) = state.screenshot.pending_reply.take() {
                reply.err(msg);
            }
        }
        Slot::Sample => {
            debug!(%msg, "sample failed");
            if let Some(reply) = state.screenshot.sample_reply.take() {
                reply.err(msg);
            }
        }
    }
}

fn slot_of(state: &AppData, frame: &ZwlrScreencopyFrameV1) -> Option<Slot> {
    if state
        .screenshot
        .flight
        .as_ref()
        .is_some_and(|f| f.frame == *frame)
    {
        Some(Slot::Shot)
    } else if state
        .screenshot
        .sample
        .as_ref()
        .is_some_and(|f| f.frame == *frame)
    {
        Some(Slot::Sample)
    } else {
        None
    }
}

fn take_flight(state: &mut AppData, slot: Slot) -> &mut Option<CaptureFlight> {
    match slot {
        Slot::Shot => &mut state.screenshot.flight,
        Slot::Sample => &mut state.screenshot.sample,
    }
}

/// Tear down flight resources for `slot`.
fn clear_flight(state: &mut AppData, slot: Slot) {
    if let Some(mut flight) = take_flight(state, slot).take() {
        if let Some(ptr) = flight.map_ptr.take() {
            // Safety: ptr/len came from our mmap; only free if non-null.
            if !ptr.is_null() && flight.map_len > 0 {
                unsafe {
                    let _ = munmap(ptr, flight.map_len);
                }
            }
        }
        if let Some(buf) = flight.buffer.take() {
            buf.destroy();
        }
        if let Some(pool) = flight.pool.take() {
            pool.destroy();
        }
        flight.frame.destroy();
        // memfd dropped here
    }
}

/// After Buffer + BufferDone: allocate SHM matching the event params and copy.
fn try_copy(state: &mut AppData, slot: Slot) {
    let (format, width, height, stride) = {
        let Some(flight) = take_flight(state, slot).as_ref() else {
            return;
        };
        if flight.copied {
            return;
        }
        let Some(format) = flight.format else {
            return;
        };
        if flight.width == 0 || flight.height == 0 || flight.stride == 0 {
            return;
        }
        (format, flight.width, flight.height, flight.stride)
    };

    let Some(shm) = state.screenshot.shm.clone() else {
        clear_flight(state, slot);
        emit_err(state, slot, "wl_shm disappeared mid-capture");
        return;
    };
    let Some(qh) = state.qh.clone() else {
        clear_flight(state, slot);
        emit_err(state, slot, "queue handle missing mid-capture");
        return;
    };

    let size = (stride as u64).checked_mul(height as u64).unwrap_or(0);
    if size == 0 || size > i32::MAX as u64 {
        clear_flight(state, slot);
        emit_err(state, slot, format!("invalid buffer size {size}"));
        return;
    }
    let size_i32 = size as i32;
    let map_len = size as usize;

    let memfd = match memfd_create("sola-screencopy", MemfdFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(e) => {
            clear_flight(state, slot);
            emit_err(state, slot, format!("memfd_create failed: {e}"));
            return;
        }
    };
    if let Err(e) = ftruncate(&memfd, size) {
        clear_flight(state, slot);
        emit_err(state, slot, format!("ftruncate failed: {e}"));
        return;
    }

    let map_ptr = match unsafe {
        mmap(
            std::ptr::null_mut(),
            map_len,
            ProtFlags::READ | ProtFlags::WRITE,
            MapFlags::SHARED,
            memfd.as_fd(),
            0,
        )
    } {
        Ok(ptr) => ptr,
        Err(e) => {
            clear_flight(state, slot);
            emit_err(state, slot, format!("mmap failed: {e}"));
            return;
        }
    };

    let pool = shm.create_pool(memfd.as_fd(), size_i32, &qh, ());
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        format,
        &qh,
        (),
    );

    let Some(flight) = take_flight(state, slot).as_mut() else {
        return;
    };
    flight.memfd = Some(memfd);
    flight.pool = Some(pool);
    flight.buffer = Some(buffer.clone());
    flight.map_ptr = Some(map_ptr);
    flight.map_len = map_len;
    flight.copied = true;

    debug!(
        width,
        height,
        stride,
        ?format,
        "screencopy: copy into SHM buffer"
    );
    flight.frame.copy(&buffer);

    if let Some(conn) = state.conn.as_ref() {
        if let Err(e) = conn.flush() {
            warn!(%e, "wayland flush after screencopy copy failed");
        }
    }
}

struct ReadySnap {
    ptr: *mut c_void,
    map_len: usize,
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
    y_invert: bool,
    kind: CaptureKind,
}

fn ready_snap(state: &AppData, slot: Slot) -> Option<ReadySnap> {
    let flight = match slot {
        Slot::Shot => state.screenshot.flight.as_ref()?,
        Slot::Sample => state.screenshot.sample.as_ref()?,
    };
    let ptr = flight.map_ptr?;
    let format = flight.format?;
    Some(ReadySnap {
        ptr,
        map_len: flight.map_len,
        format,
        width: flight.width,
        height: flight.height,
        stride: flight.stride,
        y_invert: flight.y_invert,
        kind: match &flight.kind {
            CaptureKind::Png { path } => CaptureKind::Png { path: path.clone() },
            CaptureKind::Rgba { path } => CaptureKind::Rgba { path: path.clone() },
            CaptureKind::Sample {
                pointer,
                origin,
                hot_x,
                hot_y,
            } => CaptureKind::Sample {
                pointer: *pointer,
                origin: *origin,
                hot_x: *hot_x,
                hot_y: *hot_y,
            },
        },
    })
}

/// On Ready: copy SHM off the event-loop thread, free Wayland resources.
/// Screenshots hand convert+PNG to a worker (5K encode can exceed River's
/// 3s WM budget). Samples convert on this thread — the patch is tiny.
fn finalize_ready(state: &mut AppData, slot: Slot) {
    let Some(snap) = ready_snap(state, slot) else {
        clear_flight(state, slot);
        emit_err(state, slot, "screencopy ready but buffer not mapped");
        return;
    };

    // Safety: compositor has finished writing; we own the mapping until clear.
    let src = unsafe { std::slice::from_raw_parts(snap.ptr as *const u8, snap.map_len) };
    let raw = src.to_vec();
    let width = snap.width;
    let height = snap.height;
    let stride = snap.stride;
    let format = snap.format;
    let y_invert = snap.y_invert;

    let rgba_only = matches!(&snap.kind, CaptureKind::Rgba { .. });
    match snap.kind {
        CaptureKind::Sample {
            pointer,
            origin,
            hot_x,
            hot_y,
        } => {
            clear_flight(state, Slot::Sample);
            match pixels_to_rgba8(&raw, format, width, height, stride, y_invert) {
                Ok(rgba) => {
                    let hot_x = hot_x.clamp(0, width.saturating_sub(1) as i32);
                    let hot_y = hot_y.clamp(0, height.saturating_sub(1) as i32);
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&rgba);
                    if let Some(reply) = state.screenshot.sample_reply.take() {
                        reply.ok(serde_json::json!({
                            "x": pointer.0,
                            "y": pointer.1,
                            "left": origin.0,
                            "top": origin.1,
                            "width": width,
                            "height": height,
                            "hot_x": hot_x,
                            "hot_y": hot_y,
                            "pixels": b64,
                        }));
                    }
                }
                Err(e) => emit_err(state, Slot::Sample, e),
            }
        }
        CaptureKind::Png { path } | CaptureKind::Rgba { path } => {
            clear_flight(state, Slot::Shot);
            complete_shot(
                state,
                raw,
                format,
                width,
                height,
                stride,
                y_invert,
                if rgba_only {
                    CaptureKind::Rgba { path }
                } else {
                    CaptureKind::Png { path }
                },
            );
        }
    }
}

fn write_rgba(path: &PathBuf, rgba: &[u8]) -> Result<(), String> {
    fs::write(path, rgba).map_err(|e| format!("write {}: {e}", path.display()))
}

fn write_png(path: &PathBuf, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // Fast still uses Adaptive filters (~2s on 5K debug). Fastest is
    // fdeflate + Up filter.
    encoder.set_compression(png::Compression::Fastest);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png write: {e}"))?;
    Ok(())
}

/// Convert compositor SHM pixels to tightly-packed RGBA8.
///
/// Uses the event's `stride` (not `width * bpp`) so 3-bpp formats like
/// `Bgr888` work correctly. Large frames (selection freeze, 5K PNG) split
/// rows across threads so Super+Shift+4 is not stuck on a single core.
fn pixels_to_rgba8(
    src: &[u8],
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
    y_invert: bool,
) -> Result<Vec<u8>, String> {
    let w = width as usize;
    let h = height as usize;
    let stride = stride as usize;
    let mut out = vec![0u8; w * h * 4];
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8);
    if h < 64 || threads == 1 {
        convert_rows(&mut out, src, format, w, h, stride, y_invert, 0, h)?;
        return Ok(out);
    }

    let band = h.div_ceil(threads);
    let row_bytes = w * 4;
    std::thread::scope(|scope| {
        let mut rest = out.as_mut_slice();
        let mut y0 = 0;
        let mut joins = Vec::with_capacity(threads);
        for _ in 0..threads {
            if y0 >= h {
                break;
            }
            let y1 = (y0 + band).min(h);
            let (chunk, tail) = rest.split_at_mut((y1 - y0) * row_bytes);
            rest = tail;
            let start = y0;
            joins.push(scope.spawn(move || {
                convert_rows(chunk, src, format, w, h, stride, y_invert, start, y1)
            }));
            y0 = y1;
        }
        for join in joins {
            join.join()
                .map_err(|_| "screenshot convert thread panicked".to_string())??;
        }
        Ok::<(), String>(())
    })?;
    Ok(out)
}

fn convert_rows(
    dest: &mut [u8],
    src: &[u8],
    format: wl_shm::Format,
    w: usize,
    h: usize,
    stride: usize,
    y_invert: bool,
    y0: usize,
    y1: usize,
) -> Result<(), String> {
    let row_src = |y: usize| -> Result<&[u8], String> {
        let src_y = if y_invert { h - 1 - y } else { y };
        let start = src_y
            .checked_mul(stride)
            .ok_or_else(|| "row offset overflow".to_string())?;
        let end = start
            .checked_add(stride)
            .ok_or_else(|| "row end overflow".to_string())?;
        if end > src.len() {
            return Err(format!(
                "buffer too small for row {src_y}: need {end}, have {}",
                src.len()
            ));
        }
        Ok(&src[start..end])
    };

    match format {
        // Memory LE: B, G, R, A/X
        wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888 => {
            for y in y0..y1 {
                let row = row_src(y)?;
                for x in 0..w {
                    let i = x * 4;
                    if i + 3 >= row.len() {
                        return Err("row shorter than width*4".into());
                    }
                    let b = row[i];
                    let g = row[i + 1];
                    let r = row[i + 2];
                    let a = if matches!(format, wl_shm::Format::Xrgb8888) {
                        255
                    } else {
                        row[i + 3]
                    };
                    let o = ((y - y0) * w + x) * 4;
                    dest[o] = r;
                    dest[o + 1] = g;
                    dest[o + 2] = b;
                    dest[o + 3] = a;
                }
            }
        }
        // Memory LE: R, G, B, A/X
        wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888 => {
            for y in y0..y1 {
                let row = row_src(y)?;
                for x in 0..w {
                    let i = x * 4;
                    if i + 3 >= row.len() {
                        return Err("row shorter than width*4".into());
                    }
                    let r = row[i];
                    let g = row[i + 1];
                    let b = row[i + 2];
                    let a = if matches!(format, wl_shm::Format::Xbgr8888) {
                        255
                    } else {
                        row[i + 3]
                    };
                    let o = ((y - y0) * w + x) * 4;
                    dest[o] = r;
                    dest[o + 1] = g;
                    dest[o + 2] = b;
                    dest[o + 3] = a;
                }
            }
        }
        // DRM/Wayland `bgr888`: [23:0] B:G:R little-endian → **memory** R, G, B.
        // (The bitfield name is high→low; LE puts the low bits first.)
        // Earlier this arm treated memory as B,G,R and R↔B-swapped every PNG
        // (seed cyan `#00d4ff` became yellow `#ffd400`, slate `#161b22` → brown).
        // Use event stride; pack α=255.
        wl_shm::Format::Bgr888 => {
            for y in y0..y1 {
                let row = row_src(y)?;
                for x in 0..w {
                    let i = x * 3;
                    if i + 2 >= row.len() {
                        return Err("row shorter than width*3 for Bgr888".into());
                    }
                    let r = row[i];
                    let g = row[i + 1];
                    let b = row[i + 2];
                    let o = ((y - y0) * w + x) * 4;
                    dest[o] = r;
                    dest[o + 1] = g;
                    dest[o + 2] = b;
                    dest[o + 3] = 255;
                }
            }
        }
        other => {
            return Err(format!("unsupported wl_shm format: {other:?}"));
        }
    }
    Ok(())
}

// ---------- Wayland Dispatch ----------

impl Dispatch<ZwlrScreencopyManagerV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &ZwlrScreencopyManagerV1,
        _: <ZwlrScreencopyManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Manager has no events.
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for AppData {
    fn event(
        state: &mut Self,
        frame: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(slot) = slot_of(state, frame) else {
            return;
        };

        use zwlr_screencopy_frame_v1::Event;
        match event {
            Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let fmt = match format {
                    WEnum::Value(f) => f,
                    WEnum::Unknown(v) => {
                        clear_flight(state, slot);
                        emit_err(state, slot, format!("unknown wl_shm format value {v:#x}"));
                        return;
                    }
                };
                match slot {
                    Slot::Shot => {
                        info!(?fmt, width, height, stride, "screencopy buffer params");
                    }
                    Slot::Sample => {
                        debug!(?fmt, width, height, stride, "sample buffer params");
                    }
                }
                if let Some(flight) = take_flight(state, slot).as_mut() {
                    flight.format = Some(fmt);
                    flight.width = width;
                    flight.height = height;
                    flight.stride = stride;
                }
            }
            Event::BufferDone => {
                try_copy(state, slot);
            }
            Event::LinuxDmabuf { .. } => {
                // Prefer SHM path; ignore dma-buf offer.
            }
            Event::Flags { flags } => {
                let y_invert = match flags {
                    WEnum::Value(f) => f.contains(zwlr_screencopy_frame_v1::Flags::YInvert),
                    WEnum::Unknown(_) => false,
                };
                if let Some(flight) = take_flight(state, slot).as_mut() {
                    flight.y_invert = y_invert;
                }
            }
            Event::Ready { .. } => {
                finalize_ready(state, slot);
            }
            Event::Failed => {
                clear_flight(state, slot);
                emit_err(state, slot, "screencopy frame failed");
            }
            Event::Damage { .. } => {}
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Format advertisements — not needed; we use the frame's format.
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        frame: Option<(i32, i32, i32, i32)>,
        size: Option<(i32, i32)>,
        position: Option<(i32, i32)>,
    ) -> Entry {
        Entry {
            app_id: Some("app".into()),
            title: Some("t".into()),
            max_size: (0, 0),
            pid: None,
            frame,
            size,
            position,
        }
    }

    #[test]
    fn capture_rect_prefers_live_geometry_over_frame() {
        let e = entry(Some((1, 2, 3, 4)), Some((800, 600)), Some((50, 60)));
        assert_eq!(capture_rect(&e), Some((50, 60, 800, 600)));
    }

    #[test]
    fn capture_rect_falls_back_to_positive_frame() {
        let e = entry(Some((10, 20, 300, 400)), None, None);
        assert_eq!(capture_rect(&e), Some((10, 20, 300, 400)));
    }

    #[test]
    fn capture_rect_ignores_zero_frame_when_live_present() {
        let e = entry(Some((0, 0, 0, 0)), Some((1334, 2032)), Some((50, 78)));
        assert_eq!(capture_rect(&e), Some((50, 78, 1334, 2032)));
    }

    #[test]
    fn capture_rect_none_when_only_zero_frame() {
        let e = entry(Some((0, 0, 0, 0)), None, None);
        assert_eq!(capture_rect(&e), None);
    }

    #[test]
    fn bgr888_converts_to_rgba() {
        // DRM bgr888 LE memory is R,G,B (not B,G,R). One pixel R=1,G=2,B=3.
        let src = [1u8, 2, 3];
        let out = pixels_to_rgba8(&src, wl_shm::Format::Bgr888, 1, 1, 3, false).unwrap();
        assert_eq!(out, vec![1, 2, 3, 255]);
    }

    #[test]
    fn bgr888_accent_cyan_not_yellow() {
        // Seed accent #00d4ff must not become #ffd400 (the P1 baseline bug).
        let src = [0x00u8, 0xd4, 0xff];
        let out = pixels_to_rgba8(&src, wl_shm::Format::Bgr888, 1, 1, 3, false).unwrap();
        assert_eq!(out, vec![0x00, 0xd4, 0xff, 255]);
    }

    #[test]
    fn xrgb8888_swaps_and_forces_alpha() {
        // Memory: B,G,R,X
        let src = [10u8, 20, 30, 0];
        let out = pixels_to_rgba8(&src, wl_shm::Format::Xrgb8888, 1, 1, 4, false).unwrap();
        assert_eq!(out, vec![30, 20, 10, 255]);
    }

    #[test]
    fn xbgr8888_passthrough_forces_alpha() {
        // Memory: R,G,B,X
        let src = [30u8, 20, 10, 0];
        let out = pixels_to_rgba8(&src, wl_shm::Format::Xbgr8888, 1, 1, 4, false).unwrap();
        assert_eq!(out, vec![30, 20, 10, 255]);
    }

    #[test]
    fn y_invert_flips_rows() {
        // Two rows of Xrgb8888, stride 4: row0 BGRX=(1,0,0,0) → R=0; row1=(0,0,2,0) → R=2
        let src = [1u8, 0, 0, 0, 0, 0, 2, 0];
        let out = pixels_to_rgba8(&src, wl_shm::Format::Xrgb8888, 1, 2, 4, true).unwrap();
        // After invert: first out row is former last → R=2
        assert_eq!(&out[0..4], &[2, 0, 0, 255]);
        assert_eq!(&out[4..8], &[0, 0, 1, 255]);
    }

    #[test]
    fn tall_xrgb_hits_parallel_bands() {
        // h>=64 takes the threaded convert path when more than one CPU is
        // available. One unique R per row so a band split cannot swap rows.
        let h = 80u32;
        let mut src = vec![0u8; 4 * h as usize];
        for y in 0..h as usize {
            src[y * 4 + 2] = y as u8; // memory B,G,R,X → R = y
        }
        let out = pixels_to_rgba8(&src, wl_shm::Format::Xrgb8888, 1, h, 4, false).unwrap();
        assert_eq!(out.len(), 4 * h as usize);
        for y in 0..h as usize {
            assert_eq!(out[y * 4], y as u8, "row {y}");
            assert_eq!(&out[y * 4 + 1..y * 4 + 4], &[0, 0, 255]);
        }
    }

    #[test]
    fn sample_region_centers_on_pointer() {
        let r = sample_region(100, 80, 15, 0, 0, 1920, 1080).unwrap();
        assert_eq!(
            r,
            SampleRect {
                x: 93,
                y: 73,
                width: 15,
                height: 15,
                hot_x: 7,
                hot_y: 7,
            }
        );
    }

    #[test]
    fn sample_region_clamps_top_left() {
        let r = sample_region(0, 0, 15, 0, 0, 1920, 1080).unwrap();
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 0);
        assert_eq!(r.hot_x, 0);
        assert_eq!(r.hot_y, 0);
        assert_eq!(r.width, 15);
    }

    #[test]
    fn sample_region_clamps_bottom_right() {
        let r = sample_region(1919, 1079, 15, 0, 0, 1920, 1080).unwrap();
        assert_eq!(r.x, 1905);
        assert_eq!(r.y, 1065);
        assert_eq!(r.hot_x, 14);
        assert_eq!(r.hot_y, 14);
    }

    #[test]
    fn sample_region_converts_global_pointer_to_output_local() {
        let r = sample_region(1100, 180, 15, 1000, 100, 1920, 1080).unwrap();
        assert_eq!(
            r,
            SampleRect {
                x: 93,
                y: 73,
                width: 15,
                height: 15,
                hot_x: 7,
                hot_y: 7,
            }
        );
    }

    #[test]
    fn clamp_sample_size_makes_odd() {
        assert_eq!(clamp_sample_size(0), 15);
        assert_eq!(clamp_sample_size(16), 17);
        assert_eq!(clamp_sample_size(65), 65);
        assert_eq!(clamp_sample_size(200), 65);
    }
}
