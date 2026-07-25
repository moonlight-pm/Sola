//! Drop shadows for floating windows via `river_window_v1.get_decoration_below`.
//!
//! River only draws solid borders; soft chrome is the window manager's job.
//! For each floating window we attach a decoration surface *below* the content,
//! paint a soft rounded-rect silhouette into an SHM buffer, and offset it so
//! the blur bleeds outside the window's content rect. Geometry reporting stays
//! content-only (decorations never inflate `river_window_v1` dimensions).
//!
//! The decoration's input region is empty so the shadow never steals pointer
//! events from apps behind the float.

use std::collections::{HashMap, HashSet};
use std::os::fd::AsFd;
use std::ptr;

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use tracing::{debug, warn};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_region, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};

use crate::client::AppData;
use crate::protocol::river_window_management_v1::river_decoration_v1::RiverDecorationV1;
use crate::protocol::river_window_management_v1::river_window_v1::RiverWindowV1;

/// Extra logical pixels of shadow buffer outside each edge of the content rect.
/// Must cover blur radius + vertical offset with a little headroom.
const MARGIN: i32 = 14;

/// Soft falloff radius (logical px) for the shadow edge.
const BLUR: f32 = 11.0;

/// Downward cast of the silhouette before blur (macOS-ish).
const OFFSET_Y: f32 = 3.0;

/// Peak opacity of the shadow (premultiplied black).
const PEAK_ALPHA: f32 = 0.42;

/// Corner radius matching kit `RADIUS_XL` / floating_frame.
const CORNER_RADIUS: f32 = 14.0;

/// Cap buffer area so a pathological resize can't OOM the WM.
const MAX_PIXELS: i64 = 16_000_000;

#[derive(Default)]
pub struct ShadowState {
    pub compositor: Option<wl_compositor::WlCompositor>,
    /// Live decoration per floating window that has one.
    by_window: HashMap<u32, FloatShadow>,
}

struct FloatShadow {
    decoration: RiverDecorationV1,
    surface: wl_surface::WlSurface,
    /// Content size the current buffer was painted for.
    content_w: i32,
    content_h: i32,
    /// Keep SHM resources alive for as long as the buffer is attached.
    memfd: rustix::fd::OwnedFd,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    map_ptr: *mut std::ffi::c_void,
    map_len: usize,
}

// Safety: map_ptr is only touched on the single-threaded wayland event loop.
unsafe impl Send for FloatShadow {}

impl Drop for FloatShadow {
    fn drop(&mut self) {
        // Explicit destroy matches screenshot cleanup order; Drop of inert
        // proxies is a no-op afterward.
        self.decoration.destroy();
        self.surface.attach(None, 0, 0);
        self.surface.commit();
        self.surface.destroy();
        self.buffer.destroy();
        self.pool.destroy();
        if !self.map_ptr.is_null() && self.map_len > 0 {
            unsafe {
                let _ = munmap(self.map_ptr, self.map_len);
            }
            self.map_ptr = ptr::null_mut();
        }
        // memfd closed by OwnedFd drop
        let _ = &self.memfd;
    }
}

/// Run during every `render_start` sequence.
///
/// Creates / resizes / commits decoration-below surfaces for floating windows
/// and tears them down when a window is no longer floating (or is fullscreen).
pub fn sync_on_render(state: &mut AppData) {
    if state.shadow.compositor.is_none() || state.screenshot.shm.is_none() || state.qh.is_none() {
        return;
    }

    let floating: Vec<u32> = state.floating.iter().copied().collect();
    let mut keep: HashSet<u32> = HashSet::new();

    for window_id in floating {
        if state.currently_fullscreen.contains(&window_id) {
            continue;
        }
        let Some(proxy) = state.windows_by_id.get(&window_id).cloned() else {
            continue;
        };
        let Some((cw, ch)) = state.registry.get(window_id).and_then(|e| e.size) else {
            continue;
        };
        if cw <= 0 || ch <= 0 {
            continue;
        }

        let needs_rebuild = match state.shadow.by_window.get(&window_id) {
            None => true,
            Some(s) => s.content_w != cw || s.content_h != ch,
        };

        if needs_rebuild {
            // Drop any previous decoration before creating a new surface.
            state.shadow.by_window.remove(&window_id);
            match create_float_shadow(state, &proxy, cw, ch) {
                Ok(shadow) => {
                    debug!(window_id, cw, ch, "float shadow created");
                    // Attach in this same render sequence (offset + sync + commit).
                    shadow.decoration.set_offset(-MARGIN, -MARGIN);
                    shadow.decoration.sync_next_commit();
                    shadow.surface.attach(Some(&shadow.buffer), 0, 0);
                    shadow.surface.damage_buffer(0, 0, cw + 2 * MARGIN, ch + 2 * MARGIN);
                    shadow.surface.commit();
                    state.shadow.by_window.insert(window_id, shadow);
                    keep.insert(window_id);
                }
                Err(e) => {
                    warn!(window_id, %e, "float shadow create failed");
                }
            }
            continue;
        }

        if let Some(shadow) = state.shadow.by_window.get(&window_id) {
            // Re-assert offset each render sequence (rendering state).
            shadow.decoration.set_offset(-MARGIN, -MARGIN);
            keep.insert(window_id);
        }
    }

    // Tear down shadows for windows that left the floating set (or went
    // fullscreen). Closed windows also call `destroy_for`.
    let stale: Vec<u32> = state
        .shadow
        .by_window
        .keys()
        .copied()
        .filter(|id| !keep.contains(id))
        .collect();
    for id in stale {
        state.shadow.by_window.remove(&id);
        debug!(window_id = id, "float shadow destroyed");
    }
}

