//! The idle fade (Settings → Appearance, `Appearance.fade_idle`): after
//! `fade_after_min` minutes with no input, the windows fade into the
//! background over `fade_over_min`; input brings them back over a second
//! ([`RETURN`]).
//!
//! Fading every window toward the picture looks exactly like the picture
//! rising over every window (`a·window + (1−a)·picture` either way), so
//! that is what is drawn: shell surfaces at the top of the render list
//! showing the picture (buffers over the background's own memory; black
//! without a picture), their opacity raised by `wp_alpha_modifier_v1` — a
//! number per step, no repaint. Their input regions are empty.
//!
//! The overlay is cut in pieces around what the shell keeps in sight
//! (`Topic::IdleKeep`: the top row's flower and agent islands, what you
//! may want to answer); that part is never covered, so they stay in full
//! sight while everything else goes. While the fade runs, `Topic::IdleFade`
//! says so (Projects shows a session's island even with its tab focused).
//!
//! Idle comes from `ext_idle_notifier_v1.get_idle_notification`, which
//! honors idle inhibitors (a video player keeping the screen awake); and
//! no fade runs while a window is fullscreen (Cinema).

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use sola_bus::topics::{Appearance, IdleFade, IdleKeep, Topic};
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

/// While fading, the opacity changes at most this often (over 30
/// minutes, a step of 1/1800 of the way).
const STEP: Duration = Duration::from_secs(1);
/// Input came back: the windows return over this, a step every bus tick.
const RETURN: Duration = Duration::from_secs(1);

pub struct FadeState {
    /// `ext_idle_notifier_v1`, `wp_alpha_modifier_v1`.
    pub notifier: Option<ExtIdleNotifierV1>,
    pub alpha: Option<WpAlphaModifierV1>,
    /// The idle delay asked for (ms); `None` while the fade is off.
    wanted: Option<u32>,
    /// How long the fade takes.
    over: Duration,
    /// The notification for `wanted`, and its delay.
    notification: Option<(ExtIdleNotificationV1, u32)>,
    /// When the fade began (idle, and no fullscreen window).
    since: Option<Instant>,
    /// Input came back: the level the return started from, and when.
    returning: Option<(u32, Instant)>,
    /// What stays in sight (`Topic::IdleKeep`, output-local logical).
    keep: Option<IdleKeep>,
    overlay: Option<Overlay>,
    /// Making the overlay failed this idle: stop asking for render passes.
    failed: bool,
    /// What `Topic::IdleFade` last said.
    told: Option<bool>,
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
            over: over(&look),
            notification: None,
            since: None,
            returning: None,
            keep: None,
            overlay: None,
            failed: false,
            told: None,
        }
    }
}

/// The picture over everything, in pieces, at some opacity.
struct Overlay {
    pieces: Vec<Piece>,
    /// The black buffers' own memory (none over the picture's).
    black: Option<(rustix::fd::OwnedFd, wl_shm_pool::WlShmPool)>,
    /// What it was cut around (that part has no piece).
    keep: Option<IdleKeep>,
    /// The fade's level.
    level: u32,
    set_at: Instant,
}

/// One rectangle of the overlay.
struct Piece {
    shell_surface: RiverShellSurfaceV1,
    node: RiverNodeV1,
    surface: wl_surface::WlSurface,
    opacity: WpAlphaModifierSurfaceV1,
    buffer: wl_buffer::WlBuffer,
}

impl Overlay {
    fn set(&mut self, level: u32) {
        for p in &self.pieces {
            p.opacity.set_multiplier(level);
            p.surface.commit();
        }
        self.level = level;
        self.set_at = Instant::now();
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        // Buffers outlive their pool; the pieces go after this.
        if let Some((_, pool)) = &self.black {
            pool.destroy();
        }
    }
}

impl Drop for Piece {
    fn drop(&mut self) {
        self.opacity.destroy();
        self.node.destroy();
        self.shell_surface.destroy();
        self.surface.destroy();
        self.buffer.destroy();
    }
}

