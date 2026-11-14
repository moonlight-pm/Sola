//! Badges in the screen's top-left corner while a window fills it
//! (`Topic::Badge`, `solactl badge`). A game in Cinema shows nothing of the
//! desk, so this is how a session editing its addon says a reload (↻, in
//! the accent) or a restart (⏻, in the warning colour) waits. One small
//! round layer per badge, side by side in id order, [`MARGIN`] in from the
//! corner; none while the desk itself shows.
//!
//! Each is a shell surface with an empty input region, never focused: a
//! click goes through to the game, which keeps the keyboard and its
//! fullscreen. While one shows, River composites the game instead of
//! scanning it out.

use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};
use std::fs::File;
use std::io::Write;
use std::os::fd::AsFd;

use rustix::fs::{MemfdFlags, memfd_create};
use sola_bus::topics::{Badge, BadgeKind, Topic};
use sola_core::theme::Theme;
use tracing::{info, warn};
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool, wl_surface};

use crate::client::{AppData, background};
use crate::protocol::river_window_management_v1::river_node_v1::RiverNodeV1;
use crate::protocol::river_window_management_v1::river_shell_surface_v1::RiverShellSurfaceV1;

/// A badge's side, logical pixels: small on a 5120 × 2160 screen, so the
/// game's corner stays usable and an addon's button fits at 40.
const SIZE: i32 = 32;
/// From the screen's edges, and between badges.
const MARGIN: i32 = 4;
/// The plate's opacity.
const PLATE: f32 = 0.92;

pub struct BadgeState {
    /// What is set, by id.
    set: BTreeMap<String, BadgeKind>,
    colors: Colors,
    /// The layers on screen, and what they were made for (`None`: none).
    shown: Vec<Layer>,
    made_for: Option<Look>,
}

impl Default for BadgeState {
    fn default() -> Self {
        Self {
            set: BTreeMap::new(),
            colors: Colors::of(&Theme::default()),
            shown: Vec::new(),
            made_for: None,
        }
    }
}

/// The theme's colours a badge is drawn in (sRGB 0–1).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Colors {
    base: [f32; 3],
    rim: [f32; 3],
    accent: [f32; 3],
    warning: [f32; 3],
}

impl Colors {
    fn of(theme: &Theme) -> Self {
        let base = sola_core::theme::token_rgb(theme, "bg-primary");
        Self {
            base,
            rim: sola_core::theme::rest_rim(base),
            accent: sola_core::theme::token_rgb(theme, "accent"),
            warning: sola_core::theme::token_rgb(theme, "warning"),
        }
    }
}

/// What the layers on screen show, and where.
#[derive(Debug, Clone, PartialEq)]
struct Look {
    kinds: Vec<BadgeKind>,
    colors: Colors,
    scale: i32,
    origin: (i32, i32),
}

struct Layer {
    surface: wl_surface::WlSurface,
    shell_surface: RiverShellSurfaceV1,
    node: RiverNodeV1,
    buffer: wl_buffer::WlBuffer,
    pool: wl_shm_pool::WlShmPool,
    /// The buffer's memory.
    _memory: File,
}

impl Drop for Layer {
    fn drop(&mut self) {
        self.node.destroy();
        self.shell_surface.destroy();
        self.surface.destroy();
        self.buffer.destroy();
        self.pool.destroy();
    }
}

/// `Topic::Badge`: set, or retracted (`None`). One set before this boot
/// is retracted instead: the reboot did what it asked.
pub fn set(state: &mut AppData, id: String, badge: Option<Badge>) {
    match badge {
        Some(badge) if badge.set_at < booted_at_ms() => {
            info!(%id, "badge: set before this boot; retracting it");
            state.bus.retract(Topic::Badge(badge));
            return;
        }
        Some(badge) => {
            info!(%id, kind = ?badge.kind, "badge set");
            state.badges.set.insert(id, badge.kind);
        }
        None => {
            if state.badges.set.remove(&id).is_none() {
                return;
            }
            info!(%id, "badge cleared");
        }
    }
    state.pending.render_dirty = true;
}

/// `Topic::Theme`: draw in its colours.
pub fn theme(state: &mut AppData, theme: &Theme) {
    let colors = Colors::of(theme);
    if colors != state.badges.colors {
        state.badges.colors = colors;
        state.pending.render_dirty = true;
    }
}

/// In every render sequence, before the idle fade (its dim and the
/// blackout go over a badge): keep the layers on top, or make them again
/// when what they show changed.
pub fn sync_on_render(state: &mut AppData) {
    let want = wanted(state);
    if want == state.badges.made_for {
        for layer in &state.badges.shown {
            layer.node.place_top();
        }
        return;
    }
    state.badges.shown.clear();
    state.badges.made_for = None;
    let Some(look) = want else {
        return;
    };
    let mut layers = Vec::new();
    for (i, kind) in look.kinds.iter().enumerate() {
        let x = look.origin.0 + MARGIN + i as i32 * (SIZE + MARGIN);
        let y = look.origin.1 + MARGIN;
        match make(state, *kind, look.colors, look.scale, (x, y)) {
            Ok(layer) => layers.push(layer),
            // Not tried again until the look changes.
            Err(e) => warn!(%e, ?kind, "badge layer"),
        }
    }
    state.badges.shown = layers;
    state.badges.made_for = Some(look);
}

