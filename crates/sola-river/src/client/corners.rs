//! Rounded corners for tiled windows whose app draws no frame (Steam, any
//! foreign app; `WindowChrome.rim`), as the kit draws its own tiles: the
//! corners outside the 12 px radius painted the ground, and the 1 px rim
//! along the rounded edge (7%, 18% while focused). A tile always stands on
//! the ground, so painting the corners what lies under them rounds them:
//! the black, or the background picture's own pixels there
//! (`background::sample`), repainted when the window moves or the picture
//! changes. A float's corners have other windows behind them and stay
//! square (that needs the compositor to clip).
//!
//! Four decorations above the window (`get_decoration_above`): a strip
//! along the top and the bottom (the corners and the rim between them) and
//! a 1 px strip down each side (the rim). Their input regions are empty, so
//! clicks reach the app. River's square border is off for these windows
//! (`manage.rs`).

use std::collections::{HashMap, HashSet};
use std::os::fd::AsFd;
use std::ptr;

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use tracing::{debug, warn};
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool, wl_surface};

use crate::client::AppData;
use crate::protocol::river_window_management_v1::river_decoration_v1::RiverDecorationV1;
use crate::protocol::river_window_management_v1::river_window_v1::RiverWindowV1;

/// The kit frame's corner radius (`window_frame::RADIUS`).
const RADIUS: i32 = 12;
/// The rim's white, as the kit frame's (`window_frame::paint::colors`).
const RIM: f32 = 0.07;
const RIM_FOCUSED: f32 = 0.18;

#[derive(Default)]
pub struct CornerState {
    by_window: HashMap<u32, Corners>,
}

/// One window's corners, painted for this.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    w: i32,
    h: i32,
    focused: bool,
    /// The background picture they show (`background::generation`; 0:
    /// the black) and, on a picture, where the window is.
    ground: u64,
    at: Option<(i32, i32)>,
}

struct Corners {
    key: Key,
    pieces: Vec<Piece>,
}

/// A decoration surface and the SHM buffer it shows.
struct Piece {
    decoration: RiverDecorationV1,
    surface: wl_surface::WlSurface,
    x: i32,
    y: i32,
    memfd: rustix::fd::OwnedFd,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    map_ptr: *mut std::ffi::c_void,
    map_len: usize,
}

// Safety: map_ptr is only touched on the single-threaded wayland event loop.
unsafe impl Send for Piece {}

impl Drop for Piece {
    fn drop(&mut self) {
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
        let _ = &self.memfd;
    }
}

/// Does window `id` get rounded corners: tiled (not floating, fullscreen
/// or in Cinema) and its app draws no frame.
pub fn rounded(state: &AppData, id: u32) -> bool {
    state
        .chrome
        .get(&id)
        .is_some_and(|c| c.rim && c.tiled && !c.cinema)
        && !state.floating.contains(&id)
        && !state.currently_fullscreen.contains(&id)
}

/// Run during every render sequence: create, repaint (size or focus
/// changed) or drop each window's corners.
pub fn sync_on_render(state: &mut AppData) {
    if state.shadow.compositor.is_none() || state.screenshot.shm.is_none() || state.qh.is_none() {
        return;
    }
    let ids: Vec<u32> = state.windows_by_id.keys().copied().collect();
    let mut keep: HashSet<u32> = HashSet::new();
    for id in ids {
        if !rounded(state, id) {
            continue;
        }
        let Some((w, h)) = state.registry.get(id).and_then(|e| e.size) else {
            continue;
        };
        if w < 2 * RADIUS || h < 2 * RADIUS {
            continue;
        }
        let ground = crate::client::background::generation(state);
        let at = if ground == 0 {
            None
        } else {
            match state.registry.get(id).and_then(|e| e.position) {
                Some(p) => Some(p),
                None => continue,
            }
        };
        let key = Key {
            w,
            h,
            focused: state.focused_window == Some(id),
            ground,
            at,
        };
        keep.insert(id);
        let current = state
            .corners
            .by_window
            .get(&id)
            .is_some_and(|c| c.key == key);
        if current {
            for piece in &state.corners.by_window[&id].pieces {
                piece.decoration.set_offset(piece.x, piece.y);
            }
            continue;
        }
        state.corners.by_window.remove(&id);
        let Some(window) = state.windows_by_id.get(&id).cloned() else {
            continue;
        };
        match create(state, &window, key) {
            Ok(corners) => {
                debug!(window_id = id, w, h, focused = key.focused, ground, "corners painted");
                state.corners.by_window.insert(id, corners);
            }
            Err(e) => warn!(window_id = id, %e, "corners failed"),
        }
    }
    state.corners.by_window.retain(|id, _| keep.contains(id));
}

