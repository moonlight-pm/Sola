//! What the host knows of the way to the viewer, from its acks: the round
//! trip, the least of it lately, the trip down, how long pictures wait
//! behind the ones before them, what got through, and from them when capture
//! skips a frame ([`Gate`]). Times are the host's clock in microseconds
//! ([`now_us`]); no clock is shared.
//!
//! A picture that went while a key picture was out says nothing of the
//! wait: a key picture is big by nature, and what waits behind it would make
//! a slow link look slower still. The gate keeps such a stretch short. The
//! key picture itself says what the way carries: hundreds of kilobytes cross
//! at about its speed.

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use super::rate::{CEILING_KBPS, Reading};

/// The least round trip is forgotten after this, so a path that got slower
/// is believed.
const BASE_FOR_US: u64 = 10_000_000;
/// What got through is counted over this.
const DELIVERED_OVER_US: u64 = 500_000;
/// Out at once: a round trip of frames, plus this for decoding, plus two.
const SLACK_US: u64 = 40_000;
const FEWEST_OUT: usize = 3;
const MOST_OUT: usize = 40;
/// The oldest picture out may be this much older than a round trip.
const LATE_US: u64 = 120_000;

/// The host's clock for timings: microseconds since the first call.
pub fn now_us() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_micros() as u64
}

struct Sent {
    seq: u32,
    at_us: u64,
    bytes: usize,
    key: bool,
    /// A key picture was out when it went, or it is one.
    after_key: bool,
}

#[derive(Default)]
pub struct Flow {
    /// Pictures sent and not yet acked, oldest first.
    sent: VecDeque<Sent>,
    rtt_us: u32,
    /// The least round trip, and when it was seen.
    base: Option<(u32, u64)>,
    /// When the last picture acked had all come: its ack, less the viewer's
    /// hold.
    came_us: Option<u64>,
    /// How long the last picture acked waited for the one before it to
    /// come; `None` when it says nothing of the way.
    wait_us: Option<u32>,
    /// How fast the last picture acked crossed, when it was a key picture.
    key_kbps: Option<u32>,
    /// Pictures acked lately: when, and their size.
    delivered: VecDeque<(u64, usize)>,
}

impl Flow {
    pub fn sent(&mut self, seq: u32, at_us: u64, bytes: usize, key: bool) {
        let after_key = key || self.sent.iter().any(|s| s.key);
        self.sent.push_back(Sent {
            seq,
            at_us,
            bytes,
            key,
            after_key,
        });
    }

    /// Picture `seq` was decoded `held_us` after it arrived, and its ack came
    /// at `at_us`. Pictures before it with no ack of their own go with it.
    /// How many pictures it took off.
    pub fn acked(&mut self, seq: u32, held_us: u32, at_us: u64) -> usize {
        let mut taken = 0;
        while let Some(front) = self.sent.front() {
            if (front.seq.wrapping_sub(seq) as i32) > 0 {
                break;
            }
            let sent = self.sent.pop_front().expect("front");
            taken += 1;
            self.delivered.push_back((at_us, sent.bytes));
            if sent.seq != seq {
                continue;
            }
            let came = at_us.saturating_sub(u64::from(held_us));
            let rtt = u32::try_from(came.saturating_sub(sent.at_us)).unwrap_or(u32::MAX).max(1);
            self.rtt_us = rtt;
            // Its own crossing is not waiting: only the time the one before
            // it still took, past a round trip, is.
            let ready = sent.at_us + u64::from(self.base_us());
            let wait = self.came_us.map_or(0, |before| before.saturating_sub(ready));
            self.came_us = Some(came);
            self.key_kbps = sent
                .key
                .then(|| u32::try_from(sent.bytes as u64 * 8 * 1_000 / u64::from(rtt)).unwrap_or(u32::MAX));
            if sent.after_key {
                self.wait_us = None;
                continue;
            }
            self.wait_us = Some(u32::try_from(wait).unwrap_or(u32::MAX));
            match self.base {
                Some((base, seen)) if base <= rtt && at_us.saturating_sub(seen) < BASE_FOR_US => {}
                _ => self.base = Some((rtt, at_us)),
            }
        }
        while self
            .delivered
            .front()
            .is_some_and(|&(at, _)| at_us.saturating_sub(at) > DELIVERED_OVER_US)
        {
            self.delivered.pop_front();
        }
        taken
    }

