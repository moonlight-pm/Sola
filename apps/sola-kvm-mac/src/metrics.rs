//! Low-overhead lag diagnostics for the Mac inject path.
//!
//! Always-on, rate-limited: periodic window summaries at `info`, and immediate
//! `warn` when a single batch looks like a lag episode (large socket backlog
//! or slow inject). Watch with:
//!
//! ```text
//! tail -f ~/Library/Logs/sola-kvm-mac.out.log | grep -E 'kvm-metrics|LAG'
//! ```

use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::protocol::Packet;

/// Emit a window summary at least this often while traffic is flowing.
const WINDOW: Duration = Duration::from_secs(2);

/// Batch inject wall time that counts as a lag episode.
const SLOW_INJECT: Duration = Duration::from_millis(8);

/// Socket drain size (pre-coalesce) that counts as a backlog episode.
/// Only warned when *combined* with slow inject — pure backlog with fast
/// inject is expected when coalescing and is noisy as a WARN.
const BACKLOG_PKTS: usize = 16;

#[derive(Debug, Default)]
pub struct Metrics {
    window_start: Option<Instant>,
    /// Datagrams drained this window (pre-coalesce).
    pkts_in: u64,
    /// Events actually injected this window (post-coalesce).
    pkts_out: u64,
    motions: u64,
    scrolls: u64,
    keys: u64,
    buttons: u64,
    enter_leave: u64,
    motion_collapsed: u64,
    batches: u64,
    /// Batches with inject_ms >= SLOW_INJECT.
    slow_batches: u64,
    /// Batches with pre-coalesce size >= BACKLOG_PKTS.
    backlog_batches: u64,
    inject_us_sum: u64,
    inject_us_max: u64,
    batch_in_max: u64,
    /// Longest gap between end of one inject batch and start of the next drain.
    gap_us_max: u64,
    last_inject_end: Option<Instant>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call once per drained+injected batch.
    pub fn on_batch(
        &mut self,
        pre_coalesce: usize,
        collapsed: usize,
        packets: &[(u32, Packet)],
        inject_elapsed: Duration,
    ) {
        let now = Instant::now();
        if self.window_start.is_none() {
            self.window_start = Some(now);
        }

        if let Some(prev_end) = self.last_inject_end {
            let gap = now.saturating_duration_since(prev_end);
            self.gap_us_max = self.gap_us_max.max(micros(gap));
        }

        self.batches += 1;
        self.pkts_in += pre_coalesce as u64;
        self.pkts_out += packets.len() as u64;
        self.motion_collapsed += collapsed as u64;
        self.batch_in_max = self.batch_in_max.max(pre_coalesce as u64);

        let inject_us = micros(inject_elapsed);
        self.inject_us_sum = self.inject_us_sum.saturating_add(inject_us);
        self.inject_us_max = self.inject_us_max.max(inject_us);

        for (_, p) in packets {
            match p {
                Packet::Motion { .. } => self.motions += 1,
                Packet::Scroll { .. } => self.scrolls += 1,
                Packet::Key { .. } => self.keys += 1,
                Packet::Button { .. } => self.buttons += 1,
                Packet::Enter { .. } | Packet::Leave => self.enter_leave += 1,
                Packet::Modifiers { .. } => {}
            }
        }

        let slow = inject_elapsed >= SLOW_INJECT;
        let backlog = pre_coalesce >= BACKLOG_PKTS;
        if slow {
            self.slow_batches += 1;
        }
        if backlog {
            self.backlog_batches += 1;
        }

        // Immediate spike: only when inject itself is slow (the real lag
        // source). Backlog alone with fast inject is normal with latest-wins.
        if slow {
            warn!(
                pre = pre_coalesce,
                out = packets.len(),
                collapsed,
                inject_ms = inject_elapsed.as_secs_f64() * 1000.0,
                motions = count_ty(packets, is_motion),
                scrolls = count_ty(packets, is_scroll),
                keys = count_ty(packets, is_key),
                buttons = count_ty(packets, is_button),
                "LAG spike (ember inject batch)"
            );
        }

        self.last_inject_end = Some(Instant::now());
        self.maybe_flush_window(false);
    }

    /// Idle poll timeout — still flush a window if one is open.
    pub fn on_idle_tick(&mut self) {
        self.maybe_flush_window(true);
    }

    fn maybe_flush_window(&mut self, force_if_open: bool) {
        let Some(start) = self.window_start else {
            return;
        };
        let elapsed = start.elapsed();
        if elapsed < WINDOW && !force_if_open {
            return;
        }
        if self.batches == 0 {
            // Fully idle window — don't spam.
            if elapsed >= WINDOW {
                self.reset_window();
            }
            return;
        }
        if elapsed < WINDOW && force_if_open {
            return;
        }

        let inject_avg_us = if self.batches > 0 {
            self.inject_us_sum / self.batches
        } else {
            0
        };
        let hz = if elapsed.as_secs_f64() > 0.0 {
            self.motions as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        info!(
            secs = elapsed.as_secs_f64(),
            batches = self.batches,
            pkts_in = self.pkts_in,
            pkts_out = self.pkts_out,
            motions = self.motions,
            scrolls = self.scrolls,
            keys = self.keys,
            buttons = self.buttons,
            enter_leave = self.enter_leave,
            motion_collapsed = self.motion_collapsed,
            slow_batches = self.slow_batches,
            backlog_batches = self.backlog_batches,
            inject_avg_ms = inject_avg_us as f64 / 1000.0,
            inject_max_ms = self.inject_us_max as f64 / 1000.0,
            batch_in_max = self.batch_in_max,
            gap_max_ms = self.gap_us_max as f64 / 1000.0,
            motion_hz = hz,
            "kvm-metrics ember window"
        );

        self.reset_window();
    }

    fn reset_window(&mut self) {
        *self = Self {
            last_inject_end: self.last_inject_end,
            ..Self::default()
        };
        self.window_start = Some(Instant::now());
    }
}

fn micros(d: Duration) -> u64 {
    d.as_micros().min(u64::MAX as u128) as u64
}

fn count_ty(packets: &[(u32, Packet)], pred: fn(&Packet) -> bool) -> usize {
    packets.iter().filter(|(_, p)| pred(p)).count()
}

fn is_motion(p: &Packet) -> bool {
    matches!(p, Packet::Motion { .. })
}
fn is_scroll(p: &Packet) -> bool {
    matches!(p, Packet::Scroll { .. })
}
fn is_key(p: &Packet) -> bool {
    matches!(p, Packet::Key { .. })
}
fn is_button(p: &Packet) -> bool {
    matches!(p, Packet::Button { .. })
}
