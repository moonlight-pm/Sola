//! The background picture (`Topic::Background`): a shell surface at the
//! bottom of the render list, the output's size, showing the picture
//! scaled to fill it (centre-cropped). Without one the ground is the
//! compositor's black, as before.
//!
//! The picture is decoded and scaled off the event loop (a 20 MP JPEG
//! takes a while) and kept, so `corners.rs` can paint a foreign tile's
//! corners with the picture under them ([`sample`]). Its input region is
//! empty: clicks on the ground behave as they did.

use std::os::fd::AsFd;
use std::path::PathBuf;
use std::ptr;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use tracing::{info, warn};
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool, wl_surface};

use crate::client::AppData;
use crate::protocol::river_window_management_v1::river_node_v1::RiverNodeV1;
use crate::protocol::river_window_management_v1::river_shell_surface_v1::RiverShellSurfaceV1;

#[derive(Default)]
pub struct BackgroundState {
    /// The picture the bus asked for, if any.
    want: Option<Want>,
    /// The picture decoded and scaled for the output.
    ground: Option<Ground>,
    /// A decode in flight, for this want and size.
    decoding: Option<(Want, Target, Receiver<Result<Ground, String>>)>,
    /// The last decode that failed, so it is not retried every tick.
    failed: Option<(Want, Target)>,
    /// The surface showing `ground`.
    shown: Option<Shown>,
    /// Bumped whenever `ground` changes (0: none); the corners key on it.
    generation: u64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Want {
    path: PathBuf,
    set_at: u64,
}

/// The output's size in buffer pixels, and its integer scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Target {
    w: u32,
    h: u32,
    scale: i32,
}

/// The picture at the output's size: XRGB8888 (bytes B, G, R, X).
pub struct Ground {
    want: Want,
    target: Target,
    bgrx: Vec<u8>,
}

/// The shell surface showing a ground.
struct Shown {
    generation: u64,
    position: (i32, i32),
    /// Its buffer's size and scale (the pool holds exactly that).
    w: i32,
    h: i32,
    scale: i32,
    shell_surface: RiverShellSurfaceV1,
    node: RiverNodeV1,
    surface: wl_surface::WlSurface,
    _memfd: rustix::fd::OwnedFd,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    map_ptr: *mut std::ffi::c_void,
    map_len: usize,
}

// Safety: map_ptr is only touched on the single-threaded wayland event loop.
unsafe impl Send for Shown {}

impl Drop for Shown {
    fn drop(&mut self) {
        self.node.destroy();
        self.shell_surface.destroy();
        self.surface.destroy();
        self.buffer.destroy();
        self.pool.destroy();
        if !self.map_ptr.is_null() && self.map_len > 0 {
            unsafe {
                let _ = munmap(self.map_ptr, self.map_len);
            }
        }
    }
}

/// `Topic::Background`: `Some` to show a picture, `None` (retracted) for
/// the plain ground.
pub fn set(state: &mut AppData, bg: Option<sola_bus::topics::Background>) {
    let want = bg.map(|b| Want {
        path: PathBuf::from(b.path),
        set_at: b.set_at,
    });
    if want == state.background.want {
        return;
    }
    info!(path = ?want.as_ref().map(|w| &w.path), "background");
    state.background.want = want;
    if state.background.want.is_none() {
        state.background.ground = None;
        state.background.decoding = None;
        state.background.generation += 1;
        state.pending.render_dirty = true;
    }
}

