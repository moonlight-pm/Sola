//! The desk's opacity and the idle fade (Settings → Appearance). At rest
//! the windows, the bar and the top row are `Appearance.opacity` % opaque
//! over the background (90 by default). With `fade_idle`, after
//! `fade_after_min` minutes with no input they fade into the background
//! over `fade_over_min`; input brings them back to their rest over a
//! second ([`RETURN`]).
//!
//! Fading every window toward the picture looks exactly like the picture
//! rising over every window (`a·window + (1−a)·picture` either way), so
//! that is what is drawn: shell surfaces at the top of the render list
//! showing the picture (buffers over the background's own memory; black
//! without a picture), their opacity raised by `wp_alpha_modifier_v1` — a
//! number per step, no repaint. Their input regions are empty.
//!
//! At rest the overlay is up at `100 − opacity` % (its floor), and the
//! fade rises from there; at 100 % opacity it is only up while fading.
//! It is cut in pieces around what the shell keeps in sight
//! (`Topic::IdleKeep`: the top row's flower and agent islands, what you
//! may want to answer); that piece stays at the floor, so they keep their
//! rest while everything else goes. Nothing is over a fullscreen window
//! (Cinema scans out). While the fade runs, `Topic::IdleFade`
//! says so (Projects shows a session's island even with its tab focused),
//! and once it is full, that the windows are out of sight (the shell
//! reports their tabs off screen, so a browser rests its pages).
//!
//! Idle comes from `ext_idle_notifier_v1.get_idle_notification`, which
//! honors idle inhibitors (a video player keeping the screen awake); and
//! no fade runs while a window is fullscreen (Cinema).
//!
//! Once the fade is full, the picture itself dims after a while ([`dim`]).

mod dim;

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
    /// The overlay's level at rest (`Appearance.opacity`); 0 at 100 %.
    floor: u32,
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
    told: Option<IdleFade>,
    /// The background's dim (`Appearance.dim_*`); `None` while it is off.
    dim: Option<dim::Times>,
    /// When the fade became full (the windows out of sight).
    full_since: Option<Instant>,
    dimmer: Option<dim::Dimmer>,
    /// Making the dim's layer failed this idle.
    dim_failed: bool,
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
            floor: floor(&look),
            notification: None,
            since: None,
            returning: None,
            keep: None,
            overlay: None,
            failed: false,
            told: None,
            dim: dim::times(&look),
            full_since: None,
            dimmer: None,
            dim_failed: false,
        }
    }
}

/// The picture over everything, in pieces, at some opacity.
struct Overlay {
    pieces: Vec<Piece>,
    /// The black buffers' own memory (none over the picture's).
    black: Option<(rustix::fd::OwnedFd, wl_shm_pool::WlShmPool)>,
    /// What it was cut around.
    keep: Option<IdleKeep>,
    /// The background it shows (`background::generation`; 0: black) and
    /// the output's size: either changing makes it again.
    ground: u64,
    size: (i32, i32),
    /// The fade's level.
    level: u32,
    set_at: Instant,
}

/// One rectangle of the overlay.
struct Piece {
    /// Over what the shell keeps in sight: never past the floor.
    kept: bool,
    shell_surface: RiverShellSurfaceV1,
    node: RiverNodeV1,
    surface: wl_surface::WlSurface,
    opacity: WpAlphaModifierSurfaceV1,
    buffer: wl_buffer::WlBuffer,
}