/// `Topic::Appearance` (`None`: retracted, the defaults).
pub fn settings(state: &mut AppData, look: Option<&Appearance>) {
    let look = look.cloned().unwrap_or_default();
    let wanted = look
        .fade_idle
        .then(|| u32::from(look.fade_after_min.max(1)) * 60_000);
    // A new length takes from the next step: the fade so far keeps going.
    state.fade.over = over(&look);
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

/// `Topic::IdleKeep` (`None`: retracted). An overlay up is cut again in
/// the next render sequence.
pub fn keep(state: &mut AppData, keep: Option<IdleKeep>) {
    if state.fade.keep == keep {
        return;
    }
    debug!(?keep, "idle fade keeps");
    state.fade.keep = keep;
    if state.fade.overlay.is_some() {
        state.pending.render_dirty = true;
    }
}

/// Run every bus tick: ask for the idle notification, raise the overlay's
/// opacity as the fade goes, and lower it as the windows return.
pub fn tick(state: &mut AppData) {
    ensure(state);
    if let Some((from, at)) = state.fade.returning {
        let t = at.elapsed().as_secs_f64() / RETURN.as_secs_f64();
        if t >= 1.0 || state.fade.overlay.is_none() {
            drop_overlay(state);
            return;
        }
        if let Some(overlay) = &mut state.fade.overlay {
            overlay.set(returning_level(from, t));
        }
        flush(state);
        return;
    }
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
    let target = multiplier(since.elapsed(), state.fade.over);
    let Some(overlay) = &mut state.fade.overlay else {
        // Made in the next render sequence (it must be placed on top).
        if !state.fade.failed {
            state.pending.render_dirty = true;
        }
        return;
    };
    if overlay.level == target || overlay.set_at.elapsed() < STEP {
        return;
    }
    overlay.set(target);
    flush(state);
}

/// Run during every render sequence, last: make the overlay when the fade
/// needs it (again when what it keeps moved), and keep it on top.
pub fn sync_on_render(state: &mut AppData) {
    let fading = state.fade.since.is_some() || state.fade.returning.is_some();
    if !fading || state.fade.wanted.is_none() {
        state.fade.overlay = None;
        return;
    }
    if let Some(overlay) = &state.fade.overlay {
        // Cut again only while fading out; a return is over in a second.
        if overlay.keep == state.fade.keep || state.fade.returning.is_some() {
            for p in &overlay.pieces {
                p.node.place_top();
            }
            return;
        }
    }
    if !state.currently_fullscreen.is_empty() || state.fade.failed {
        return;
    }
    let level = match (&state.fade.overlay, state.fade.since) {
        (Some(overlay), _) => overlay.level,
        (None, Some(since)) => multiplier(since.elapsed(), state.fade.over),
        (None, None) => 0,
    };
    match make_overlay(state, level) {
        Ok(overlay) => {
            debug!(level, pieces = overlay.pieces.len(), "idle fade overlay");
            // The old one, if any, goes now: the new is already on top.
            state.fade.overlay = Some(overlay);
        }
        Err(e) => {
            warn!(%e, "idle fade overlay");
            state.fade.failed = true;
        }
    }
}

/// The opacity `elapsed` into a fade that takes `over`.
fn multiplier(elapsed: Duration, over: Duration) -> u32 {
    let t = (elapsed.as_secs_f64() / over.as_secs_f64().max(1.0)).clamp(0.0, 1.0);
    (t * f64::from(u32::MAX)) as u32
}

/// The opacity `t` (0–1) into a return from `from`: quick at first, then
/// settling.
fn returning_level(from: u32, t: f64) -> u32 {
    (f64::from(from) * (1.0 - t.clamp(0.0, 1.0)).powi(2)) as u32
}

fn over(look: &Appearance) -> Duration {
    Duration::from_secs(u64::from(look.fade_over_min.max(1)) * 60)
}

/// A rectangle of the output, logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// The output (`w` × `h`) in pieces that cover it once: the part kept
/// (`true`; inside the output), and the up to four around it.
fn cut(w: i32, h: i32, keep: Option<IdleKeep>) -> Vec<(Rect, bool)> {
    let full = Rect { x: 0, y: 0, w, h };
    let Some(k) = keep else {
        return vec![(full, false)];
    };
    let (x0, y0) = (k.x.clamp(0, w), k.y.clamp(0, h));
    let x1 = k.x.saturating_add(k.w as i32).clamp(0, w);
    let y1 = k.y.saturating_add(k.h as i32).clamp(0, h);
    if x1 <= x0 || y1 <= y0 {
        return vec![(full, false)];
    }
    let around = [
        Rect { x: 0, y: 0, w, h: y0 },
        Rect { x: 0, y: y1, w, h: h - y1 },
        Rect { x: 0, y: y0, w: x0, h: y1 - y0 },
        Rect { x: x1, y: y0, w: w - x1, h: y1 - y0 },
    ];
    let mut out = vec![(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, true)];
    out.extend(around.into_iter().filter(|r| r.w > 0 && r.h > 0).map(|r| (r, false)));
    out
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

/// Input: the windows come back over [`RETURN`].
fn resume(state: &mut AppData) {
    tell(state, false);
    state.fade.since = None;
    state.fade.failed = false;
    match state.fade.overlay.as_ref().map(|o| o.level) {
        Some(level) if level > 0 => {
            if state.fade.returning.is_none() {
                state.fade.returning = Some((level, Instant::now()));
            }
        }
        _ => drop_overlay(state),
    }
}

/// Tell the bus whether the desk is fading (`Topic::IdleFade`), once per
/// change.
fn tell(state: &mut AppData, fading: bool) {
    if state.fade.told == Some(fading) {
        return;
    }
    state.fade.told = Some(fading);
    state.bus.emit(Topic::IdleFade(IdleFade { fading }));
}

/// The fade turned off or changed: the windows back at once.
fn wake(state: &mut AppData) {
    tell(state, false);
    state.fade.since = None;
    state.fade.failed = false;
    drop_overlay(state);
}

fn drop_overlay(state: &mut AppData) {
    state.fade.returning = None;
    let Some(mut overlay) = state.fade.overlay.take() else {
        return;
    };
    overlay.set(0);
    drop(overlay);
    flush(state);
}

fn flush(state: &AppData) {
    if let Some(conn) = &state.conn {
        let _ = conn.flush();
    }
}

fn make_overlay(state: &mut AppData, level: u32) -> Result<Overlay, String> {
    let wm = state.wm.clone().ok_or("window manager not bound")?;
    let alpha = state.fade.alpha.clone().ok_or("wp_alpha_modifier_v1 not bound")?;
    let compositor = state
        .shadow
        .compositor
        .clone()
        .ok_or("wl_compositor not bound")?;
    let qh = state.qh.clone().ok_or("queue handle missing")?;
    let (ow, oh) = state.output_size.ok_or("output size unknown")?;
    let (pool, bw, bh, scale, black) = match crate::client::background::overlay_pool(state) {
        Some((pool, w, h, scale)) => (pool, w, h, scale.max(1), None),
        None => {
            let shm = state.screenshot.shm.clone().ok_or("wl_shm not bound")?;
            let size = ow.checked_mul(oh).and_then(|p| p.checked_mul(4)).ok_or("size")?;
            // A fresh memfd reads as zeros: black.
            let memfd = memfd_create("sola-fade", MemfdFlags::CLOEXEC)
                .map_err(|e| format!("memfd_create: {e}"))?;
            ftruncate(&memfd, size as u64).map_err(|e| format!("ftruncate: {e}"))?;
            let pool = shm.create_pool(memfd.as_fd(), size, &qh, ());
            (pool.clone(), ow, oh, 1, Some((memfd, pool)))
        }
    };
    let (x, y) = state.output_origin.unwrap_or((0, 0));
    let mut pieces = Vec::new();
    for (r, kept) in cut(ow, oh, state.fade.keep) {
        // The kept part stays uncovered: the flower and the agent islands.
        if kept {
            continue;
        }
        // The piece in the buffer's pixels, inside it, whole logical pixels.
        let (px, py) = (r.x * scale, r.y * scale);
        let pw = (r.w * scale).min(bw - px);
        let ph = (r.h * scale).min(bh - py);
        let (pw, ph) = (pw - pw % scale, ph - ph % scale);
        if pw <= 0 || ph <= 0 {
            continue;
        }
        let offset = (py * bw + px) * 4;
        let buffer = pool.create_buffer(offset, pw, ph, bw * 4, wl_shm::Format::Xrgb8888, &qh, ());
        let surface = compositor.create_surface(&qh, ());
        let region = compositor.create_region(&qh, ());
        surface.set_input_region(Some(&region));
        region.destroy();
        let opacity = alpha.get_surface(&surface, &qh, ());
        opacity.set_multiplier(level);
        let shell_surface = wm.get_shell_surface(&surface, &qh, ());
        let node = shell_surface.get_node(&qh, ());
        node.place_top();
        node.set_position(x + r.x, y + r.y);
        shell_surface.sync_next_commit();
        surface.set_buffer_scale(scale);
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, pw, ph);
        surface.commit();
        pieces.push(Piece {
            shell_surface,
            node,
            surface,
            opacity,
            buffer,
        });
    }
    Ok(Overlay {
        pieces,
        black,
        keep: state.fade.keep,
        level,
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
                tell(state, true);
                state.fade.returning = None;
                state.fade.since = Some(Instant::now());
                state.pending.render_dirty = true;
            }
            ext_idle_notification_v1::Event::Resumed => {
                if state.fade.since.is_some() {
                    info!("input: the windows come back");
                }
                resume(state);
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
        let over = over(&Appearance::default());
        assert_eq!(over, Duration::from_secs(30 * 60));
        assert_eq!(multiplier(Duration::ZERO, over), 0);
        let half = multiplier(over / 2, over);
        assert!((half as f64 / u32::MAX as f64 - 0.5).abs() < 0.001);
        assert_eq!(multiplier(Duration::from_secs(3 * 3600), over), u32::MAX);
        let quick = Duration::from_secs(5 * 60);
        assert_eq!(multiplier(quick, quick), u32::MAX);
    }

    #[test]
    fn the_return_starts_quick_and_ends_clear() {
        let from = u32::MAX;
        assert_eq!(returning_level(from, 0.0), from);
        let half = returning_level(from, 0.5) as f64 / f64::from(from);
        assert!((half - 0.25).abs() < 0.001, "{half}");
        assert_eq!(returning_level(from, 1.0), 0);
    }

    #[test]
    fn the_cut_covers_the_output_once() {
        let keep = IdleKeep {
            x: 0,
            y: 0,
            w: 240,
            h: 40,
        };
        let pieces = cut(1920, 1080, Some(keep));
        let area: i32 = pieces.iter().map(|(r, _)| r.w * r.h).sum();
        assert_eq!(area, 1920 * 1080);
        let kept: Vec<_> = pieces.iter().filter(|(_, k)| *k).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, Rect { x: 0, y: 0, w: 240, h: 40 });
        // In the middle: four around it.
        let mid = IdleKeep {
            x: 100,
            y: 100,
            w: 50,
            h: 50,
        };
        assert_eq!(cut(400, 300, Some(mid)).len(), 5);
        // Off the output, or none: one piece.
        let off = IdleKeep {
            x: 5000,
            y: 0,
            w: 10,
            h: 10,
        };
        let whole = Rect { x: 0, y: 0, w: 400, h: 300 };
        assert_eq!(cut(400, 300, Some(off)), vec![(whole, false)]);
        assert_eq!(cut(400, 300, None).len(), 1);
    }
}