/// What should show: the badges set, while a window fills the screen.
fn wanted(state: &AppData) -> Option<Look> {
    let b = &state.badges;
    if b.set.is_empty() || !background::screen_filled(state) {
        return None;
    }
    Some(Look {
        kinds: b.set.values().copied().collect(),
        colors: b.colors,
        scale: background::scale(state),
        origin: state.output_origin.unwrap_or((0, 0)),
    })
}

/// A badge's layer on top at `at`, its input region empty.
fn make(
    state: &AppData,
    kind: BadgeKind,
    colors: Colors,
    scale: i32,
    at: (i32, i32),
) -> Result<Layer, String> {
    let wm = state.wm.clone().ok_or("window manager not bound")?;
    let compositor = state
        .shadow
        .compositor
        .clone()
        .ok_or("wl_compositor not bound")?;
    let shm = state.screenshot.shm.as_ref().ok_or("wl_shm not bound")?;
    let qh = state.qh.clone().ok_or("queue handle missing")?;
    let px = SIZE * scale;
    let pixels = paint(kind, colors, px as usize);
    let memfd =
        memfd_create("sola-badge", MemfdFlags::CLOEXEC).map_err(|e| format!("memfd_create: {e}"))?;
    let mut memory = File::from(memfd);
    memory.write_all(&pixels).map_err(|e| format!("write: {e}"))?;
    let pool = shm.create_pool(memory.as_fd(), pixels.len() as i32, &qh, ());
    let buffer = pool.create_buffer(0, px, px, px * 4, wl_shm::Format::Argb8888, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let region = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
    let shell_surface = wm.get_shell_surface(&surface, &qh, ());
    let node = shell_surface.get_node(&qh, ());
    node.place_top();
    node.set_position(at.0, at.1);
    shell_surface.sync_next_commit();
    surface.set_buffer_scale(scale);
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, px, px);
    surface.commit();
    Ok(Layer {
        surface,
        shell_surface,
        node,
        buffer,
        pool,
        _memory: memory,
    })
}

/// When this boot began (unix ms), from `/proc/stat`'s `btime`; 0 when
/// unknown, so nothing is dropped.
fn booted_at_ms() -> u64 {
    std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|stat| btime(&stat))
        .map_or(0, |secs| secs * 1000)
}

fn btime(stat: &str) -> Option<u64> {
    stat.lines()
        .find_map(|line| line.strip_prefix("btime "))
        .and_then(|secs| secs.trim().parse().ok())
}

