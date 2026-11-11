//! Statistics: each second of pictures rolled up, for the window's corner
//! (View → Statistics) and `--measure`. They move only when pictures come,
//! so a still screen leaves the last second showing.

use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SECOND: Duration = Duration::from_secs(1);

/// What one picture says of its way here.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    pub bytes: usize,
    /// Capture to sending, on the host.
    pub host_us: u32,
    /// The host's estimate of the trip here.
    pub net_us: u32,
    /// Arrived to decoded.
    pub decode_us: u32,
    pub kbps: u32,
    pub skipped: u16,
}

impl Sample {
    fn to_decoded_us(&self) -> u32 {
        self.host_us.saturating_add(self.net_us).saturating_add(self.decode_us)
    }
}

/// A decoded picture on its way to the GPU: what it took so far, and when
/// it was decoded.
#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub to_decoded_us: u32,
    pub decoded: Instant,
}

/// A span of pictures, rolled up.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub pictures: u32,
    pub shown: u32,
    pub seconds: f64,
    pub mbps: f64,
    /// The host's bitrate at the last picture.
    pub kbps: u32,
    pub skipped: u32,
    pub host_ms: f64,
    pub net_ms: f64,
    pub decode_ms: f64,
    /// Decoded to on the GPU; `None` with no window.
    pub show_ms: Option<f64>,
    /// Capture to shown (to decoded with no window): median and worst.
    pub delay_ms: f64,
    pub worst_ms: f64,
}

impl Summary {
    pub fn fps(&self) -> f64 {
        per_second(self.pictures, self.seconds)
    }

    pub fn shown_fps(&self) -> f64 {
        per_second(self.shown, self.seconds)
    }

    /// One line, for the log and `--measure`.
    pub fn line(&self) -> String {
        let show = self.show_ms.map(|ms| format!(", show {ms:.1}")).unwrap_or_default();
        format!(
            "{:.1} fps ({:.1} shown), {:.1} of {} Mbps, delay {:.1} ms (worst {:.1}): host {:.1}, network {:.1}, decode {:.1}{show}; skipped {}",
            self.fps(),
            self.shown_fps(),
            self.mbps,
            self.kbps / 1000,
            self.delay_ms,
            self.worst_ms,
            self.host_ms,
            self.net_ms,
            self.decode_ms,
            self.skipped,
        )
    }
}

/// The window's corner.
impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let to = if self.show_ms.is_some() { "shown" } else { "decoded" };
        writeln!(f, "{:.0} pictures a second, {:.0} shown", self.fps(), self.shown_fps())?;
        writeln!(f, "{:.1} of {} Mbps", self.mbps, self.kbps / 1000)?;
        writeln!(f, "{:.0} ms capture to {to}, worst {:.0}", self.delay_ms, self.worst_ms)?;
        write!(
            f,
            "host {:.0} · network {:.0} · decode {:.0}",
            self.host_ms, self.net_ms, self.decode_ms
        )?;
        if let Some(ms) = self.show_ms {
            write!(f, " · show {ms:.0}")?;
        }
        write!(f, "\n{} frames skipped", self.skipped)
    }
}

fn per_second(n: u32, seconds: f64) -> f64 {
    if seconds > 0.0 { f64::from(n) / seconds } else { 0.0 }
}

#[derive(Default)]
struct Span {
    since: Option<Instant>,
    pictures: u32,
    bytes: u64,
    kbps: u32,
    skipped: u32,
    host: Vec<u32>,
    net: Vec<u32>,
    decode: Vec<u32>,
    to_decoded: Vec<u32>,
    show: Vec<u32>,
    to_shown: Vec<u32>,
}

impl Span {
    fn picture(&mut self, sample: &Sample, at: Instant) {
        self.since.get_or_insert(at);
        self.pictures += 1;
        self.bytes += sample.bytes as u64;
        self.kbps = sample.kbps;
        self.skipped += u32::from(sample.skipped);
        self.host.push(sample.host_us);
        self.net.push(sample.net_us);
        self.decode.push(sample.decode_us);
        self.to_decoded.push(sample.to_decoded_us());
    }

    fn shown(&mut self, stamp: Stamp) {
        let show = micros(stamp.decoded.elapsed());
        self.show.push(show);
        self.to_shown.push(stamp.to_decoded_us.saturating_add(show));
    }

    fn summary(&self, until: Instant) -> Summary {
        let seconds = self.since.map_or(0.0, |since| until.duration_since(since).as_secs_f64());
        let delays = if self.to_shown.is_empty() { &self.to_decoded } else { &self.to_shown };
        Summary {
            pictures: self.pictures,
            shown: self.show.len() as u32,
            seconds,
            mbps: if seconds > 0.0 { self.bytes as f64 * 8.0 / seconds / 1e6 } else { 0.0 },
            kbps: self.kbps,
            skipped: self.skipped,
            host_ms: median_ms(&self.host),
            net_ms: median_ms(&self.net),
            decode_ms: median_ms(&self.decode),
            show_ms: (!self.show.is_empty()).then(|| median_ms(&self.show)),
            delay_ms: median_ms(delays),
            worst_ms: f64::from(delays.iter().copied().max().unwrap_or(0)) / 1e3,
        }
    }
}

