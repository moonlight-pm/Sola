//! The idle fade (Settings → Appearance, `Appearance.fade_idle`): after
//! `fade_after_min` minutes with no input, the windows fade into the
//! background over [`Appearance::FADE_MINUTES`]; any input brings them
//! back at once.
//!
//! Fading every window toward the picture looks exactly like the picture
//! rising over every window (`a·window + (1−a)·picture` either way), so
//! that is what is drawn: a shell surface at the top of the render list
//! showing the picture (the background's own memory; black without a
//! picture), its opacity raised by `wp_alpha_modifier_v1` — a number per
//! step, no repaint. Its input region is empty.
//!
//! Idle comes from `ext_idle_notifier_v1.get_idle_notification`, which
//! honors idle inhibitors (a video player keeping the screen awake); and
//! no fade runs while a window is fullscreen (Cinema).

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use sola_bus::topics::Appearance;
use tracing::{debug, info, warn};
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use wayland_protocols::wp::alpha_modifier::v1::client::wp_alpha_modifier_surface_v1::WpAlphaModifierSurfaceV1;
use wayland_protocols::wp::alpha_modifier::v1::client::wp_alpha_modifier_v1::WpAlphaModifierV1;

use crate::client::AppData;
use crate::protocol::river_window_management_v1::river_node_v1::RiverNodeV1;
use crate::protocol::river_window_management_v1::river_shell_surface_v1::RiverShellSurfaceV1;

/// The opacity changes at most this often (a step of 1/1800 of the way).
const STEP: Duration = Duration::from_secs(1);

pub struct FadeState {
    /// `ext_idle_notifier_v1`, `wp_alpha_modifier_v1`.
    pub notifier: Option<ExtIdleNotifierV1>,
    pub alpha: Option<WpAlphaModifierV1>,
    /// The idle delay asked for (ms); `None` while the fade is off.
    wanted: Option<u32>,
    /// The notification for `wanted`, and its delay.
    notification: Option<(ExtIdleNotificationV1, u32)>,
    /// When the fade began (idle, and no fullscreen window).
    since: Option<Instant>,
    overlay: Option<Overlay>,
    /// Making the overlay failed this idle: stop asking for render passes.
    failed: bool,
}

impl Default for FadeState {
    /// The fade as `Appearance::default()` has it, until the saved
    /// settings replay (none saved: the default stands).
    fn default() -> Self {
        let look = Appearance::default();
        Self {
            notifier: None,
            alpha: None,
            wanted: look
                .fade_idle
                .then(|| u32::from(look.fade_after_min) * 60_000),
            notification: None,
            since: None,
            overlay: None,
            failed: false,
        }
    }
}

/// The picture over everything, at some opacity.
struct Overlay {
    shell_surface: RiverShellSurfaceV1,
    node: RiverNodeV1,
    surface: wl_surface::WlSurface,
    opacity: WpAlphaModifierSurfaceV1,
    buffer: wl_buffer::WlBuffer,
    /// A black buffer's own memory (none over the picture's).
    black: Option<(rustix::fd::OwnedFd, wl_shm_pool::WlShmPool)>,
    multiplier: u32,
    set_at: Instant,
}

impl Drop for Overlay {
    fn drop(&mut self) {
        self.opacity.destroy();
        self.node.destroy();
        self.shell_surface.destroy();
        self.surface.destroy();
        self.buffer.destroy();
        if let Some((_, pool)) = &self.black {
            pool.destroy();
        }
    }
}

/// `Topic::Appearance` (`None`: retracted, the defaults).
pub fn settings(state: &mut AppData, look: Option<&Appearance>) {
    let look = look.cloned().unwrap_or_default();
    let wanted = look
        .fade_idle
        .then(|| u32::from(look.fade_after_min.max(1)) * 60_000);
    if wanted == state.fade.wanted {
        return;
    }
    info!(after_ms = ?wanted, "idle fade");
    state.fade.wanted = wanted;
    if let Some((n, _)) = state.fade.notification.take() {
        n.destroy();
    }
    wake(state);
    ensure(state);
}

/// Run every bus tick: ask for the idle notification, and raise the
/// overlay's opacity as the fade goes.
pub fn tick(state: &mut AppData) {
    ensure(state);
    let Some(since) = state.fade.since else {
        return;
    };
    if !state.currently_fullscreen.is_empty() {
        // A fullscreen window (a film): no fade; it starts over after.
        if state.fade.overlay.is_some() {
            drop_overlay(state);
        }
        state.fade.since = Some(Instant::now());
        return;
    }
    let target = multiplier(since.elapsed());
    let Some(overlay) = &mut state.fade.overlay else {
        // Made in the next render sequence (it must be placed on top).
        if !state.fade.failed {
            state.pending.render_dirty = true;
        }
        return;
    };
    if overlay.multiplier == target || overlay.set_at.elapsed() < STEP {
        return;
    }
    overlay.opacity.set_multiplier(target);
    overlay.surface.commit();
    overlay.multiplier = target;
    overlay.set_at = Instant::now();
    if let Some(conn) = &state.conn {
        let _ = conn.flush();
    }
}

