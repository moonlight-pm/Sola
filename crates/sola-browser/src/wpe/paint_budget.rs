//! Adaptive content paint budget — anti-checkerboard control plane.
//!
//! Fast scroll on heavy pages outruns WebKit tile raster. Forced 2×
//! supersample multiplies pixel work by ~4 vs 1× and was a major residual
//! black-swath driver (NVIDIA + large windows, full-width holes including
//! fixed chrome when the whole framebuffer row is unpainted).
//!
//! Policy (product default):
//! - **Idle:** compositor scale only (honest). Opt-in supersample:
//!   `SOLA_BROWSER_SUPER_SAMPLE=1` restores old max(2.0) when scale is 1.
//! - **Scrolling:** clamp toward 1.0 so tiles keep up; restore after idle.
//! - **Override:** `SOLA_BROWSER_DPR=N` forces fixed scale always.
//!
//! Mark scroll from iced wheel; `choose_dpr` is called every prepare.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// After last scroll event, stay in scroll budget this long.
const SCROLL_IDLE_MS: u64 = 220;

static LAST_SCROLL_MS: AtomicU64 = AtomicU64::new(0);
static FORCED: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Call on every content scroll (wheel / touchpad).
pub fn note_scroll() {
    LAST_SCROLL_MS.store(now_ms(), Ordering::Relaxed);
}

fn scrolling_now() -> bool {
    let last = LAST_SCROLL_MS.load(Ordering::Relaxed);
    if last == 0 {
        return false;
    }
    now_ms().saturating_sub(last) < SCROLL_IDLE_MS
}

/// Device scale for WebKit content buffers + plane `buffer_scale`.
pub fn choose_dpr(compositor_scale: f64, css_w: u32, css_h: u32) -> f64 {
    let max_css = css_w.max(css_h).max(1) as f64;
    // Cap physical edge — matches prior MAX_PHYS_EDGE in frame.rs (~8k).
    const MAX_PHYS_EDGE: f64 = 8192.0;
    let edge_cap = (MAX_PHYS_EDGE / max_css).clamp(1.0, 2.0);

    if let Ok(s) = std::env::var("SOLA_BROWSER_DPR") {
        if let Ok(v) = s.parse::<f64>() {
            if v.is_finite() && v >= 1.0 {
                FORCED.store(true, Ordering::Relaxed);
                return v.min(edge_cap);
            }
        }
    }

    let comp = compositor_scale.max(1.0);
    let idle = if std::env::var_os("SOLA_BROWSER_SUPER_SAMPLE").is_some() {
        // Opt-in supersample when compositor reports 1.0 (soft-text fix).
        comp.max(2.0)
    } else {
        // Honest scale: paint budget matches display.
        comp
    };

    let want = if scrolling_now() {
        // Scroll budget: no supersample; prefer ≤1.25× so tiles keep up.
        idle.min(comp).min(1.25).max(1.0)
    } else {
        idle
    };
    want.min(edge_cap)
}

/// Whether we are currently in scroll paint budget (telem).
#[allow(dead_code)]
pub fn is_scroll_budget() -> bool {
    scrolling_now() && !FORCED.load(Ordering::Relaxed)
}

/// Idle duration exported for docs/tests.
#[allow(dead_code)]
pub fn scroll_idle() -> Duration {
    Duration::from_millis(SCROLL_IDLE_MS)
}