/// A badge `px` buffer pixels square, premultiplied ARGB in little-endian
/// bytes (B, G, R, A): a round plate in the theme's base with a 1 px rim,
/// and its glyph. A restart's rim is in the warning colour too.
fn paint(kind: BadgeKind, colors: Colors, px: usize) -> Vec<u8> {
    let s = px as f32;
    let c = s / 2.0;
    let edge = c - 0.5;
    let line = s / SIZE as f32;
    let (rim, ink) = match kind {
        BadgeKind::Reload => (colors.rim, colors.accent),
        BadgeKind::Restart => (colors.warning, colors.warning),
    };
    let mut out = Vec::with_capacity(px * px * 4);
    for y in 0..px {
        for x in 0..px {
            let p = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let r = p.0.hypot(p.1);
            let mut dst = [0.0; 4];
            over(&mut dst, colors.base, PLATE * cover(r - edge));
            over(&mut dst, rim, cover(r - edge).min(cover(edge - line - r)));
            over(&mut dst, ink, cover(glyph(kind, s, p)));
            let [r, g, b, a] = dst.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out
}

/// How much of a pixel a shape covers, from its centre's distance to the
/// shape's edge (negative inside).
fn cover(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// `rgb` at opacity `a` over `dst` (premultiplied RGBA).
fn over(dst: &mut [f32; 4], rgb: [f32; 3], a: f32) {
    for i in 0..3 {
        dst[i] = rgb[i] * a + dst[i] * (1.0 - a);
    }
    dst[3] = a + dst[3] * (1.0 - a);
}

/// Distance from `p` (from the centre, y down) to the glyph of a badge
/// `s` across: a ring open at the top, with a bar down into it (⏻), or
/// turning clockwise to an arrowhead (↻).
fn glyph(kind: BadgeKind, s: f32, p: (f32, f32)) -> f32 {
    let r = 0.24 * s;
    let half = 0.085 * s / 2.0;
    let up = -PI / 2.0;
    match kind {
        BadgeKind::Restart => {
            let gap = 40f32.to_radians();
            let ring = arc(p, r, up + gap, TAU - 2.0 * gap);
            let bar = segment(p, (0.0, -r), (0.0, -0.15 * r));
            ring.min(bar) - half
        }
        BadgeKind::Reload => {
            let gap = 30f32.to_radians();
            let end = up - gap;
            let ring = arc(p, r, up + gap, TAU - 2.0 * gap) - half;
            let at = (r * end.cos(), r * end.sin());
            let ahead = (-end.sin(), end.cos());
            let out = (end.cos(), end.sin());
            let (long, wide) = (3.2 * half, 2.5 * half);
            let tip = (at.0 + ahead.0 * long, at.1 + ahead.1 * long);
            let a = (at.0 + out.0 * wide, at.1 + out.1 * wide);
            let b = (at.0 - out.0 * wide, at.1 - out.1 * wide);
            ring.min(triangle(p, [tip, a, b]))
        }
    }
}

/// Distance from `p` to the arc of radius `r` from angle `from` turning
/// clockwise on screen through `sweep` (radians).
fn arc(p: (f32, f32), r: f32, from: f32, sweep: f32) -> f32 {
    if (p.1.atan2(p.0) - from).rem_euclid(TAU) <= sweep {
        return (p.0.hypot(p.1) - r).abs();
    }
    let to = |a: f32| (p.0 - r * a.cos()).hypot(p.1 - r * a.sin());
    to(from).min(to(from + sweep))
}

/// Distance from `p` to the segment `a`–`b`.
fn segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

/// Signed distance from `p` to a triangle's edges' lines, the farthest
/// (exact inside and along the edges, close enough by its corners).
fn triangle(p: (f32, f32), v: [(f32, f32); 3]) -> f32 {
    let mid = ((v[0].0 + v[1].0 + v[2].0) / 3.0, (v[0].1 + v[1].1 + v[2].1) / 3.0);
    (0..3)
        .map(|i| {
            let (a, b) = (v[i], v[(i + 1) % 3]);
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            let mut n = ((b.1 - a.1) / len, (a.0 - b.0) / len);
            if (mid.0 - a.0) * n.0 + (mid.1 - a.1) * n.1 > 0.0 {
                n = (-n.0, -n.1);
            }
            (p.0 - a.0) * n.0 + (p.1 - a.1) * n.1
        })
        .fold(f32::MIN, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> Colors {
        Colors::of(&Theme::default())
    }

    /// The pixel at (`x`, `y`) of a badge `px` square: B, G, R, A.
    fn pixel(buf: &[u8], px: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * px + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    fn bgr(rgb: [f32; 3]) -> [u8; 3] {
        let [r, g, b] = rgb.map(|v| (v * 255.0).round() as u8);
        [b, g, r]
    }

    #[test]
    fn the_corners_are_clear_and_the_plate_is_not() {
        let px = SIZE as usize;
        let buf = paint(BadgeKind::Reload, colors(), px);
        assert_eq!(buf.len(), px * px * 4);
        assert_eq!(pixel(&buf, px, 0, 0)[3], 0);
        assert_eq!(pixel(&buf, px, px - 1, px - 1)[3], 0);
        // The middle is the plate alone, the glyph around it.
        let mid = pixel(&buf, px, px / 2, px / 2);
        assert_eq!(mid[3], (PLATE * 255.0).round() as u8);
    }

    #[test]
    fn the_power_glyph_has_its_bar_where_the_arrow_has_its_gap() {
        let px = SIZE as usize;
        let c = colors();
        let warning = bgr(c.warning);
        // Halfway down the bar, just above the middle.
        let (x, y) = (px / 2, px / 2 - 4);
        let restart = pixel(&paint(BadgeKind::Restart, c, px), px, x, y);
        assert_eq!(restart[..3], warning, "{restart:?}");
        let reload = pixel(&paint(BadgeKind::Reload, c, px), px, x, y);
        assert_ne!(reload[..3], bgr(c.accent), "{reload:?}");
        // Straight below the middle both have their ring.
        let below = (px / 2, px / 2 + (0.24 * px as f32) as usize);
        let ring = pixel(&paint(BadgeKind::Reload, c, px), px, below.0, below.1);
        assert_eq!(ring[..3], bgr(c.accent), "{ring:?}");
    }

    #[test]
    fn a_badge_doubles_on_a_doubled_screen() {
        let buf = paint(BadgeKind::Restart, colors(), 2 * SIZE as usize);
        assert_eq!(buf.len(), 4 * (SIZE * SIZE) as usize * 4);
    }

    #[test]
    fn the_boot_time_is_read_from_proc_stat() {
        let stat = "cpu  1 2 3\nintr 5\nctxt 9\nbtime 1763100000\nprocesses 4\n";
        assert_eq!(btime(stat), Some(1_763_100_000));
        assert_eq!(btime("cpu 1\n"), None);
    }
}