/// Run during every render sequence, last: make the overlay when the fade
/// needs it, and keep it on top.
pub fn sync_on_render(state: &mut AppData) {
    if state.fade.since.is_none() || state.fade.wanted.is_none() {
        state.fade.overlay = None;
        return;
    }
    if let Some(overlay) = &state.fade.overlay {
        overlay.node.place_top();
        return;
    }
    if !state.currently_fullscreen.is_empty() || state.fade.failed {
        return;
    }
    match make_overlay(state) {
        Ok(overlay) => {
            debug!(multiplier = overlay.multiplier, "idle fade overlay");
            state.fade.overlay = Some(overlay);
        }
        Err(e) => {
            warn!(%e, "idle fade overlay");
            state.fade.failed = true;
        }
    }
}

/// The opacity `elapsed` into the fade.
fn multiplier(elapsed: Duration) -> u32 {
    let total = f64::from(Appearance::FADE_MINUTES) * 60.0;
    let t = (elapsed.as_secs_f64() / total).clamp(0.0, 1.0);
    (t * f64::from(u32::MAX)) as u32
}

/// Ask for the idle notification when the fade is on and it is missing.
fn ensure(state: &mut AppData) {
    let Some(timeout) = state.fade.wanted else {
        return;
    };
    if state.fade.notification.is_some() {
        return;
    }
    let (Some(notifier), Some(seat), Some(qh)) = (
        state.fade.notifier.as_ref(),
        state.wl_seat.as_ref(),
        state.qh.as_ref(),
    ) else {
        return;
    };
    let n = notifier.get_idle_notification(timeout, seat, qh, ());
    state.fade.notification = Some((n, timeout));
}

/// Input (or the fade turned off): the windows back at once.
fn wake(state: &mut AppData) {
    state.fade.since = None;
    state.fade.failed = false;
    drop_overlay(state);
}

fn drop_overlay(state: &mut AppData) {
    let Some(overlay) = state.fade.overlay.take() else {
        return;
    };
    overlay.opacity.set_multiplier(0);
    overlay.surface.commit();
    drop(overlay);
    if let Some(conn) = &state.conn {
        let _ = conn.flush();
    }
}

fn make_overlay(state: &mut AppData) -> Result<Overlay, String> {
    let wm = state.wm.clone().ok_or("window manager not bound")?;
    let alpha = state.fade.alpha.clone().ok_or("wp_alpha_modifier_v1 not bound")?;
    let compositor = state
        .shadow
        .compositor
        .clone()
        .ok_or("wl_compositor not bound")?;
    let qh = state.qh.clone().ok_or("queue handle missing")?;
    let (buffer, w, h, scale, black) = match crate::client::background::overlay_buffer(state) {
        Some((buffer, w, h, scale)) => (buffer, w, h, scale, None),
        None => {
            let (w, h) = state.output_size.ok_or("output size unknown")?;
            let shm = state.screenshot.shm.clone().ok_or("wl_shm not bound")?;
            let size = w.checked_mul(h).and_then(|p| p.checked_mul(4)).ok_or("size")?;
            // A fresh memfd reads as zeros: black.
            let memfd = memfd_create("sola-fade", MemfdFlags::CLOEXEC)
                .map_err(|e| format!("memfd_create: {e}"))?;
            ftruncate(&memfd, size as u64).map_err(|e| format!("ftruncate: {e}"))?;
            let pool = shm.create_pool(memfd.as_fd(), size, &qh, ());
            let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Xrgb8888, &qh, ());
            (buffer, w, h, 1, Some((memfd, pool)))
        }
    };
    let surface = compositor.create_surface(&qh, ());
    let region = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
    let opacity = alpha.get_surface(&surface, &qh, ());
    let start = state.fade.since.map_or(0, |s| multiplier(s.elapsed()));
    opacity.set_multiplier(start);
    let shell_surface = wm.get_shell_surface(&surface, &qh, ());
    let node = shell_surface.get_node(&qh, ());
    node.place_top();
    let (x, y) = state.output_origin.unwrap_or((0, 0));
    node.set_position(x, y);
    shell_surface.sync_next_commit();
    surface.set_buffer_scale(scale);
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w, h);
    surface.commit();
    Ok(Overlay {
        shell_surface,
        node,
        surface,
        opacity,
        buffer,
        black,
        multiplier: start,
        set_at: Instant::now(),
    })
}

impl Dispatch<ExtIdleNotificationV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => {
                info!("idle: the fade starts");
                state.fade.since = Some(Instant::now());
                state.pending.render_dirty = true;
            }
            ext_idle_notification_v1::Event::Resumed => {
                if state.fade.since.is_some() {
                    info!("input: the windows back");
                }
                wake(state);
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpAlphaModifierV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &WpAlphaModifierV1,
        _: <WpAlphaModifierV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpAlphaModifierSurfaceV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &WpAlphaModifierSurfaceV1,
        _: <WpAlphaModifierSurfaceV1 as Proxy>::Event,
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
    fn the_fade_rises_over_its_minutes() {
        assert_eq!(multiplier(Duration::ZERO), 0);
        let half = multiplier(Duration::from_secs(u64::from(Appearance::FADE_MINUTES) * 30));
        assert!((half as f64 / u32::MAX as f64 - 0.5).abs() < 0.001);
        assert_eq!(multiplier(Duration::from_secs(3 * 3600)), u32::MAX);
    }
}
