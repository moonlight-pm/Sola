//! Memory budget: Design holds its boards to what the machine has free.
//!
//! There is no fixed size. Design keeps [`reserve`] of the machine's memory
//! free for everything else (`MemAvailable` in `/proc/meminfo`) and fits
//! its boards in the rest. Each open board is costed by [`board_cost`]
//! (measured: about ten copies of its frame across the CEF processes, plus
//! a small base). Opens and discards take a few seconds to show in
//! `MemAvailable`, so a [`Ledger`] carries them until they land.
//!
//! Over budget, idle boards are discarded first ([`pick`]: least recently
//! used); an agent is refused only when the boards in use still do not fit
//! (decision log 2026-10-13).

use std::collections::HashMap;
use std::time::{Duration, Instant};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Share of the machine's memory kept free for everything else.
const RESERVE_PCT: u64 = 15;
/// Kept free on a small machine whatever the share says.
const RESERVE_MIN: u64 = 2 * GIB;
/// Per board on top of its frames: renderer heap, tab state.
const BOARD_BASE: u64 = 64 * MIB;
/// Copies of a board's frame its CEF processes hold (renderer tiles,
/// compositor output, helper frame, IPC, plane texture). Measured
/// 2026-10-13: ten boards with 295 MiB of frames took ~2.9 GiB.
const FRAME_COPIES: u64 = 9;
/// Discards free this much beyond the reserve, so pressure does not
/// discard one board per tick.
pub const SLACK: u64 = GIB;
/// An open is carried until its board settles, at most this long.
const OPEN_CARRY: Duration = Duration::from_secs(20);
/// A discard is carried this long: the renderer exits at once, but the
/// CEF GPU process hands its shared memory back 10–20 s later (measured
/// 2026-10-14: ten closed boards' ~2.9 GiB returned within ~20 s).
const FREE_CARRY: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemInfo {
    /// `MemTotal`, bytes.
    pub total: u64,
    /// `MemAvailable`, bytes.
    pub available: u64,
}

/// `MemTotal` and `MemAvailable` from `/proc/meminfo` text.
pub fn parse_meminfo(text: &str) -> Option<MemInfo> {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map(|kib| kib * 1024)
    };
    Some(MemInfo {
        total: field("MemTotal")?,
        available: field("MemAvailable")?,
    })
}

/// The machine's memory now (`None` off Linux: no budget).
pub fn read() -> Option<MemInfo> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// Bytes Design leaves free for the rest of the machine.
pub fn reserve(total: u64) -> u64 {
    (total / 100 * RESERVE_PCT).max(RESERVE_MIN)
}

/// Estimated bytes one open board holds, from its OSR surface in px.
pub fn board_cost(surface: (u32, u32)) -> u64 {
    let frame = u64::from(surface.0) * u64::from(surface.1) * 4;
    BOARD_BASE + FRAME_COPIES * frame
}

/// Opens and discards `MemAvailable` does not show yet.
#[derive(Debug, Default)]
pub struct Ledger {
    /// Board tab → (cost, opened at).
    opens: HashMap<u64, (u64, Instant)>,
    /// (cost, discarded at).
    frees: Vec<(u64, Instant)>,
}

impl Ledger {
    /// Board `tab` opened: it will take about `cost`.
    pub fn open(&mut self, tab: u64, cost: u64, now: Instant) {
        self.opens.insert(tab, (cost, now));
    }

    /// Board `tab` has loaded (or closed): `MemAvailable` holds it now.
    pub fn landed(&mut self, tab: u64) {
        self.opens.remove(&tab);
    }

    /// Boards still being carried as opening.
    pub fn opening(&self) -> impl Iterator<Item = u64> + '_ {
        self.opens.keys().copied()
    }

    /// A board was discarded or closed: about `cost` comes back.
    pub fn freed(&mut self, cost: u64, now: Instant) {
        self.frees.push((cost, now));
    }

    /// Drop what `MemAvailable` has caught up with.
    pub fn expire(&mut self, now: Instant) {
        self.opens
            .retain(|_, (_, at)| now.duration_since(*at) < OPEN_CARRY);
        self.frees
            .retain(|(_, at)| now.duration_since(*at) < FREE_CARRY);
    }

    /// `available` as it will be once the carried opens and frees land.
    pub fn project(&self, available: u64) -> u64 {
        let opening: u64 = self.opens.values().map(|(c, _)| c).sum();
        let freeing: u64 = self.frees.iter().map(|(c, _)| c).sum();
        (available + freeing).saturating_sub(opening)
    }
}

