//! Content paint budget — anti-checkerboard, **stable scale**.
//!
//! Forced 2× supersample multiplied raster work and caused residual full-width
//! black bands under hard scroll. Default is honest compositor scale.
//!
//! **Do not** change scale mid-scroll. An earlier adaptive clamp (1.25× while
//! scrolling) thrashed Resize + wl_buffer cache destroy against the front
//! buffer → constant tile flicker regression. Scale must be sticky once
//! chosen for a viewport size.
//!
//! Overrides:
//! - `SOLA_BROWSER_DPR=N` — force scale
//! - `SOLA_BROWSER_SUPER_SAMPLE=1` — max(compositor, 2.0) for crisp text on 1×

use std::sync::atomic::{AtomicU64, Ordering};

/// Soft cap on physical content edge (px).
const MAX_PHYS_EDGE: f64 = 8192.0;

/// Sticky last (phys_w, phys_h) key so we never micro-thrash Resize on
/// subpixel bound jitter — only real size/DPR policy changes.
static STICKY_PHYS: AtomicU64 = AtomicU64::new(0);

fn pack(w: u32, h: u32) -> u64 {
    ((w as u64) << 32) | (h as u64)
}

/// Call on content wheel (optional telem); scale policy is **not** scroll-coupled.
pub fn note_scroll() {
    // Reserved for future telem; do not change paint scale here.
}

/// Device scale for WebKit content buffers + plane `buffer_scale`.
///
/// Stable for a given compositor scale + CSS size (no scroll-edge thrash).
pub fn choose_dpr(compositor_scale: f64, css_w: u32, css_h: u32) -> f64 {
    let max_css = css_w.max(css_h).max(1) as f64;
    let edge_cap = (MAX_PHYS_EDGE / max_css).clamp(1.0, 2.0);

    if let Ok(s) = std::env::var("SOLA_BROWSER_DPR") {
        if let Ok(v) = s.parse::<f64>() {
            if v.is_finite() && v >= 1.0 {
                return v.min(edge_cap);
            }
        }
    }

    let comp = compositor_scale.max(1.0);
    let want = if std::env::var_os("SOLA_BROWSER_SUPER_SAMPLE").is_some() {
        comp.max(2.0)
    } else {
        // Honest scale — primary anti-checkerboard lever without Resize thrash.
        comp
    };
    want.min(edge_cap)
}

/// Hysteresis for physical size: ignore 1px jitter that causes Resize storms.
pub fn stabilize_phys(phys_w: u32, phys_h: u32) -> (u32, u32) {
    let packed = pack(phys_w, phys_h);
    let prev = STICKY_PHYS.load(Ordering::Relaxed);
    if prev == 0 {
        STICKY_PHYS.store(packed, Ordering::Relaxed);
        return (phys_w, phys_h);
    }
    let pw = (prev >> 32) as u32;
    let ph = (prev & 0xffff_ffff) as u32;
    // Allow change only if either edge moves by more than 1 px.
    if phys_w.abs_diff(pw) <= 1 && phys_h.abs_diff(ph) <= 1 {
        return (pw, ph);
    }
    STICKY_PHYS.store(packed, Ordering::Relaxed);
    (phys_w, phys_h)
}
