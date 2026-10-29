//! The background's dim (Settings → Appearance, `Appearance.dim_*`): once
//! the idle fade is full and only the picture shows, after
//! `dim_after_min` more minutes the picture itself darkens over
//! `dim_over_min`, to [`LEFT`] of its light.
//!
//! A black layer over everything, the fade's overlay included, its
//! opacity raised by `wp_alpha_modifier_v1` as the fade's is. It covers
//! the flower and the agent islands too: kept bright, their strip would
//! be a band of picture in the dark. Input brings it back down with the
//! windows; a fullscreen window drops it with the fade.

use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use sola_bus::topics::Appearance;
use tracing::{debug, info, warn};
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool, wl_surface};

use super::{STEP, black_pool, flush, returning_level};
use crate::client::AppData;
use crate::protocol::river_window_management_v1::river_node_v1::RiverNodeV1;
use crate::protocol::river_window_management_v1::river_shell_surface_v1::RiverShellSurfaceV1;
use wayland_protocols::wp::alpha_modifier::v1::client::wp_alpha_modifier_surface_v1::WpAlphaModifierSurfaceV1;

/// What is left of the picture's light, dimmed.
const LEFT: f64 = 0.10;

/// When and how long (`Appearance.dim_*`), while the dim is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Times {
    after: Duration,
    over: Duration,
}

pub fn times(look: &Appearance) -> Option<Times> {
    let mins = |m: u16| Duration::from_secs(u64::from(m) * 60);
    look.dim_idle.then(|| Times {
        after: mins(look.dim_after_min),
        over: mins(look.dim_over_min.max(1)),
    })
}

/// The black over the picture.
pub struct Dimmer {
    surface: wl_surface::WlSurface,
    shell_surface: RiverShellSurfaceV1,
    node: RiverNodeV1,
    opacity: WpAlphaModifierSurfaceV1,
    buffer: wl_buffer::WlBuffer,
    /// The buffer's memory (zeros: black).
    pool: (OwnedFd, wl_shm_pool::WlShmPool),
    size: (i32, i32),
    level: u32,
    set_at: Instant,
    /// Input came back: the level the return started from.
    from: u32,
}

impl Dimmer {
    fn set(&mut self, level: u32) {
        self.opacity.set_multiplier(level);
        self.surface.commit();
        self.level = level;
        self.set_at = Instant::now();
    }
}

impl Drop for Dimmer {
    fn drop(&mut self) {
        self.opacity.destroy();
        self.node.destroy();
        self.shell_surface.destroy();
        self.surface.destroy();
        self.buffer.destroy();
        self.pool.1.destroy();
    }
}

/// The black's opacity `full` after the fade became full: none for
/// `after`, then rising over `over` until [`LEFT`] of the picture shows.
fn level(full: Duration, t: Times) -> u32 {
    let Some(into) = full.checked_sub(t.after) else {
        return 0;
    };
    let f = (into.as_secs_f64() / t.over.as_secs_f64().max(1.0)).clamp(0.0, 1.0);
    (f * (1.0 - LEFT) * f64::from(u32::MAX)) as u32
}

/// Where the dim should be now: 0 when off, not full yet, or returning.
fn target(state: &AppData) -> u32 {
    let fade = &state.fade;
    match (fade.dim, fade.full_since) {
        (Some(t), Some(full)) if fade.returning.is_none() => level(full.elapsed(), t),
        _ => 0,
    }
}

/// Each bus tick while the fade is full: raise the black as the dim goes
/// (made in the next render sequence, on top).
pub(super) fn tick(state: &mut AppData) {
    let target = target(state);
    if target == 0 {
        // Settings moved the dim later (or off): it lifts at once.
        if state.fade.dimmer.take().is_some() {
            flush(state);
        }
        return;
    }
    match &mut state.fade.dimmer {
        None if !state.fade.dim_failed => state.pending.render_dirty = true,
        None => {}
        Some(d) if d.level != target && d.set_at.elapsed() >= STEP => {
            d.set(target);
            flush(state);
        }
        Some(_) => {}
    }
}

/// Input: the black comes back down from where it is, with the windows.
pub(super) fn start_return(state: &mut AppData) {
    if let Some(d) = &mut state.fade.dimmer {
        d.from = d.level;
    }
}

/// `t` (0–1) into the return.
pub(super) fn returning(state: &mut AppData, t: f64) {
    if let Some(d) = &mut state.fade.dimmer {
        d.set(returning_level(d.from, 0, t));
    }
}

/// Last in every render sequence, after the fade's overlay: keep the
/// black on top, make it when the dim has begun, drop it with the
/// overlay.
pub(super) fn sync(state: &mut AppData) {
    if state.fade.overlay.is_none() {
        state.fade.dimmer = None;
        return;
    }
    if let Some(d) = &state.fade.dimmer {
        if Some(d.size) == state.output_size {
            d.node.place_top();
            return;
        }
    }
    let level = match &state.fade.dimmer {
        Some(d) => d.level,
        None => target(state),
    };
    if level == 0 || state.fade.dim_failed {
        return;
    }
    match make(state, level) {
        Ok(d) => {
            if state.fade.dimmer.is_none() {
                info!("idle: the background dims");
            }
            debug!(level, "idle dim layer");
            state.fade.dimmer = Some(d);
        }
        Err(e) => {
            warn!(%e, "idle dim layer");
            state.fade.dim_failed = true;
        }
    }
}

fn make(state: &mut AppData, level: u32) -> Result<Dimmer, String> {
    let wm = state.wm.clone().ok_or("window manager not bound")?;
    let alpha = state.fade.alpha.clone().ok_or("wp_alpha_modifier_v1 not bound")?;
    let compositor = state
        .shadow
        .compositor
        .clone()
        .ok_or("wl_compositor not bound")?;
    let qh = state.qh.clone().ok_or("queue handle missing")?;
    let (w, h) = state.output_size.ok_or("output size unknown")?;
    let pool = black_pool(state, w, h)?;
    let buffer = pool.1.create_buffer(0, w, h, w * 4, wl_shm::Format::Xrgb8888, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let region = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
    let opacity = alpha.get_surface(&surface, &qh, ());
    opacity.set_multiplier(level);
    let shell_surface = wm.get_shell_surface(&surface, &qh, ());
    let node = shell_surface.get_node(&qh, ());
    node.place_top();
    let (x, y) = state.output_origin.unwrap_or((0, 0));
    node.set_position(x, y);
    shell_surface.sync_next_commit();
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w, h);
    surface.commit();
    Ok(Dimmer {
        surface,
        shell_surface,
        node,
        opacity,
        buffer,
        pool,
        size: (w, h),
        level,
        set_at: Instant::now(),
        from: level,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dim_waits_then_leaves_a_tenth() {
        let t = times(&Appearance::default()).unwrap();
        assert_eq!(t.after, Duration::from_secs(60 * 60));
        assert_eq!(t.over, Duration::from_secs(30 * 60));
        assert_eq!(level(Duration::ZERO, t), 0);
        assert_eq!(level(t.after, t), 0);
        let half = level(t.after + t.over / 2, t) as f64 / f64::from(u32::MAX);
        assert!((half - 0.45).abs() < 0.001, "{half}");
        let done = level(t.after + t.over * 3, t) as f64 / f64::from(u32::MAX);
        assert!((done - 0.9).abs() < 0.001, "{done}");
        let off = Appearance {
            dim_idle: false,
            ..Appearance::default()
        };
        assert_eq!(times(&off), None);
    }
}