/// A board that may be discarded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub tab: u64,
    pub cost: u64,
    /// Last looked at or worked on.
    pub used: Instant,
}

/// Boards to discard for `need` bytes: least recently used first, only as
/// many as it takes. Fewer than `need` when the candidates run out.
pub fn pick(mut candidates: Vec<Candidate>, need: u64) -> Vec<Candidate> {
    candidates.sort_by_key(|c| c.used);
    let mut got = 0;
    let mut out = Vec::new();
    for c in candidates {
        if got >= need {
            break;
        }
        got += c.cost;
        out.push(c);
    }
    out
}

/// `3.1 GB`, `640 MB`: what the human reads (powers of 1024, short).
pub fn size(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    } else {
        format!("{} MB", bytes.div_ceil(MIB))
    }
}

/// Whole MiB, for `status`.
pub fn mib(bytes: u64) -> u64 {
    bytes / MIB
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_total_and_available() {
        let text = "MemTotal:       131698832 kB\nMemFree:        79001200 kB\nMemAvailable:   102245632 kB\n";
        let m = parse_meminfo(text).unwrap();
        assert_eq!(m.total, 131_698_832 * 1024);
        assert_eq!(m.available, 102_245_632 * 1024);
        assert!(parse_meminfo("MemTotal: 1 kB\n").is_none());
    }

    #[test]
    fn reserve_is_a_share_with_a_floor() {
        assert_eq!(reserve(100 * GIB), 15 * GIB);
        assert_eq!(reserve(4 * GIB), RESERVE_MIN);
    }

    #[test]
    fn a_board_costs_its_frames_and_a_base() {
        // 1440 × 2000 CSS at 2×: a 44 MiB frame.
        let cost = board_cost((2880, 4000));
        assert_eq!(cost, BOARD_BASE + 9 * 2880 * 4000 * 4);
        assert!(cost > 400 * MIB && cost < 500 * MIB, "{}", size(cost));
    }

    #[test]
    fn the_ledger_carries_until_things_land() {
        let mut l = Ledger::default();
        let t0 = Instant::now();
        l.open(1, 3 * GIB, t0);
        l.freed(GIB, t0);
        assert_eq!(l.project(10 * GIB), 8 * GIB);
        l.landed(1);
        assert_eq!(l.project(10 * GIB), 11 * GIB);
        l.expire(t0 + FREE_CARRY);
        assert_eq!(l.project(10 * GIB), 10 * GIB);
        l.open(2, 20 * GIB, t0);
        assert_eq!(l.project(10 * GIB), 0, "never below zero");
        l.expire(t0 + OPEN_CARRY);
        assert_eq!(l.opening().count(), 0);
    }

    #[test]
    fn pick_takes_the_least_recently_used_until_enough() {
        let t0 = Instant::now();
        let c = |tab, secs| Candidate {
            tab,
            cost: GIB,
            used: t0 + Duration::from_secs(secs),
        };
        let got = pick(vec![c(1, 30), c(2, 10), c(3, 20)], GIB + 1);
        assert_eq!(got.iter().map(|c| c.tab).collect::<Vec<_>>(), vec![2, 3]);
        assert!(pick(vec![c(1, 0)], 0).is_empty());
        assert_eq!(pick(vec![c(1, 0)], 5 * GIB).len(), 1, "all there is");
    }

    #[test]
    fn sizes_read_short() {
        assert_eq!(size(3 * GIB + GIB / 10), "3.1 GB");
        assert_eq!(size(640 * MIB), "640 MB");
        assert_eq!(size(1), "1 MB");
    }
}