fn median_ms(values: &[u32]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).map_or(0.0, |&us| f64::from(us) / 1e3)
}

pub fn micros(d: Duration) -> u32 {
    u32::try_from(d.as_micros()).unwrap_or(u32::MAX)
}

#[derive(Default)]
struct Inner {
    second: Span,
    /// Every picture, for `--measure`'s last line; `None` in the window,
    /// which runs for hours.
    run: Option<Span>,
    last: Option<Summary>,
    rolls: u64,
}

#[derive(Default)]
pub struct Stats {
    inner: Mutex<Inner>,
}

impl Stats {
    /// Statistics that also keep the whole run ([`Self::run`]).
    pub fn whole_run() -> Self {
        let stats = Self::default();
        stats.lock().run = Some(Span::default());
        stats
    }

    /// A picture decoded. When a second of pictures has passed before it,
    /// that second rolls up and is returned.
    pub fn picture(&self, sample: &Sample) -> Option<Summary> {
        self.picture_at(sample, Instant::now())
    }

    fn picture_at(&self, sample: &Sample, at: Instant) -> Option<Summary> {
        let mut inner = self.lock();
        let rolled = match inner.second.since {
            Some(since) if at.duration_since(since) >= SECOND => {
                let summary = std::mem::take(&mut inner.second).summary(at);
                inner.last = Some(summary.clone());
                inner.rolls += 1;
                Some(summary)
            }
            _ => None,
        };
        inner.second.picture(sample, at);
        if let Some(run) = &mut inner.run {
            run.picture(sample, at);
        }
        rolled
    }

    /// A picture went to the GPU.
    pub fn shown(&self, stamp: Stamp) {
        let mut inner = self.lock();
        inner.second.shown(stamp);
        if let Some(run) = &mut inner.run {
            run.shown(stamp);
        }
    }

    /// The last second rolled up, and how many have.
    pub fn last(&self) -> (u64, Option<Summary>) {
        let inner = self.lock();
        (inner.rolls, inner.last.clone())
    }

    /// Every picture so far, with [`Self::whole_run`].
    pub fn run(&self) -> Option<Summary> {
        self.lock().run.as_ref().map(|run| run.summary(Instant::now()))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(host_us: u32) -> Sample {
        Sample {
            bytes: 125_000,
            host_us,
            net_us: 1_000,
            decode_us: 3_000,
            kbps: 50_000,
            skipped: 1,
        }
    }

    #[test]
    fn a_second_of_pictures_rolls_up_when_the_next_one_comes() {
        let stats = Stats::default();
        let start = Instant::now();
        for i in 0..60u32 {
            let at = start + Duration::from_micros(u64::from(i) * 16_667);
            assert_eq!(stats.picture_at(&sample(8_000 + i), at), None);
        }
        let summary = stats.picture_at(&sample(8_000), start + SECOND).expect("a second");
        assert_eq!(summary.pictures, 60);
        assert!((summary.fps() - 60.0).abs() < 0.01, "{}", summary.fps());
        assert!((summary.mbps - 60.0).abs() < 0.01, "{}", summary.mbps);
        assert_eq!(summary.skipped, 60);
        assert!((summary.host_ms - 8.03).abs() < 0.001, "{}", summary.host_ms);
        // With no window, the delay runs to decoded: 8 + 1 + 3.
        assert_eq!(summary.show_ms, None);
        assert!((summary.delay_ms - 12.03).abs() < 0.001, "{}", summary.delay_ms);
        assert!((summary.worst_ms - 12.059).abs() < 0.001, "{}", summary.worst_ms);
        assert_eq!(stats.last(), (1, Some(summary)));
    }

    #[test]
    fn shown_pictures_take_the_delay_to_the_gpu() {
        let stats = Stats::default();
        let start = Instant::now();
        stats.picture_at(&sample(8_000), start);
        stats.shown(Stamp {
            to_decoded_us: 12_000,
            decoded: Instant::now() - Duration::from_millis(5),
        });
        let summary = stats.picture_at(&sample(8_000), start + SECOND).expect("a second");
        assert_eq!(summary.shown, 1);
        assert!(summary.show_ms.expect("shown") >= 5.0);
        assert!(summary.delay_ms >= 17.0, "{}", summary.delay_ms);
    }

    #[test]
    fn the_whole_run_is_kept_only_when_asked() {
        assert_eq!(Stats::default().run(), None);
        let stats = Stats::whole_run();
        let start = Instant::now();
        for i in 0..90u64 {
            stats.picture_at(&sample(8_000), start + Duration::from_millis(i * 16));
        }
        assert_eq!(stats.run().expect("kept").pictures, 90);
    }
}
