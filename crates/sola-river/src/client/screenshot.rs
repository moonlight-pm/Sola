//! Screenshot capture via `wlr-screencopy-unstable-v1`.
//!
//! Call-plane screenshot (`compositor.screenshot`). This module runs the
//! compositor-side capture, writes a PNG, and completes the call reply.
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
//! 2. `manager.capture_output[_region](…)` on the first `wl_output`.
//! 3. On `Buffer` + `BufferDone`: allocate SHM (`memfd` + event stride),
//!    `frame.copy(buffer)`.
//! 4. On `Ready`: **copy** SHM bytes off the mmap, destroy Wayland
//!    resources, and spawn a worker thread for convert+PNG. The result is
//!    polled from `bus_tick` and completed as the call reply.
//!    On `Failed` / any error: reply `Err(msg)`.
//!
//! ## Why off-thread encode?
//!
//! A 5120×2160 capture is ~44 MB RGBA. Convert + `png` encode can take
//! multiple seconds on the CPU. River disconnects the window-management
//! client if it is unresponsive for **>3 s** (`window manager unresponsive
//! … disconnecting`). Doing encode on the calloop/Wayland thread froze the
//! desktop and broke clients (terminals lost input / Broken pipe).
//!
//! V1 concurrency: a single in-flight capture (Wayland flight **or** encode
//! worker). A second request while one is running gets
//! `Err("screenshot already in progress")`.

use std::ffi::c_void;
use std::fs::{self, File};
use std::io::BufWriter;
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use sola_bus::topics::{CaptureScreenPayload, CaptureTarget};
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
    /// At most one capture in flight (Wayland phase).
    pub flight: Option<CaptureFlight>,
    /// Encode-worker result channel. While `Some`, a capture is still in
    /// progress even if `flight` is already cleared.
    result_rx: Option<Receiver<Result<PathBuf, String>>>,
    /// When the request came from sola-call, complete this after encode.
    pending_reply: Option<sola_call::ReplyTx>,
}