/// A window closed.
pub fn destroy_for(state: &mut AppData, window_id: u32) {
    state.corners.by_window.remove(&window_id);
}

/// The strips of a `w` × `h` window: `(x, y, width, height)` in its
/// coordinates.
fn strips(w: i32, h: i32) -> [(i32, i32, i32, i32); 4] {
    [
        (0, 0, w, RADIUS),
        (0, h - RADIUS, w, RADIUS),
        (0, RADIUS, 1, h - 2 * RADIUS),
        (w - 1, RADIUS, 1, h - 2 * RADIUS),
    ]
}

fn create(state: &mut AppData, window: &RiverWindowV1, key: Key) -> Result<Corners, String> {
    let Key { w, h, focused, at, .. } = key;
    let rim = if focused { RIM_FOCUSED } else { RIM };
    let mut pieces = Vec::new();
    for (x, y, pw, ph) in strips(w, h) {
        if pw <= 0 || ph <= 0 {
            continue;
        }
        let ground = |gx: i32, gy: i32| match at {
            Some((wx, wy)) => crate::client::background::sample(state, wx + gx, wy + gy),
            None => [0, 0, 0],
        };
        let mut buf = vec![0u8; (pw * ph * 4) as usize];
        paint(&mut buf, x, y, pw, ph, w, h, rim, ground);
        let piece = piece(state, window, x, y, pw, ph, |map| map.copy_from_slice(&buf))?;
        piece.decoration.set_offset(x, y);
        piece.decoration.sync_next_commit();
        piece.surface.attach(Some(&piece.buffer), 0, 0);
        piece.surface.damage_buffer(0, 0, pw, ph);
        piece.surface.commit();
        pieces.push(piece);
    }
    Ok(Corners { key, pieces })
}