    fn base_us(&self) -> u32 {
        self.base.map_or(0, |(base, _)| base)
    }

    /// The trip down: the round trip less the way back. Acks are small and
    /// wait behind nothing, so the way back is half the least round trip.
    pub fn net_us(&self) -> u32 {
        self.base.map_or(0, |(base, _)| self.rtt_us.saturating_sub(base / 2))
    }

    /// What the last ack says of the way, at `at_us`, for the bitrate.
    pub fn reading(&self, at_us: u64) -> Reading {
        let bytes: usize = self.delivered.iter().map(|&(_, bytes)| bytes).sum();
        let kbps = bytes as u64 * 8 * 1_000 / DELIVERED_OVER_US;
        Reading {
            at_us,
            wait_us: self.wait_us,
            key_kbps: self.key_kbps,
            delivered_kbps: u32::try_from(kbps).unwrap_or(u32::MAX),
        }
    }
}

/// Between the session's thread, which keeps the [`Flow`] and the bitrate,
/// and capture, which reads this to skip a frame or change the bitrate.
pub struct Gate {
    /// Frames given to the encoder and not yet acked or dropped.
    in_flight: AtomicUsize,
    /// When the oldest picture not acked was sent; `u64::MAX` when none.
    oldest_us: AtomicU64,
    /// The least round trip lately; 0 before the first ack.
    base_us: AtomicU32,
    kbps: AtomicU32,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            in_flight: AtomicUsize::new(0),
            oldest_us: AtomicU64::new(u64::MAX),
            base_us: AtomicU32::new(0),
            kbps: AtomicU32::new(CEILING_KBPS),
        }
    }
}