/// Drop shadow resources for a single window (closed event path).
pub fn destroy_for(state: &mut AppData, window_id: u32) {
    if state.shadow.by_window.remove(&window_id).is_some() {
        debug!(window_id, "float shadow destroyed (window closed)");
    }
}

fn create_float_shadow(
    state: &mut AppData,
    window: &RiverWindowV1,
    content_w: i32,
    content_h: i32,
) -> Result<FloatShadow, String> {
    let compositor = state
        .shadow
        .compositor
        .clone()
        .ok_or_else(|| "wl_compositor not bound".to_string())?;
    let shm = state
        .screenshot
        .shm
        .clone()
        .ok_or_else(|| "wl_shm not bound".to_string())?;
    let qh = state
        .qh
        .clone()
        .ok_or_else(|| "queue handle missing".to_string())?;

    let buf_w = content_w.checked_add(2 * MARGIN).ok_or("width overflow")?;
    let buf_h = content_h.checked_add(2 * MARGIN).ok_or("height overflow")?;
    if buf_w <= 0 || buf_h <= 0 {
        return Err("non-positive shadow buffer".into());
    }
    let pixels = (buf_w as i64).saturating_mul(buf_h as i64);
    if pixels > MAX_PIXELS {
        return Err(format!("shadow buffer too large ({pixels} px)"));
    }

    let stride = buf_w.checked_mul(4).ok_or("stride overflow")?;
    let size = (stride as i64)
        .checked_mul(buf_h as i64)
        .ok_or("size overflow")?;
    if size <= 0 || size > i32::MAX as i64 {
        return Err(format!("invalid shadow buffer size {size}"));
    }
    let size_i32 = size as i32;
    let map_len = size as usize;

    let memfd = memfd_create("sola-float-shadow", MemfdFlags::CLOEXEC)
        .map_err(|e| format!("memfd_create: {e}"))?;
    ftruncate(&memfd, size as u64).map_err(|e| format!("ftruncate: {e}"))?;

    let map_ptr = unsafe {
        mmap(
            ptr::null_mut(),
            map_len,
            ProtFlags::READ | ProtFlags::WRITE,
            MapFlags::SHARED,
            memfd.as_fd(),
            0,
        )
    }
    .map_err(|e| format!("mmap: {e}"))?;

    {
        let px = unsafe { std::slice::from_raw_parts_mut(map_ptr as *mut u8, map_len) };
        paint_shadow(px, buf_w as u32, buf_h as u32, content_w as u32, content_h as u32);
    }

    let pool = shm.create_pool(memfd.as_fd(), size_i32, &qh, ());
    let buffer = pool.create_buffer(
        0,
        buf_w,
        buf_h,
        stride,
        wl_shm::Format::Argb8888,
        &qh,
        (),
    );

    // Surface must have no role and no buffer when get_decoration_below runs.
    let surface = compositor.create_surface(&qh, ());
    // Empty input region — shadow never receives pointer events.
    let region = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();

    let decoration = window.get_decoration_below(&surface, &qh, ());

    Ok(FloatShadow {
        decoration,
        surface,
        content_w,
        content_h,
        memfd,
        pool,
        buffer,
        map_ptr,
        map_len,
    })
}

