//! The bitrate, following the queue on the way to the viewer: cut when
//! pictures wait, raised slowly when they do not and the screen uses it.
//! Each step is taken on an ack; the intervals below pace that control loop,
//! and nothing waits on them.

/// The bitrate at the start, and the most it goes to: the LAN is the usual
/// case, and the first picture should be sharp.
pub const CEILING_KBPS: u32 = 50_000;
/// The least it goes to.
const FLOOR_KBPS: u32 = 2_000;
/// A picture waiting longer than this behind the ones before it is
/// congestion.
const CONGESTED_WAIT_US: u32 = 30_000;
/// Under this the way is clear.
const CLEAR_WAIT_US: u32 = 15_000;
const CUT_EVERY_US: u64 = 300_000;
const RAISE_AFTER_CUT_US: u64 = 1_000_000;
const RAISE_EVERY_US: u64 = 200_000;
/// What got through at the last congestion is forgotten after this, so a
/// way that got faster is used.
const CAPACITY_FOR_US: u64 = 15_000_000;

/// What an ack says of the way down.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reading {
    pub at_us: u64,
    /// How long the picture waited behind the ones before it; `None` when
    /// it says nothing of the way.
    pub wait_us: Option<u32>,
    /// How fast a key picture crossed.
    pub key_kbps: Option<u32>,
    /// What got through in the last half second.
    pub delivered_kbps: u32,
}

pub struct Rate {
    kbps: u32,
    cut_at: Option<u64>,
    raised_at: Option<u64>,
    /// What got through at the last congestion, and when.
    capacity: Option<(u32, u64)>,
}

impl Default for Rate {
    fn default() -> Self {
        Self {
            kbps: CEILING_KBPS,
            cut_at: None,
            raised_at: None,
            capacity: None,
        }
    }
}

fn percent(kbps: u32, percent: u64) -> u32 {
    u32::try_from(u64::from(kbps) * percent / 100).unwrap_or(u32::MAX)
}

impl Rate {
    pub fn kbps(&self) -> u32 {
        self.kbps
    }