impl Gate {
    /// Whether to encode a frame captured at `now_us`, frames coming
    /// `interval_us` apart. Not while more are out than a round trip and
    /// decoding need, nor while the oldest has waited too long: skipped
    /// before encoding, no later picture loses its reference. Counted out
    /// when it is.
    pub fn admit(&self, now_us: u64, interval_us: u32) -> bool {
        let base = u64::from(self.base_us.load(Ordering::Relaxed));
        let most = ((base + SLACK_US) / u64::from(interval_us.max(1)) + 2) as usize;
        let late = now_us.saturating_sub(self.oldest_us.load(Ordering::Relaxed)) > base + LATE_US;
        if late || self.in_flight.load(Ordering::Relaxed) >= most.clamp(FEWEST_OUT, MOST_OUT) {
            return false;
        }
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// A frame let through that will not be sent.
    pub fn dropped(&self) {
        self.take(1);
    }

    /// The bitrate to encode at.
    pub fn kbps(&self) -> u32 {
        self.kbps.load(Ordering::Relaxed)
    }

    /// After a picture is sent or acked: `acked` came back.
    pub fn follow(&self, flow: &Flow, acked: usize, kbps: u32) {
        self.take(acked);
        let oldest = flow.sent.front().map_or(u64::MAX, |s| s.at_us);
        self.oldest_us.store(oldest, Ordering::Relaxed);
        self.base_us.store(flow.base_us(), Ordering::Relaxed);
        self.kbps.store(kbps, Ordering::Relaxed);
    }

    fn take(&self, n: usize) {
        let mut out = self.in_flight.load(Ordering::Relaxed);
        while let Err(now) =
            self.in_flight
                .compare_exchange_weak(out, out.saturating_sub(n), Ordering::Relaxed, Ordering::Relaxed)
        {
            out = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 60 frames a second.
    const FRAME_US: u32 = 16_667;

    #[test]
    fn the_trip_down_is_the_round_trip_less_half_the_least() {
        let mut flow = Flow::default();
        assert_eq!(flow.net_us(), 0);
        flow.sent(1, 1_000, 100, false);
        flow.acked(1, 3_000, 6_000);
        // 5 ms out and back, 3 of them decoding: 2 ms, 1 each way.
        assert_eq!(flow.net_us(), 1_000);
        flow.sent(2, 10_000, 100, false);
        flow.acked(2, 3_000, 33_000);
        // 20 ms now: the queue is on the way down.
        assert_eq!(flow.net_us(), 19_000);
    }

    #[test]
    fn a_picture_waits_for_the_one_before_it_not_for_its_own_crossing() {
        let mut flow = Flow::default();
        flow.sent(1, 0, 100, false);
        flow.acked(1, 0, 2_000);
        // 40 ms crossing a slow link, alone: no wait.
        flow.sent(2, 10_000, 40_000, false);
        flow.acked(2, 0, 52_000);
        assert_eq!(flow.reading(52_000).wait_us, Some(0));
        // Sent 17 ms after it, this one waited until it had come.
        flow.sent(3, 27_000, 10_000, false);
        flow.sent(4, 44_000, 10_000, false);
        flow.acked(3, 0, 62_000);
        assert_eq!(flow.reading(62_000).wait_us, Some(23_000));
        flow.acked(4, 0, 72_000);
        assert_eq!(flow.reading(72_000).wait_us, Some(16_000));
    }

    #[test]
    fn pictures_behind_a_key_picture_say_nothing() {
        let mut flow = Flow::default();
        flow.sent(1, 0, 900_000, true);
        flow.sent(2, 17_000, 20_000, false);
        flow.acked(1, 0, 900_000);
        // 900 kB in 0.9 s: 8 Mbps.
        assert_eq!(flow.reading(900_000).key_kbps, Some(8_000));
        flow.acked(2, 0, 920_000);
        assert_eq!(flow.reading(900_000).key_kbps, None);
        assert_eq!(flow.reading(920_000).wait_us, None);
        assert_eq!(flow.base, None);
        flow.sent(3, 950_000, 20_000, false);
        flow.acked(3, 0, 972_000);
        assert_eq!(flow.reading(972_000).wait_us, Some(0));
        assert_eq!(flow.base, Some((22_000, 972_000)));
    }

    #[test]
    fn an_ack_takes_the_pictures_before_it() {
        let mut flow = Flow::default();
        for seq in 1..=3 {
            flow.sent(seq, u64::from(seq) * 1_000, 100, false);
        }
        assert_eq!(flow.acked(2, 0, 4_000), 2);
        assert_eq!(flow.sent.len(), 1);
        assert_eq!(flow.rtt_us, 2_000);
    }

    #[test]
    fn the_least_round_trip_is_forgotten_after_a_while() {
        let mut flow = Flow::default();
        flow.sent(1, 0, 100, false);
        flow.acked(1, 0, 2_000);
        flow.sent(2, 5_000, 100, false);
        flow.acked(2, 0, 13_000);
        assert_eq!(flow.base, Some((2_000, 2_000)));
        flow.sent(3, BASE_FOR_US + 3_000, 100, false);
        flow.acked(3, 0, BASE_FOR_US + 11_000);
        assert_eq!(flow.base, Some((8_000, BASE_FOR_US + 11_000)));
    }

    #[test]
    fn what_got_through_is_the_last_half_second() {
        let mut flow = Flow::default();
        for seq in 1..=60u32 {
            let at = u64::from(seq) * u64::from(FRAME_US);
            flow.sent(seq, at, 25_000, false);
            flow.acked(seq, 0, at + 1_000);
        }
        // 30 pictures of 25 kB in half a second: 12 Mbps.
        let kbps = flow.reading(60 * u64::from(FRAME_US) + 1_000).delivered_kbps;
        assert!((11_900..=12_500).contains(&kbps), "{kbps}");
    }

    #[test]
    fn the_gate_lets_through_a_round_trip_of_frames() {
        let (gate, mut flow) = (Gate::default(), Flow::default());
        // Before any ack: 40 ms of frames and two more, 4.
        assert_eq!((0..10).filter(|_| gate.admit(0, FRAME_US)).count(), 4);
        // A 100 ms round trip lets 10 out.
        flow.sent(1, 0, 100, false);
        flow.acked(1, 0, 100_000);
        gate.follow(&flow, 1, CEILING_KBPS);
        assert_eq!((0..20).filter(|_| gate.admit(100_000, FRAME_US)).count(), 7);
        gate.dropped();
        assert!(gate.admit(100_000, FRAME_US));
    }

    #[test]
    fn the_gate_closes_while_the_oldest_waits_too_long() {
        let (gate, mut flow) = (Gate::default(), Flow::default());
        flow.sent(1, 1_000, 100, false);
        gate.follow(&flow, 0, CEILING_KBPS);
        assert!(gate.admit(1_000 + LATE_US, FRAME_US));
        assert!(!gate.admit(2_000 + LATE_US, FRAME_US));
        flow.acked(1, 0, 3_000 + LATE_US);
        gate.follow(&flow, 1, CEILING_KBPS);
        assert!(gate.admit(4_000 + LATE_US, FRAME_US));
    }
}
