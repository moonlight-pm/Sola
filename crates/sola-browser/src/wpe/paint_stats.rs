//! Paint-pipeline telemetry for scroll blackouts / chrome flicker.
//!
//! Counters are process-wide atomics (worker + iced threads). A periodic
//! flush logs a one-line summary when anything moved. Dump anytime with
//! [`PaintStats::log_snapshot`] or the Browser menu action `paint-stats`.
//!
//! Always logs at `info` when the interval saw activity. Immediate `warn`
//! on blackout-class events (large present gap, sample clear, live cap).
//!
//! How to read a `paint telem` line:
//! - `drop_ch` high → mailbox replaced an older frame (latest-wins; healthy under scroll)
//! - `drop_bg` high → inactive-tab presents released without claim
//! - `drop_cap` > 0 → live buffer cap; frames released untracked (blackout risk)
//! - `ignore` high → same WPE buffer re-presented while still held
//! - `prep_idle` ≫ `prep_new` → redraws without new imported frames
//! - `gap_present_ms` / `gap_import_ms` large → freeze or black gap
//! - `sample_clear` > 0 → bind group cleared (true black flash path)
//! - `yuv_skip` high → video/UI multi-plane not painted
//! - `since_import_ms` large → last texture is stale / frozen

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

static GLOBAL: OnceLock<Arc<PaintStats>> = OnceLock::new();

/// Process-wide paint telemetry (worker + iced).
pub fn global() -> &'static Arc<PaintStats> {
    GLOBAL.get_or_init(PaintStats::new)
}

/// Shared paint counters (cheap; hot path is fetch_add / max).
#[derive(Debug, Default)]
pub struct PaintStats {
    /// buffer-rendered callbacks (all tabs)
    pub presented: AtomicU64,
    /// claimed + enqueued for iced
    pub claimed: AtomicU64,
    /// mailbox replaced an older pending frame (latest-wins under load)
    pub drop_channel: AtomicU64,
    /// not the paint tab — dropped without import
    pub drop_bg: AtomicU64,
    /// live_buffers at MAX — released untracked
    pub drop_cap: AtomicU64,
    /// same buffer re-presented while still held
    pub ignore_repr: AtomicU64,
    /// NV12 / non-RGB multi-plane skip+release
    pub skip_yuv: AtomicU64,
    /// multi-plane ARGB/XRGB path
    pub multi_rgb: AtomicU64,
    /// wgpu import success / fail
    pub import_ok: AtomicU64,
    pub import_err: AtomicU64,
    /// Cmd::Release → WPE release
    pub released: AtomicU64,
    /// Cmd::Release skipped (stale claim / epoch)
    pub release_skip: AtomicU64,
    /// prepare ran with no pending frame (reuses last sample)
    pub prepare_idle: AtomicU64,
    /// prepare imported a new frame
    pub prepare_new: AtomicU64,
    /// NewFrame coalesced (redraw already queued)
    pub newframe_coalesce: AtomicU64,
    /// NewFrame delivered to iced
    pub newframe_sent: AtomicU64,
    /// shader sample bind group cleared (black path)
    pub sample_clear: AtomicU64,
    /// WebKit render fence waited OK / timed out / missing
    pub fence_ok: AtomicU64,
    pub fence_timeout: AtomicU64,
    pub fence_none: AtomicU64,
    /// last successful present wall time (ms since process start)
    last_present_ms: AtomicU64,
    /// last successful import wall time (ms since process start)
    last_import_ms: AtomicU64,
    /// max gap between presents this interval (ms)
    max_present_gap_ms: AtomicU64,
    /// max gap between imports this interval (ms)
    max_import_gap_ms: AtomicU64,
    /// max live_buffers observed this interval
    pub live_peak: AtomicU64,
    /// max outstanding tokens this interval
    pub outstanding_peak: AtomicU64,
    /// start Instant as millis baseline
    start: OnceLock<Instant>,
}

/// Present/import gap (ms) that warrants an immediate warn (blackout-class).
/// Idle pages often present ~1–2 Hz (~500–600 ms gaps); only flag real stalls.
const BLACKOUT_GAP_MS: u64 = 250;

