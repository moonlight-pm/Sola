//! Low-overhead lag diagnostics for the novus send path.
//!
//! Periodic window summaries at `info`, plus immediate `warn` on send stalls
//! or large pace hold-ups. Watch:
//!
//! ```text
//! grep -E 'kvm-metrics|LAG' /opt/sola/log/sola.log
//! ```

use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::protocol::Packet;

const WINDOW: Duration = Duration::from_secs(2);
const SLOW_SEND: Duration = Duration::from_millis(4);

#[derive(Debug, Default)]
pub struct Metrics {
    window_start: Option<Instant>,
    remote: bool,
    packets: u64,
    motions: u64,
    scrolls: u64,
    keys: u64,
    buttons: u64,
    enter_leave: u64,
    /// Motions absorbed by the pacer (not sent this event).
    motion_paced: u64,
    send_us_sum: u64,
    send_us_max: u64,
    slow_sends: u64,
    send_errors: u64,
    events_in: u64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_remote(&mut self, remote: bool) {
        if remote != self.remote {
            // Close the previous window when crossing local↔remote.
            self.flush_window();
            self.remote = remote;
            info!(remote, "kvm-metrics mode");
        }
    }

    pub fn on_input_events(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.ensure_window();
        self.events_in += n as u64;
    }

    pub fn on_paced_motion(&mut self) {
        self.ensure_window();
        self.motion_paced += 1;
    }

    pub fn on_send(&mut self, packet: &Packet, send_elapsed: Duration, err: bool) {
        self.ensure_window();
        self.packets += 1;
        match packet {
            Packet::Motion { .. } => self.motions += 1,
            Packet::Scroll { .. } => self.scrolls += 1,
            Packet::Key { .. } => self.keys += 1,
            Packet::Button { .. } => self.buttons += 1,
            Packet::Enter { .. } | Packet::Leave => self.enter_leave += 1,
            Packet::Modifiers { .. } => {}
        }

        let us = micros(send_elapsed);
        self.send_us_sum = self.send_us_sum.saturating_add(us);
        self.send_us_max = self.send_us_max.max(us);
        if err {
            self.send_errors += 1;
        }
        if send_elapsed >= SLOW_SEND {
            self.slow_sends += 1;
            warn!(
                send_ms = send_elapsed.as_secs_f64() * 1000.0,
                ?packet,
                err,
                "LAG spike (novus UDP send)"
            );
        }

        self.maybe_flush_window(false);
    }

    pub fn on_idle_tick(&mut self) {
        // Only emit while remote so local desk doesn't spam.
        if self.remote {
            self.maybe_flush_window(true);
        }
    }

    fn ensure_window(&mut self) {
        if self.window_start.is_none() {
            self.window_start = Some(Instant::now());
        }
    }

    fn maybe_flush_window(&mut self, force_if_due: bool) {
        let Some(start) = self.window_start else {
            return;
        };
        let elapsed = start.elapsed();
        if elapsed < WINDOW {
            return;
        }
        if !force_if_due && self.packets == 0 && self.events_in == 0 {
            self.window_start = Some(Instant::now());
            return;
        }
        self.flush_window();
        let _ = elapsed;
    }

    fn flush_window(&mut self) {
        let Some(start) = self.window_start else {
            return;
        };
        let elapsed = start.elapsed();
        if self.packets == 0 && self.events_in == 0 && self.motion_paced == 0 {
            self.window_start = Some(Instant::now());
            return;
        }

        let send_avg_us = if self.packets > 0 {
            self.send_us_sum / self.packets
        } else {
            0
        };
        let motion_hz = if elapsed.as_secs_f64() > 0.0 {
            self.motions as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        info!(
            remote = self.remote,
            secs = elapsed.as_secs_f64(),
            events_in = self.events_in,
            packets = self.packets,
            motions = self.motions,
            scrolls = self.scrolls,
            keys = self.keys,
            buttons = self.buttons,
            enter_leave = self.enter_leave,
            motion_paced = self.motion_paced,
            slow_sends = self.slow_sends,
            send_errors = self.send_errors,
            send_avg_ms = send_avg_us as f64 / 1000.0,
            send_max_ms = self.send_us_max as f64 / 1000.0,
            motion_hz,
            "kvm-metrics novus window"
        );

        let remote = self.remote;
        *self = Self::default();
        self.remote = remote;
        self.window_start = Some(Instant::now());
    }
}

fn micros(d: Duration) -> u64 {
    d.as_micros().min(u64::MAX as u128) as u64
}