/// A decoration above `window` at `(x, y)`, `pw` × `ph`, its buffer
/// painted by `fill`.
fn piece(
    state: &mut AppData,
    window: &RiverWindowV1,
    x: i32,
    y: i32,
    pw: i32,
    ph: i32,
    fill: impl FnOnce(&mut [u8]),
) -> Result<Piece, String> {
    let compositor = state
        .shadow
        .compositor
        .clone()
        .ok_or("wl_compositor not bound")?;
    let shm = state.screenshot.shm.clone().ok_or("wl_shm not bound")?;
    let qh = state.qh.clone().ok_or("queue handle missing")?;
    let stride = pw.checked_mul(4).ok_or("stride overflow")?;
    let size = (stride as i64)
        .checked_mul(ph as i64)
        .filter(|s| *s > 0 && *s <= i32::MAX as i64)
        .ok_or("invalid buffer size")?;
    let map_len = size as usize;
    let memfd = memfd_create("sola-corners", MemfdFlags::CLOEXEC)
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
    fill(unsafe { std::slice::from_raw_parts_mut(map_ptr as *mut u8, map_len) });
    let pool = shm.create_pool(memfd.as_fd(), size as i32, &qh, ());
    let buffer = pool.create_buffer(0, pw, ph, stride, wl_shm::Format::Argb8888, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let region = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
    let decoration = window.get_decoration_above(&surface, &qh, ());
    Ok(Piece {
        decoration,
        surface,
        x,
        y,
        memfd,
        pool,
        buffer,
        map_ptr,
        map_len,
    })
}

/// Paint the strip at `(ox, oy)`, `pw` × `ph`, of a `w` × `h` window:
/// the ground outside the rounded edge (`ground(x, y)` in the window's
/// coordinates, `[b, g, r]`), the rim along it inside, clear within.
/// Premultiplied ARGB8888 (bytes B, G, R, A).
#[allow(clippy::too_many_arguments)]
fn paint(
    buf: &mut [u8],
    ox: i32,
    oy: i32,
    pw: i32,
    ph: i32,
    w: i32,
    h: i32,
    rim: f32,
    ground: impl Fn(i32, i32) -> [u8; 3],
) {
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let r = RADIUS as f32;
    for y in 0..ph {
        for x in 0..pw {
            let px = (ox + x) as f32 + 0.5 - hw;
            let py = (oy + y) as f32 + 0.5 - hh;
            let d = rounded_box_sdf(px, py, hw, hh, r);
            let inside = (0.5 - d).clamp(0.0, 1.0);
            let inner = (0.5 - (d + 1.0)).clamp(0.0, 1.0);
            let band = (inside - inner) * rim;
            let alpha = (1.0 - inside) + band;
            let outside = 1.0 - inside;
            let under = if outside > 0.0 {
                ground(ox + x, oy + y)
            } else {
                [0, 0, 0]
            };
            let i = ((y * pw + x) * 4) as usize;
            for (c, g) in under.iter().enumerate() {
                let v = *g as f32 * outside + band * 255.0;
                buf[i + c] = v.round().clamp(0.0, 255.0) as u8;
            }
            buf[i + 3] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// Signed distance to a rounded box centred at the origin (negative
/// inside).
fn rounded_box_sdf(px: f32, py: f32, half_w: f32, half_h: f32, radius: f32) -> f32 {
    let dx = px.abs() - (half_w - radius);
    let dy = py.abs() - (half_h - radius);
    let (ox, oy) = (dx.max(0.0), dy.max(0.0));
    (ox * ox + oy * oy).sqrt() + dx.max(dy).min(0.0) - radius
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(buf: &[u8], pw: i32, x: i32, y: i32) -> [u8; 4] {
        let i = ((y * pw + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn the_top_strip_rounds_its_corners_and_rims_its_edge() {
        let (w, h) = (100, 80);
        let mut buf = vec![0u8; (w * RADIUS * 4) as usize];
        paint(&mut buf, 0, 0, w, RADIUS, w, h, 0.18, |_, _| [0, 0, 0]);
        // The very corner is the ground: opaque black.
        assert_eq!(pixel(&buf, w, 0, 0), [0, 0, 0, 255]);
        // The top edge between the corners is the rim: white at 18%.
        assert_eq!(pixel(&buf, w, 50, 0), [46, 46, 46, 46]);
        // Below the edge, inside: clear.
        assert_eq!(pixel(&buf, w, 50, 5), [0, 0, 0, 0]);
        // Inside the corner's curve, away from it: clear.
        assert_eq!(pixel(&buf, w, 11, 11), [0, 0, 0, 0]);
    }

    #[test]
    fn a_side_strip_is_the_rim() {
        let (w, h) = (100, 80);
        let ph = h - 2 * RADIUS;
        let mut buf = vec![0u8; (ph * 4) as usize];
        paint(&mut buf, 0, RADIUS, 1, ph, w, h, 0.07, |_, _| [0, 0, 0]);
        assert_eq!(pixel(&buf, 1, 0, 10), [18, 18, 18, 18]);
    }

    #[test]
    fn on_a_picture_the_corner_is_the_picture_under_it() {
        let (w, h) = (100, 80);
        let mut buf = vec![0u8; (w * RADIUS * 4) as usize];
        // The picture: blue at the window's own (0, 0), red elsewhere.
        let ground = |x: i32, y: i32| if (x, y) == (0, 0) { [200, 0, 0] } else { [0, 0, 90] };
        paint(&mut buf, 0, 0, w, RADIUS, w, h, 0.18, ground);
        assert_eq!(pixel(&buf, w, 0, 0), [200, 0, 0, 255]);
        assert_eq!(pixel(&buf, w, 1, 0), [0, 0, 90, 255]);
        // Inside the curve the window shows: clear.
        assert_eq!(pixel(&buf, w, 11, 11), [0, 0, 0, 0]);
    }

    #[test]
    fn strips_cover_the_edges() {
        let s = strips(100, 80);
        assert_eq!(s[1], (0, 68, 100, 12));
        assert_eq!(s[3], (99, 12, 1, 56));
    }
}