impl PaintStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn now_ms(&self) -> u64 {
        let start = self.start.get_or_init(Instant::now);
        start.elapsed().as_millis() as u64
    }

    #[inline]
    pub fn inc(c: &AtomicU64) {
        c.fetch_add(1, Ordering::Relaxed);
    }

    fn note_max(slot: &AtomicU64, v: u64) {
        let _ = slot.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| {
            if v > p {
                Some(v)
            } else {
                None
            }
        });
    }

    /// Call on every buffer-rendered (before early returns that skip claim).
    pub fn note_present(&self) {
        Self::inc(&self.presented);
        let now = self.now_ms();
        let prev = self.last_present_ms.swap(now, Ordering::Relaxed);
        if prev > 0 {
            let gap = now.saturating_sub(prev);
            Self::note_max(&self.max_present_gap_ms, gap);
            if gap >= BLACKOUT_GAP_MS {
                tracing::warn!(
                    gap_ms = gap,
                    "paint telem: present gap (possible scroll blackout)"
                );
            }
        }
    }

    pub fn note_import_ok(&self) {
        Self::inc(&self.import_ok);
        let now = self.now_ms();
        let prev = self.last_import_ms.swap(now, Ordering::Relaxed);
        if prev > 0 {
            let gap = now.saturating_sub(prev);
            Self::note_max(&self.max_import_gap_ms, gap);
            if gap >= BLACKOUT_GAP_MS {
                tracing::warn!(
                    gap_ms = gap,
                    "paint telem: import gap (stale texture / lag)"
                );
            }
        }
    }

    /// Shader sample cleared — true black flash path.
    pub fn note_sample_clear(&self, why: &'static str) {
        Self::inc(&self.sample_clear);
        tracing::warn!(why, "paint telem: sample clear (black flash path)");
    }

    pub fn note_live(&self, live: usize, outstanding: u64) {
        Self::note_max(&self.live_peak, live as u64);
        Self::note_max(&self.outstanding_peak, outstanding);
    }

    /// ms since last successful import (`None` if never).
    pub fn ms_since_import(&self) -> Option<u64> {
        let last = self.last_import_ms.load(Ordering::Relaxed);
        if last == 0 {
            return None;
        }
        Some(self.now_ms().saturating_sub(last))
    }

    fn take(&self, c: &AtomicU64) -> u64 {
        c.swap(0, Ordering::Relaxed)
    }

    /// Log interval deltas and reset counters. Call every ~1–2s from worker.
    pub fn flush_interval(&self, live_now: usize, outstanding_now: u64) {
        let presented = self.take(&self.presented);
        let claimed = self.take(&self.claimed);
        let drop_channel = self.take(&self.drop_channel);
        let drop_bg = self.take(&self.drop_bg);
        let drop_cap = self.take(&self.drop_cap);
        let ignore_repr = self.take(&self.ignore_repr);
        let skip_yuv = self.take(&self.skip_yuv);
        let multi_rgb = self.take(&self.multi_rgb);
        let import_ok = self.take(&self.import_ok);
        let import_err = self.take(&self.import_err);
        let released = self.take(&self.released);
        let release_skip = self.take(&self.release_skip);
        let prepare_idle = self.take(&self.prepare_idle);
        let prepare_new = self.take(&self.prepare_new);
        let nf_coal = self.take(&self.newframe_coalesce);
        let nf_sent = self.take(&self.newframe_sent);
        let sample_clear = self.take(&self.sample_clear);
        let fence_timeout = self.take(&self.fence_timeout);
        let fence_ok = self.take(&self.fence_ok);
        let fence_none = self.take(&self.fence_none);
        let gap_present = self.take(&self.max_present_gap_ms);
        let gap_import = self.take(&self.max_import_gap_ms);
        let live_peak = self.take(&self.live_peak).max(live_now as u64);
        let out_peak = self.take(&self.outstanding_peak).max(outstanding_now);
        let since = self.ms_since_import();

        let activity = presented
            + claimed
            + drop_channel
            + drop_cap
            + import_ok
            + import_err
            + prepare_new
            + skip_yuv
            + sample_clear;
        if activity == 0 {
            return;
        }

        let drop_total = drop_channel + drop_bg + drop_cap;
        let drop_pct = if presented > 0 {
            (drop_total * 100) / presented
        } else {
            0
        };
        // Rough present rate over the flush window (~2s).
        let present_hz = presented / FLUSH_EVERY.as_secs().max(1);

        tracing::info!(
            present = presented,
            present_hz,
            claim = claimed,
            drop_ch = drop_channel,
            drop_bg = drop_bg,
            drop_cap = drop_cap,
            drop_pct,
            ignore = ignore_repr,
            yuv_skip = skip_yuv,
            multi_rgb = multi_rgb,
            import_ok,
            import_err,
            released,
            release_skip,
            prep_new = prepare_new,
            prep_idle = prepare_idle,
            nf_sent,
            nf_coal,
            sample_clear,
            fence_ok,
            fence_timeout,
            fence_none,
            gap_present_ms = gap_present,
            gap_import_ms = gap_import,
            live = live_now,
            live_peak,
            out = outstanding_now,
            out_peak,
            since_import_ms = since.unwrap_or(u64::MAX),
            "paint telem"
        );
    }

    /// Full snapshot without resetting (for bus dump / one-shot).
    pub fn log_snapshot(&self, why: &str, live_now: usize, outstanding_now: u64) {
        tracing::info!(
            why,
            present = self.presented.load(Ordering::Relaxed),
            claim = self.claimed.load(Ordering::Relaxed),
            drop_ch = self.drop_channel.load(Ordering::Relaxed),
            drop_bg = self.drop_bg.load(Ordering::Relaxed),
            drop_cap = self.drop_cap.load(Ordering::Relaxed),
            ignore = self.ignore_repr.load(Ordering::Relaxed),
            yuv_skip = self.skip_yuv.load(Ordering::Relaxed),
            multi_rgb = self.multi_rgb.load(Ordering::Relaxed),
            import_ok = self.import_ok.load(Ordering::Relaxed),
            import_err = self.import_err.load(Ordering::Relaxed),
            released = self.released.load(Ordering::Relaxed),
            release_skip = self.release_skip.load(Ordering::Relaxed),
            prep_new = self.prepare_new.load(Ordering::Relaxed),
            prep_idle = self.prepare_idle.load(Ordering::Relaxed),
            nf_sent = self.newframe_sent.load(Ordering::Relaxed),
            nf_coal = self.newframe_coalesce.load(Ordering::Relaxed),
            sample_clear = self.sample_clear.load(Ordering::Relaxed),
            gap_present_ms = self.max_present_gap_ms.load(Ordering::Relaxed),
            gap_import_ms = self.max_import_gap_ms.load(Ordering::Relaxed),
            live = live_now,
            outstanding = outstanding_now,
            since_import_ms = self.ms_since_import().unwrap_or(u64::MAX),
            "paint telem snapshot"
        );
    }
}

/// Interval for worker pump flushes.
pub const FLUSH_EVERY: Duration = Duration::from_secs(2);