impl Overlay {
    fn set(&mut self, level: u32, floor: u32) {
        for p in &self.pieces {
            p.opacity
                .set_multiplier(if p.kept { level.min(floor) } else { level });
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
    let floor = floor(&look);
    if floor != state.fade.floor {
        info!(opacity = look.opacity, "the desk's opacity");
        state.fade.floor = floor;
        rest(state);
    }
    let wanted = look
        .fade_idle
        .then(|| u32::from(look.fade_after_min.max(1)) * 60_000);
    // A new length takes from the next step: the fade so far keeps going.
    state.fade.over = over(&look);
    state.fade.dim = dim::times(&look);
    if state.fade.dim.is_none() && state.fade.dimmer.take().is_some() {
        flush(state);
    }
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
    let floor = state.fade.floor;
    if let Some((from, at)) = state.fade.returning {
        let t = at.elapsed().as_secs_f64() / RETURN.as_secs_f64();
        if t >= 1.0 || state.fade.overlay.is_none() {
            state.fade.returning = None;
            state.fade.dimmer = None;
            rest(state);
            return;
        }
        if let Some(overlay) = &mut state.fade.overlay {
            overlay.set(returning_level(from, floor, t), floor);
        }
        dim::returning(state, t);
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
        tell(state, true, false);
        state.fade.since = Some(Instant::now());
        state.fade.full_since = None;
        return;
    }
    let target = multiplier(floor, since.elapsed(), state.fade.over);
    let Some(overlay) = &mut state.fade.overlay else {
        // Made in the next render sequence (it must be placed on top).
        if !state.fade.failed {
            state.pending.render_dirty = true;
        }
        return;
    };
    if overlay.level != target && overlay.set_at.elapsed() >= STEP {
        overlay.set(target, floor);
        flush(state);
    }
    // Full: the windows are out of sight until input, and the dim's
    // wait begins.
    if state.fade.overlay.as_ref().is_some_and(|o| o.level == u32::MAX) {
        tell(state, true, true);
        state.fade.full_since.get_or_insert_with(Instant::now);
        dim::tick(state);
    }
}

/// Run during every render sequence, last: make the overlay when the rest
/// or the fade needs it (again when what it keeps moved), and keep it on
/// top, with the dim's black over it.
pub fn sync_on_render(state: &mut AppData) {
    sync_overlay(state);
    dim::sync(state);
}

fn sync_overlay(state: &mut AppData) {
    let fading = state.fade.wanted.is_some()
        && (state.fade.since.is_some() || state.fade.returning.is_some());
    if !fading && state.fade.floor == 0 {
        state.fade.overlay = None;
        return;
    }
    if !state.currently_fullscreen.is_empty() {
        // A fullscreen window scans out with nothing over it.
        state.fade.overlay = None;
        return;
    }
    let ground = crate::client::background::generation(state);
    if let Some(overlay) = &state.fade.overlay {
        // Made again (a new cut, picture or size) unless returning; a
        // return is over in a second.
        let current = overlay.keep == state.fade.keep
            && overlay.ground == ground
            && Some(overlay.size) == state.output_size;
        if current || state.fade.returning.is_some() {
            for p in &overlay.pieces {
                p.node.place_top();
            }
            return;
        }
    }
    if state.fade.failed {
        return;
    }
    let floor = state.fade.floor;
    let level = match (&state.fade.overlay, state.fade.since) {
        (Some(overlay), _) => overlay.level,
        (None, Some(since)) => multiplier(floor, since.elapsed(), state.fade.over),
        (None, None) => floor,
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

/// The overlay's opacity `elapsed` into a fade that takes `over`, rising
/// from `floor`.
fn multiplier(floor: u32, elapsed: Duration, over: Duration) -> u32 {
    let t = (elapsed.as_secs_f64() / over.as_secs_f64().max(1.0)).clamp(0.0, 1.0);
    floor + (t * f64::from(u32::MAX - floor)) as u32
}

/// The opacity `t` (0–1) into a return from `from` to `floor`: quick at
/// first, then settling.
fn returning_level(from: u32, floor: u32, t: f64) -> u32 {
    let from = from.max(floor);
    floor + (f64::from(from - floor) * (1.0 - t.clamp(0.0, 1.0)).powi(2)) as u32
}

/// The overlay's level at rest: the picture over the desk at
/// `100 − opacity` %.
fn floor(look: &Appearance) -> u32 {
    let clear = 100 - u32::from(look.opacity.min(100));
    (f64::from(clear) / 100.0 * f64::from(u32::MAX)) as u32
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

/// Input: the windows come back to their rest over [`RETURN`].
fn resume(state: &mut AppData) {
    tell(state, false, false);
    state.fade.since = None;
    state.fade.full_since = None;
    state.fade.failed = false;
    state.fade.dim_failed = false;
    match state.fade.overlay.as_ref().map(|o| o.level) {
        Some(level) if level > state.fade.floor => {
            if state.fade.returning.is_none() {
                dim::start_return(state);
                state.fade.returning = Some((level, Instant::now()));
            }
        }
        _ => {
            state.fade.returning = None;
            state.fade.dimmer = None;
            rest(state);
        }
    }
}

/// Not fading: the overlay at the floor, made in the next render sequence
/// if it is not up, gone at 100 %. A fade under way carries on and comes
/// back to the new floor.
fn rest(state: &mut AppData) {
    if state.fade.since.is_some() || state.fade.returning.is_some() {
        return;
    }
    let floor = state.fade.floor;
    match &mut state.fade.overlay {
        Some(_) if floor == 0 => drop_overlay(state),
        Some(overlay) => {
            overlay.set(floor, floor);
            flush(state);
        }
        None if floor > 0 => state.pending.render_dirty = true,
        None => {}
    }
}

/// Tell the bus whether the desk is fading, and whether the windows are
/// out of sight (`Topic::IdleFade`), once per change.
fn tell(state: &mut AppData, fading: bool, out: bool) {
    let fade = IdleFade { fading, out };
    if state.fade.told == Some(fade) {
        return;
    }
    if state.fade.told.map(|t| t.out) != Some(out) {
        info!(out, "idle fade: windows {}", if out { "out of sight" } else { "in sight" });
    }
    state.fade.told = Some(fade);
    state.bus.emit(Topic::IdleFade(fade));
}

/// The fade turned off or changed: the windows back to their rest at
/// once.
fn wake(state: &mut AppData) {
    tell(state, false, false);
    state.fade.since = None;
    state.fade.full_since = None;
    state.fade.failed = false;
    state.fade.dim_failed = false;
    state.fade.returning = None;
    state.fade.dimmer = None;
    rest(state);
}

fn drop_overlay(state: &mut AppData) {
    state.fade.returning = None;
    state.fade.dimmer = None;
    let Some(mut overlay) = state.fade.overlay.take() else {
        return;
    };
    overlay.set(0, 0);
    drop(overlay);
    flush(state);
}

fn flush(state: &AppData) {
    if let Some(conn) = &state.conn {
        let _ = conn.flush();
    }
}

/// A pool of `w` × `h` XRGB pixels, all black: a fresh memfd reads as
/// zeros.
fn black_pool(
    state: &AppData,
    w: i32,
    h: i32,
) -> Result<(rustix::fd::OwnedFd, wl_shm_pool::WlShmPool), String> {
    let shm = state.screenshot.shm.as_ref().ok_or("wl_shm not bound")?;
    let qh = state.qh.as_ref().ok_or("queue handle missing")?;
    let size = w.checked_mul(h).and_then(|p| p.checked_mul(4)).ok_or("size")?;
    let memfd =
        memfd_create("sola-fade", MemfdFlags::CLOEXEC).map_err(|e| format!("memfd_create: {e}"))?;
    ftruncate(&memfd, size as u64).map_err(|e| format!("ftruncate: {e}"))?;
    let pool = shm.create_pool(memfd.as_fd(), size, qh, ());
    Ok((memfd, pool))
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
            let (memfd, pool) = black_pool(state, ow, oh)?;
            (pool.clone(), ow, oh, 1, Some((memfd, pool)))
        }
    };
    let (x, y) = state.output_origin.unwrap_or((0, 0));
    let mut pieces = Vec::new();
    let floor = state.fade.floor;
    for (r, kept) in cut(ow, oh, state.fade.keep) {
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
        // The kept part (the flower, the agent islands) stays at the rest.
        opacity.set_multiplier(if kept { level.min(floor) } else { level });
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
            kept,
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
        ground: crate::client::background::generation(state),
        size: (ow, oh),
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
                tell(state, true, false);
                state.fade.returning = None;
                state.fade.since = Some(Instant::now());
                state.fade.full_since = None;
                state.fade.dim_failed = false;
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
        assert_eq!(multiplier(0, Duration::ZERO, over), 0);
        let half = multiplier(0, over / 2, over);
        assert!((half as f64 / u32::MAX as f64 - 0.5).abs() < 0.001);
        assert_eq!(multiplier(0, Duration::from_secs(3 * 3600), over), u32::MAX);
        let quick = Duration::from_secs(5 * 60);
        assert_eq!(multiplier(0, quick, quick), u32::MAX);
    }

    #[test]
    fn the_fade_rises_from_the_rest() {
        let floor = floor(&Appearance::default());
        // 90 % opaque: the picture over the desk at a tenth.
        assert!((floor as f64 / u32::MAX as f64 - 0.1).abs() < 0.001);
        let over = Duration::from_secs(60);
        assert_eq!(multiplier(floor, Duration::ZERO, over), floor);
        assert_eq!(multiplier(floor, over, over), u32::MAX);
        let opaque = Appearance {
            opacity: 100,
            ..Appearance::default()
        };
        assert_eq!(super::floor(&opaque), 0);
    }

    #[test]
    fn the_return_starts_quick_and_ends_at_the_rest() {
        let from = u32::MAX;
        assert_eq!(returning_level(from, 0, 0.0), from);
        let half = returning_level(from, 0, 0.5) as f64 / f64::from(from);
        assert!((half - 0.25).abs() < 0.001, "{half}");
        assert_eq!(returning_level(from, 0, 1.0), 0);
        let floor = u32::MAX / 10;
        assert_eq!(returning_level(from, floor, 1.0), floor);
        assert_eq!(returning_level(floor / 2, floor, 0.5), floor);
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
