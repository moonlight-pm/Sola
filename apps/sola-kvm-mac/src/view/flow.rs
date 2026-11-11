//! What the host knows of the way to the viewer, from its acks: the round
//! trip, the least of it lately, and from them the trip down. Times are the
//! host's clock in microseconds ([`now_us`]); no clock is shared.

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Instant;

/// The encoder's bitrate.
pub const CEILING_KBPS: u32 = 50_000;
/// The least round trip is forgotten after this, so a path that got slower
/// is believed.
const BASE_FOR_US: u64 = 10_000_000;

/// The host's clock for timings: microseconds since the first call.
pub fn now_us() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_micros() as u64
}

struct Sent {
    seq: u32,
    at_us: u64,
}

#[derive(Default)]
pub struct Flow {
    /// Pictures sent and not yet acked, oldest first.
    sent: VecDeque<Sent>,
    rtt_us: u32,
    /// The least round trip, and when it was seen.
    base: Option<(u32, u64)>,
}

impl Flow {
    pub fn sent(&mut self, seq: u32, at_us: u64) {
        self.sent.push_back(Sent { seq, at_us });
    }

    /// Picture `seq` was decoded `held_us` after it arrived, and its ack came
    /// at `at_us`. Pictures before it with no ack of their own go with it.
    pub fn acked(&mut self, seq: u32, held_us: u32, at_us: u64) {
        while let Some(front) = self.sent.front() {
            if (front.seq.wrapping_sub(seq) as i32) > 0 {
                break;
            }
            let sent = self.sent.pop_front().expect("front");
            if sent.seq != seq {
                continue;
            }
            let rtt = at_us.saturating_sub(sent.at_us).saturating_sub(u64::from(held_us));
            let rtt = u32::try_from(rtt).unwrap_or(u32::MAX).max(1);
            self.rtt_us = rtt;
            match self.base {
                Some((base, seen)) if base <= rtt && at_us.saturating_sub(seen) < BASE_FOR_US => {}
                _ => self.base = Some((rtt, at_us)),
            }
        }
    }

    /// The trip down: the round trip less the way back. Acks are small and
    /// wait behind nothing, so the way back is half the least round trip.
    pub fn net_us(&self) -> u32 {
        self.base.map_or(0, |(base, _)| self.rtt_us.saturating_sub(base / 2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trip_down_is_the_round_trip_less_half_the_least() {
        let mut flow = Flow::default();
        assert_eq!(flow.net_us(), 0);
        flow.sent(1, 1_000);
        flow.acked(1, 3_000, 6_000);
        // 5 ms out and back, 3 of them decoding: 2 ms, 1 each way.
        assert_eq!(flow.net_us(), 1_000);
        flow.sent(2, 10_000);
        flow.acked(2, 3_000, 33_000);
        // 20 ms now: the queue is on the way down.
        assert_eq!(flow.net_us(), 19_000);
    }

    #[test]
    fn an_ack_takes_the_pictures_before_it() {
        let mut flow = Flow::default();
        for seq in 1..=3 {
            flow.sent(seq, u64::from(seq) * 1_000);
        }
        flow.acked(2, 0, 4_000);
        assert_eq!(flow.sent.len(), 1);
        assert_eq!(flow.rtt_us, 2_000);
    }

    #[test]
    fn the_least_round_trip_is_forgotten_after_a_while() {
        let mut flow = Flow::default();
        flow.sent(1, 0);
        flow.acked(1, 0, 2_000);
        flow.sent(2, 5_000);
        flow.acked(2, 0, 13_000);
        assert_eq!(flow.base, Some((2_000, 2_000)));
        flow.sent(3, BASE_FOR_US + 3_000);
        flow.acked(3, 0, BASE_FOR_US + 11_000);
        assert_eq!(flow.base, Some((8_000, BASE_FOR_US + 11_000)));
    }
}