/// In-flight screencopy state machine.
pub struct CaptureFlight {
    path: PathBuf,
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
fn in_progress(state: &AppData) -> bool {
    state.screenshot.flight.is_some() || state.screenshot.result_rx.is_some()
}

/// Poll the encode worker from `bus_tick` (must not run on the worker thread).
pub fn poll_results(state: &mut AppData) {
    let Some(rx) = state.screenshot.result_rx.as_ref() else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(path)) => {
            state.screenshot.result_rx = None;
            emit_ok(state, path);
        }
        Ok(Err(msg)) => {
            state.screenshot.result_rx = None;
            emit_err(state, msg);
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            state.screenshot.result_rx = None;
            emit_err(state, "screenshot encode thread died");
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

fn start_capture(state: &mut AppData, req: CaptureScreenPayload) {
    let Some(manager) = state.screenshot.manager.clone() else {
        emit_err(state, "zwlr_screencopy_manager_v1 not available");
        return;
    };
    let Some(_shm) = state.screenshot.shm.clone() else {
        emit_err(state, "wl_shm not available");
        return;
    };
    let Some(output) = state.screenshot.outputs.first().cloned() else {
        emit_err(state, "no wl_output bound yet");
        return;
    };
    let Some(qh) = state.qh.clone() else {
        emit_err(state, "wayland queue handle not ready");
        return;
    };

    let path = match resolve_path(req.path) {
        Ok(p) => p,
        Err(e) => {
            emit_err(state, e);
            return;
        }
    };

    let frame = match &req.target {
        CaptureTarget::FullOutput => {
            info!(path = %path.display(), "screenshot: full output");
            manager.capture_output(0, &output, &qh, ())
        }
        CaptureTarget::Window { app_id, title } => {
            let Some(entry) = state.registry.find_by_app_title(app_id, title.as_deref()) else {
                emit_err(
                    state,
                    format!("window not found: app_id={app_id} title={title:?}"),
                );
                return;
            };
            // Prefer live size+position (what River actually placed) over the
            // shell's last `Topic::Frame`. Floating windows are intentionally
            // not re-framed after move/resize, and a poisoned 0×0 Frame
            // (Float zone rect / bad FloatGeometry restore) would otherwise
            // make region capture fail while the window is still visible.
            let Some((x, y, w, h)) = capture_rect(entry) else {
                emit_err(
                    state,
                    "window has no usable geometry yet (no live size/position and no frame)",
                );
                return;
            };
            // Region is screen content at that rect, including overlaps.
            info!(
                path = %path.display(),
                %app_id,
                ?title,
                x,
                y,
                w,
                h,
                frame = ?entry.frame,
                size = ?entry.size,
                position = ?entry.position,
                "screenshot: window region (screen content at rect)"
            );
            manager.capture_output_region(0, &output, x, y, w, h, &qh, ())
        }
        CaptureTarget::Region {
            x,
            y,
            width,
            height,
        } => {
            let (x, y, width, height) = (*x, *y, *width, *height);
            if width <= 0 || height <= 0 {
                emit_err(state, format!("invalid region size: {width}×{height}"));
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

    state.screenshot.flight = Some(CaptureFlight {
        path,
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

/// Pick a capture rectangle for a window.
///
/// Live River placement (`position` + `size`) wins when both are known and
/// positive — that is what the compositor actually drew. The shell's last
/// `Topic::Frame` is a fallback for windows that have been framed but not
/// yet reported dimensions (first paint). A non-positive frame is ignored so
/// a poisoned 0×0 float restore cannot block capture of a live window.
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;
    }
    Ok(path)
}

fn emit_ok(state: &mut AppData, path: PathBuf) {
    info!(path = %path.display(), "screenshot saved");
    if let Some(reply) = state.screenshot.pending_reply.take() {
        reply.ok(serde_json::json!({ "path": path }));
    }
}

fn emit_err(state: &mut AppData, msg: impl Into<String>) {
    let msg = msg.into();
    warn!(%msg, "screenshot failed");
    if let Some(reply) = state.screenshot.pending_reply.take() {
        reply.err(msg);
    }
}

/// Tear down flight resources and clear `state.screenshot.flight`.
fn clear_flight(state: &mut AppData) {
    if let Some(mut flight) = state.screenshot.flight.take() {
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
fn try_copy(state: &mut AppData) {
    let Some(flight) = state.screenshot.flight.as_mut() else {
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

    let Some(shm) = state.screenshot.shm.clone() else {
        clear_flight(state);
        emit_err(state, "wl_shm disappeared mid-capture");
        return;
    };
    let Some(qh) = state.qh.clone() else {
        clear_flight(state);
        emit_err(state, "queue handle missing mid-capture");
        return;
    };

    let size = (flight.stride as u64)
        .checked_mul(flight.height as u64)
        .unwrap_or(0);
    if size == 0 || size > i32::MAX as u64 {
        clear_flight(state);
        emit_err(state, format!("invalid buffer size {size}"));
        return;
    }
    let size_i32 = size as i32;
    let map_len = size as usize;

    let memfd = match memfd_create("sola-screencopy", MemfdFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(e) => {
            clear_flight(state);
            emit_err(state, format!("memfd_create failed: {e}"));
            return;
        }
    };
    if let Err(e) = ftruncate(&memfd, size) {
        clear_flight(state);
        emit_err(state, format!("ftruncate failed: {e}"));
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
            clear_flight(state);
            emit_err(state, format!("mmap failed: {e}"));
            return;
        }
    };

    let pool = shm.create_pool(memfd.as_fd(), size_i32, &qh, ());
    let buffer = pool.create_buffer(
        0,
        flight.width as i32,
        flight.height as i32,
        flight.stride as i32,
        format,
        &qh,
        (),
    );

    flight.memfd = Some(memfd);
    flight.pool = Some(pool);
    flight.buffer = Some(buffer.clone());
    flight.map_ptr = Some(map_ptr);
    flight.map_len = map_len;
    flight.copied = true;

    debug!(
        width = flight.width,
        height = flight.height,
        stride = flight.stride,
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

/// On Ready: copy SHM off the event-loop thread, free Wayland resources,
/// and hand convert+PNG to a worker. River kills the WM client if we block
/// the calloop thread for >3s — encode of a 5K buffer routinely exceeds that.
fn finalize_ready(state: &mut AppData) {
    let Some(flight) = state.screenshot.flight.as_ref() else {
        return;
    };

    let (Some(ptr), Some(format)) = (flight.map_ptr, flight.format) else {
        clear_flight(state);
        emit_err(state, "screenshot ready but buffer not mapped");
        return;
    };

    let width = flight.width;
    let height = flight.height;
    let stride = flight.stride;
    let y_invert = flight.y_invert;
    let path = flight.path.clone();
    let map_len = flight.map_len;

    // Safety: compositor has finished writing; we own the mapping until clear.
    let src = unsafe { std::slice::from_raw_parts(ptr as *const u8, map_len) };

    // Copy only — keep this under River's WM responsiveness budget.
    let t_copy = Instant::now();
    let raw = src.to_vec();
    let copy_ms = t_copy.elapsed().as_millis();
    info!(
        width,
        height,
        stride,
        bytes = raw.len(),
        copy_ms,
        "screenshot: SHM copied; encode offloaded to worker"
    );

    // Drop Wayland proxies / munmap before starting the slow work.
    clear_flight(state);

    let (tx, rx) = mpsc::channel();
    state.screenshot.result_rx = Some(rx);

    if let Err(e) = std::thread::Builder::new()
        .name("sola-screenshot-encode".into())
        .spawn(move || {
            let t0 = Instant::now();
            let result = (|| {
                let rgba = pixels_to_rgba8(&raw, format, width, height, stride, y_invert)?;
                let convert_ms = t0.elapsed().as_millis();
                let t1 = Instant::now();
                write_png(&path, width, height, &rgba)?;
                let encode_ms = t1.elapsed().as_millis();
                info!(
                    path = %path.display(),
                    convert_ms,
                    encode_ms,
                    total_ms = t0.elapsed().as_millis(),
                    "screenshot: encode worker finished"
                );
                Ok(path)
            })();
            // If the main loop dropped the receiver (shutdown), ignore.
            let _ = tx.send(result);
        })
    {
        state.screenshot.result_rx = None;
        emit_err(
            state,
            format!("failed to spawn screenshot encode thread: {e}"),
        );
    }
}

fn write_png(path: &PathBuf, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
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
/// `Bgr888` work correctly.
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
            for y in 0..h {
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
                    let o = (y * w + x) * 4;
                    out[o] = r;
                    out[o + 1] = g;
                    out[o + 2] = b;
                    out[o + 3] = a;
                }
            }
        }
        // Memory LE: R, G, B, A/X
        wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888 => {
            for y in 0..h {
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
                    let o = (y * w + x) * 4;
                    out[o] = r;
                    out[o + 1] = g;
                    out[o + 2] = b;
                    out[o + 3] = a;
                }
            }
        }
        // DRM/Wayland `bgr888`: [23:0] B:G:R little-endian → **memory** R, G, B.
        // (The bitfield name is high→low; LE puts the low bits first.)
        // Earlier this arm treated memory as B,G,R and R↔B-swapped every PNG
        // (seed cyan `#00d4ff` became yellow `#ffd400`, slate `#161b22` → brown).
        // Use event stride; pack α=255.
        wl_shm::Format::Bgr888 => {
            for y in 0..h {
                let row = row_src(y)?;
                for x in 0..w {
                    let i = x * 3;
                    if i + 2 >= row.len() {
                        return Err("row shorter than width*3 for Bgr888".into());
                    }
                    let r = row[i];
                    let g = row[i + 1];
                    let b = row[i + 2];
                    let o = (y * w + x) * 4;
                    out[o] = r;
                    out[o + 1] = g;
                    out[o + 2] = b;
                    out[o + 3] = 255;
                }
            }
        }
        other => {
            return Err(format!("unsupported wl_shm format: {other:?}"));
        }
    }

    Ok(out)
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
        // Ignore events for a stale frame (shouldn't happen with single-flight).
        let is_ours = state
            .screenshot
            .flight
            .as_ref()
            .map(|f| f.frame == *frame)
            .unwrap_or(false);
        if !is_ours {
            return;
        }

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
                        clear_flight(state);
                        emit_err(state, format!("unknown wl_shm format value {v:#x}"));
                        return;
                    }
                };
                // info: channel-order bugs only show up with the live format;
                // keep this visible without RUST_LOG=debug.
                info!(?fmt, width, height, stride, "screencopy buffer params");
                if let Some(flight) = state.screenshot.flight.as_mut() {
                    flight.format = Some(fmt);
                    flight.width = width;
                    flight.height = height;
                    flight.stride = stride;
                }
            }
            Event::BufferDone => {
                try_copy(state);
            }
            Event::LinuxDmabuf { .. } => {
                // Prefer SHM path; ignore dma-buf offer.
            }
            Event::Flags { flags } => {
                let y_invert = match flags {
                    WEnum::Value(f) => f.contains(zwlr_screencopy_frame_v1::Flags::YInvert),
                    WEnum::Unknown(_) => false,
                };
                if let Some(flight) = state.screenshot.flight.as_mut() {
                    flight.y_invert = y_invert;
                }
            }
            Event::Ready { .. } => {
                finalize_ready(state);
            }
            Event::Failed => {
                clear_flight(state);
                emit_err(state, "screencopy frame failed");
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
}