/// Paint a soft black rounded-rect cast shadow. Premultiplied ARGB8888
/// little-endian byte order is B, G, R, A.
///
/// The silhouette is a **filled** soft shape (peak alpha inside, falloff
/// outside), shifted down by [`OFFSET_Y`]. The window content is drawn
/// above this decoration and covers the centre. We deliberately do **not**
/// punch a transparent hole for the content rect: with a downward offset
/// that hole would extend past the window's bottom edge and leave a clear
/// strip where the desktop/browser bleeds through the "shadow".
fn paint_shadow(buf: &mut [u8], buf_w: u32, buf_h: u32, content_w: u32, content_h: u32) {
    let margin = MARGIN as f32;
    let half_w = content_w as f32 * 0.5;
    let half_h = content_h as f32 * 0.5;
    // Content rect centre in buffer coords, shifted down by OFFSET_Y.
    let cx = margin + half_w;
    let cy = margin + half_h + OFFSET_Y;
    let radius = CORNER_RADIUS.min(half_w).min(half_h);

    for y in 0..buf_h {
        for x in 0..buf_w {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let d = rounded_box_sdf(px - cx, py - cy, half_w, half_h, radius);
            // Filled silhouette: full peak inside, smooth falloff outside.
            let a = if d <= 0.0 {
                PEAK_ALPHA
            } else if d >= BLUR {
                0.0
            } else {
                let t = 1.0 - (d / BLUR);
                let s = t * t * (3.0 - 2.0 * t); // smoothstep
                PEAK_ALPHA * s
            };

            let i = ((y * buf_w + x) * 4) as usize;
            if i + 3 >= buf.len() {
                continue;
            }
            let ab = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
            buf[i] = 0;
            buf[i + 1] = 0;
            buf[i + 2] = 0;
            buf[i + 3] = ab;
        }
    }
}

/// Signed distance to a rounded axis-aligned box centred at the origin.
/// Negative inside, positive outside.
fn rounded_box_sdf(px: f32, py: f32, half_w: f32, half_h: f32, radius: f32) -> f32 {
    let bx = half_w - radius;
    let by = half_h - radius;
    let dx = px.abs() - bx;
    let dy = py.abs() - by;
    let ox = dx.max(0.0);
    let oy = dy.max(0.0);
    (ox * ox + oy * oy).sqrt() + dx.max(dy).min(0.0) - radius
}

// ---------- Dispatch stubs for surfaces we own ----------
// wl_shm / pool / buffer Dispatch already live in screenshot.rs.

impl Dispatch<wl_compositor::WlCompositor, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_region::WlRegion, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &wl_region::WlRegion,
        _: wl_region::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdf_negative_at_center() {
        let d = rounded_box_sdf(0.0, 0.0, 50.0, 40.0, 14.0);
        assert!(d < 0.0, "center should be inside, got {d}");
    }

    #[test]
    fn sdf_positive_outside() {
        let d = rounded_box_sdf(100.0, 0.0, 50.0, 40.0, 14.0);
        assert!(d > 0.0, "far right should be outside, got {d}");
    }

    #[test]
    fn paint_writes_alpha_under_content_not_far_corner() {
        let bw = (100 + 2 * MARGIN) as u32;
        let bh = (80 + 2 * MARGIN) as u32;
        let mut buf = vec![0u8; (bw * bh * 4) as usize];
        paint_shadow(&mut buf, bw, bh, 100, 80);
        // Far corner of buffer (outside blur) should be fully transparent.
        assert_eq!(buf[3], 0);
        // A pixel just below the content rect (with OFFSET_Y) should have alpha.
        let x = MARGIN as u32 + 50;
        let y = (MARGIN as u32 + 80) + (OFFSET_Y as u32) + 4;
        let i = ((y * bw + x) * 4) as usize;
        assert!(
            buf[i + 3] > 0,
            "expected shadow alpha under content, got {}",
            buf[i + 3]
        );
    }

    #[test]
    fn paint_fills_offset_gap_below_window() {
        // Regression: with a downward OFFSET_Y, a "ring only" paint left a
        // transparent strip under the window bottom. The filled silhouette
        // must keep peak alpha in that band so the desktop can't bleed.
        let cw = 100u32;
        let ch = 80u32;
        let bw = cw + 2 * MARGIN as u32;
        let bh = ch + 2 * MARGIN as u32;
        let mut buf = vec![0u8; (bw * bh * 4) as usize];
        paint_shadow(&mut buf, bw, bh, cw, ch);
        let x = MARGIN as u32 + cw / 2;
        // Just below the content bottom, still inside the offset silhouette.
        let y = MARGIN as u32 + ch + 1;
        let i = ((y * bw + x) * 4) as usize;
        let peak = (PEAK_ALPHA * 255.0).round() as u8;
        assert_eq!(
            buf[i + 3], peak,
            "expected filled peak alpha in OFFSET_Y gap under window"
        );
    }
}