/// Run every bus tick: start a decode when the picture or the output
/// changed, and take a finished one.
pub fn poll(state: &mut AppData) {
    let Some(want) = state.background.want.clone() else {
        return;
    };
    let Some(target) = target(state) else {
        return;
    };
    if let Some((w, t, rx)) = &state.background.decoding {
        if *w == want && *t == target {
            match rx.try_recv() {
                Ok(Ok(ground)) => {
                    info!(w = target.w, h = target.h, scale = target.scale, "background ready");
                    state.background.ground = Some(ground);
                    state.background.decoding = None;
                    state.background.generation += 1;
                    state.pending.render_dirty = true;
                }
                Ok(Err(e)) => {
                    warn!(path = %want.path.display(), %e, "background failed");
                    state.background.failed = Some((want, target));
                    state.background.decoding = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => state.background.decoding = None,
            }
            return;
        }
    }
    let current = state
        .background
        .ground
        .as_ref()
        .is_some_and(|g| g.want == want && g.target == target);
    if current || state.background.failed == Some((want.clone(), target)) {
        return;
    }
    let (tx, rx) = channel();
    let job = want.clone();
    let spawned = std::thread::Builder::new()
        .name("background".into())
        .spawn(move || {
            let _ = tx.send(decode(&job, target));
        });
    match spawned {
        Ok(_) => state.background.decoding = Some((want, target, rx)),
        Err(e) => warn!(%e, "background: spawn decode"),
    }
}

/// Run during every render sequence: show the ground, or take it away.
pub fn sync_on_render(state: &mut AppData) {
    let generation = state.background.generation;
    if state.background.ground.is_none() {
        state.background.shown = None;
        return;
    }
    let position = state.output_origin.unwrap_or((0, 0));
    if let Some(shown) = &mut state.background.shown {
        if shown.generation == generation {
            if shown.position != position {
                shown.node.set_position(position.0, position.1);
                shown.position = position;
            }
            return;
        }
    }
    state.background.shown = None;
    match show(state, generation, position) {
        Ok(shown) => state.background.shown = Some(shown),
        Err(e) => warn!(%e, "background: show"),
    }
}

/// A second buffer over the shown picture's memory, for the idle fade's
/// overlay (`fade.rs`): the buffer, its size and scale. `None` without a
/// picture on screen.
pub fn overlay_buffer(state: &AppData) -> Option<(wl_buffer::WlBuffer, i32, i32, i32)> {
    let shown = state.background.shown.as_ref()?;
    let qh = state.qh.as_ref()?;
    let buffer = shown.pool.create_buffer(
        0,
        shown.w,
        shown.h,
        shown.w * 4,
        wl_shm::Format::Xrgb8888,
        qh,
        (),
    );
    Some((buffer, shown.w, shown.h, shown.scale))
}

/// Which ground the corners were painted on (0: none).
pub fn generation(state: &AppData) -> u64 {
    state
        .background
        .ground
        .as_ref()
        .map_or(0, |_| state.background.generation)
}

/// The picture's colour at global logical `(x, y)` as `[b, g, r]`; black
/// without a picture or off it.
pub fn sample(state: &AppData, x: i32, y: i32) -> [u8; 3] {
    let Some(g) = &state.background.ground else {
        return [0, 0, 0];
    };
    let (ox, oy) = state.output_origin.unwrap_or((0, 0));
    let px = (x - ox) * g.target.scale;
    let py = (y - oy) * g.target.scale;
    if px < 0 || py < 0 || px >= g.target.w as i32 || py >= g.target.h as i32 {
        return [0, 0, 0];
    }
    let i = ((py as usize) * g.target.w as usize + px as usize) * 4;
    [g.bgrx[i], g.bgrx[i + 1], g.bgrx[i + 2]]
}

/// The output in buffer pixels: its logical size times the integer scale
/// its current mode gives (1 when the mode is not a whole multiple).
fn target(state: &AppData) -> Option<Target> {
    let (lw, lh) = state.output_size?;
    if lw <= 0 || lh <= 0 {
        return None;
    }
    let mode = state.output_config.heads.values().find_map(|h| {
        let m = state.output_config.modes.get(h.current_mode.as_ref()?)?;
        h.enabled.then_some((m.width, m.height))
    });
    let scale = match mode {
        Some((mw, mh)) if mw % lw == 0 && mw / lw >= 1 && mh == lh * (mw / lw) => mw / lw,
        _ => 1,
    };
    Some(Target {
        w: (lw * scale) as u32,
        h: (lh * scale) as u32,
        scale,
    })
}

/// Decode `want`, turned upright, scaled to fill `target` and
/// centre-cropped.
fn decode(want: &Want, target: Target) -> Result<Ground, String> {
    use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType};
    let reader = ImageReader::open(&want.path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("open: {e}"))?;
    let mut decoder = reader.into_decoder().map_err(|e| format!("decoder: {e}"))?;
    let orientation = decoder.orientation().map_err(|e| format!("orientation: {e}"))?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| format!("decode: {e}"))?;
    img.apply_orientation(orientation);
    let filled = img
        .resize_to_fill(target.w, target.h, FilterType::Lanczos3)
        .to_rgb8();
    Ok(Ground {
        want: want.clone(),
        target,
        bgrx: bgrx(filled.as_raw()),
    })
}

/// RGB to XRGB8888's little-endian bytes.
fn bgrx(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for px in rgb.chunks_exact(3) {
        out.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    out
}

fn show(state: &mut AppData, generation: u64, position: (i32, i32)) -> Result<Shown, String> {
    let ground = state.background.ground.as_ref().ok_or("no ground")?;
    let wm = state.wm.clone().ok_or("window manager not bound")?;
    let compositor = state
        .shadow
        .compositor
        .clone()
        .ok_or("wl_compositor not bound")?;
    let shm = state.screenshot.shm.clone().ok_or("wl_shm not bound")?;
    let qh = state.qh.clone().ok_or("queue handle missing")?;
    let (w, h) = (ground.target.w as i32, ground.target.h as i32);
    let stride = w.checked_mul(4).ok_or("stride overflow")?;
    let size = (stride as i64)
        .checked_mul(h as i64)
        .filter(|s| *s > 0 && *s <= i32::MAX as i64)
        .ok_or("invalid buffer size")?;
    let map_len = size as usize;
    if ground.bgrx.len() != map_len {
        return Err("ground size mismatch".into());
    }
    let memfd = memfd_create("sola-background", MemfdFlags::CLOEXEC)
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
    unsafe { std::slice::from_raw_parts_mut(map_ptr as *mut u8, map_len) }
        .copy_from_slice(&ground.bgrx);
    let pool = shm.create_pool(memfd.as_fd(), size as i32, &qh, ());
    let buffer = pool.create_buffer(0, w, h, stride, wl_shm::Format::Xrgb8888, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let region = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
    let shell_surface = wm.get_shell_surface(&surface, &qh, ());
    let node = shell_surface.get_node(&qh, ());
    node.place_bottom();
    node.set_position(position.0, position.1);
    shell_surface.sync_next_commit();
    surface.set_buffer_scale(ground.target.scale);
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w, h);
    surface.commit();
    Ok(Shown {
        generation,
        position,
        w,
        h,
        scale: ground.target.scale,
        shell_surface,
        node,
        surface,
        _memfd: memfd,
        pool,
        buffer,
        map_ptr,
        map_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_becomes_little_endian_xrgb() {
        assert_eq!(bgrx(&[1, 2, 3, 4, 5, 6]), vec![3, 2, 1, 255, 6, 5, 4, 255]);
    }
}