    /// Follow one ack's reading.
    pub fn follow(&mut self, r: &Reading) {
        // A key picture slower than the bitrate: the way carries no more.
        if let Some(carried) = r.key_kbps.filter(|&k| k < self.kbps) {
            self.kbps = percent(carried, 85).max(FLOOR_KBPS);
            self.capacity = Some((carried, r.at_us));
            self.cut_at = Some(r.at_us);
        }
        let Some(wait) = r.wait_us else {
            return;
        };
        let since = |at: Option<u64>| at.map_or(u64::MAX, |at| r.at_us.saturating_sub(at));
        if since(self.capacity.map(|(_, at)| at)) >= CAPACITY_FOR_US {
            self.capacity = None;
        }
        // Only a screen that presses on the bitrate says, by what gets
        // through, what the way carries.
        let pressed = r.delivered_kbps >= self.kbps / 2;
        if wait > CONGESTED_WAIT_US {
            if since(self.cut_at) >= CUT_EVERY_US {
                let mut next = percent(self.kbps, 85);
                if pressed {
                    next = next.min(percent(r.delivered_kbps, 85));
                    self.capacity = Some((r.delivered_kbps, r.at_us));
                }
                self.kbps = next.max(FLOOR_KBPS);
                self.cut_at = Some(r.at_us);
            }
        } else if wait < CLEAR_WAIT_US
            && pressed
            && since(self.cut_at) >= RAISE_AFTER_CUT_US
            && since(self.raised_at) >= RAISE_EVERY_US
        {
            let top = self
                .capacity
                .map_or(CEILING_KBPS, |(kbps, _)| percent(kbps, 90).min(CEILING_KBPS));
            let next = (percent(self.kbps, 106) + 100).min(top);
            if next > self.kbps {
                self.kbps = next;
                self.raised_at = Some(r.at_us);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000;

    fn congested(at_ms: u64, delivered_kbps: u32) -> Reading {
        Reading {
            at_us: at_ms * MS,
            wait_us: Some(50_000),
            key_kbps: None,
            delivered_kbps,
        }
    }

    fn clear(at_ms: u64, delivered_kbps: u32) -> Reading {
        Reading {
            at_us: at_ms * MS,
            wait_us: Some(1_000),
            key_kbps: None,
            delivered_kbps,
        }
    }

    #[test]
    fn congestion_cuts_to_under_what_got_through_at_most_every_300_ms() {
        let mut rate = Rate::default();
        rate.follow(&congested(1_000, 30_000));
        assert_eq!(rate.kbps(), 25_500);
        rate.follow(&congested(1_200, 20_000));
        assert_eq!(rate.kbps(), 25_500);
        rate.follow(&congested(1_300, 20_000));
        assert_eq!(rate.kbps(), 17_000);
    }

    #[test]
    fn a_quiet_screen_is_cut_but_says_nothing_of_the_way() {
        let mut rate = Rate::default();
        // One big picture on a still screen: 1 Mbps got through.
        rate.follow(&congested(1_000, 1_000));
        assert_eq!(rate.kbps(), 42_500);
        assert_eq!(rate.capacity, None);
    }

    #[test]
    fn a_slow_key_picture_brings_the_bitrate_down_to_it() {
        let mut rate = Rate::default();
        let key = |at_ms, kbps| Reading {
            key_kbps: Some(kbps),
            wait_us: None,
            ..clear(at_ms, 0)
        };
        // Across a LAN it says nothing.
        rate.follow(&key(1_000, 700_000));
        assert_eq!(rate.kbps(), CEILING_KBPS);
        rate.follow(&key(2_000, 8_000));
        assert_eq!(rate.kbps(), 6_800);
        // And it rises no further than 90% of it.
        rate.follow(&clear(3_000, 6_000));
        assert_eq!(rate.kbps(), 7_200);
    }

    #[test]
    fn a_reading_that_says_nothing_changes_nothing() {
        let mut rate = Rate::default();
        rate.follow(&Reading {
            wait_us: None,
            ..congested(1_000, 30_000)
        });
        assert_eq!(rate.kbps(), CEILING_KBPS);
    }

    #[test]
    fn it_rises_a_second_after_a_cut_to_under_what_got_through() {
        let mut rate = Rate::default();
        rate.follow(&congested(1_000, 30_000));
        assert_eq!(rate.kbps(), 25_500);
        rate.follow(&clear(1_900, 20_000));
        assert_eq!(rate.kbps(), 25_500);
        // Up 6% and 100 kbps, but no further than 90% of the 30 Mbps.
        rate.follow(&clear(2_000, 20_000));
        assert_eq!(rate.kbps(), 27_000);
        rate.follow(&clear(2_200, 20_000));
        assert_eq!(rate.kbps(), 27_000);
        // Forgotten after 15 s, it goes on, every 200 ms.
        rate.follow(&clear(16_000, 20_000));
        assert_eq!(rate.kbps(), 28_720);
        rate.follow(&clear(16_100, 20_000));
        assert_eq!(rate.kbps(), 28_720);
    }

    #[test]
    fn it_does_not_rise_for_a_screen_that_does_not_use_it() {
        let mut rate = Rate::default();
        rate.follow(&congested(1_000, 30_000));
        rate.follow(&clear(3_000, 1_000));
        assert_eq!(rate.kbps(), 25_500);
    }

    #[test]
    fn it_stays_between_the_floor_and_the_ceiling() {
        let mut rate = Rate::default();
        for i in 0..40 {
            rate.follow(&congested(i * 300, 0));
        }
        assert_eq!(rate.kbps(), FLOOR_KBPS);
        let mut rate = Rate::default();
        rate.follow(&clear(1_000, 50_000));
        assert_eq!(rate.kbps(), CEILING_KBPS);
    }
}
