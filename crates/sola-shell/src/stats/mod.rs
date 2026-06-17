//! Menubar system monitors: background sampling + the snapshot model.
//! See docs/specs/2026-06-16-menubar-system-monitors-design.md.

pub mod cpu;
pub mod gpu;
pub mod mem;
pub mod net;

use iced::futures::Stream;
use iced::Color;
use iced::Subscription;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Which metric an indicator / panel represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    Cpu,
    Gpu,
    Mem,
    Net,
}

/// Threshold colors for level metrics (cpu/gpu/mem). Net is a rate, no level.
pub const AMBER: Color = Color::from_rgb(0.824, 0.600, 0.133); // #d29922
pub const RED: Color = Color::from_rgb(0.973, 0.318, 0.286); // #f85149
pub const WARN_PCT: f32 = 75.0;
pub const CRIT_PCT: f32 = 90.0;

/// Pick the readout color for a percentage level: `neutral` until WARN, then
/// amber, then red at/above CRIT.
pub fn level_color(pct: f32, neutral: Color) -> Color {
    if pct >= CRIT_PCT {
        RED
    } else if pct >= WARN_PCT {
        AMBER
    } else {
        neutral
    }
}

/// Fixed-capacity sample window for a metric's history graph.
#[derive(Clone, Debug)]
pub struct History {
    cap: usize,
    buf: VecDeque<f32>,
}

impl History {
    pub fn new(cap: usize) -> Self {
        Self { cap, buf: VecDeque::with_capacity(cap) }
    }
    pub fn push(&mut self, v: f32) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }
    /// Samples oldest→newest as a contiguous slice (compacts the deque).
    pub fn samples(&mut self) -> &[f32] {
        self.buf.make_contiguous();
        self.buf.as_slices().0
    }
    pub fn peak(&self) -> f32 {
        self.buf.iter().copied().fold(0.0, f32::max)
    }
}

/// One process holding the currently-open panel metric so the sampler thread
/// knows whether to do tier-2 (expensive) detail and for which metric.
static ACTIVE_METRIC: Mutex<Option<Metric>> = Mutex::new(None);

/// Set/clear the metric whose detail the sampler should gather. Called by the
/// shell when a stat panel opens (`Some`) or closes (`None`).
pub fn set_active_metric(m: Option<Metric>) {
    if let Ok(mut g) = ACTIVE_METRIC.lock() {
        *g = m;
    }
}

fn active_metric() -> Option<Metric> {
    ACTIVE_METRIC.lock().ok().and_then(|g| *g)
}

/// A complete sample delivered to the UI each tick.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub cpu_pct: f32,
    pub mem_pct: f32,
    pub net_down: f32, // bytes/sec
    pub net_up: f32,   // bytes/sec
    pub gpu: Option<GpuLite>,
    /// Tier-2 detail for the open metric, if any.
    pub detail: Option<Detail>,
}

/// Tier-1 GPU summary for the bar (None when no NVIDIA GPU).
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuLite {
    pub util: f32,
    pub temp_c: f32,
}

/// Tier-2 detail; exactly one variant is filled, matching the open panel.
#[derive(Clone, Debug)]
pub enum Detail {
    Cpu(cpu::CpuDetail),
    Mem(mem::MemDetail),
    Net(net::NetDetail),
    Gpu(gpu::GpuDetail),
}

const TICK: Duration = Duration::from_millis(1000);

/// Background sampler: reads tier-1 aggregates every tick and tier-2 detail for
/// the active metric, delivering a `Snapshot` to iced. Mirrors the kit's
/// `bus_stream` poller (one thread, mpsc → stream).
pub fn subscription() -> Subscription<Arc<Snapshot>> {
    Subscription::run(stats_stream)
}

fn stats_stream() -> impl Stream<Item = Arc<Snapshot>> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded::<Arc<Snapshot>>();

    std::thread::spawn(move || {
        // Per-thread previous samples for delta-based rates.
        let mut prev_cpu = cpu::parse_aggregate(&read("/proc/stat").unwrap_or_default());
        let mut prev_net = net::read_counters();
        loop {
            if tx.is_closed() {
                break;
            }
            std::thread::sleep(TICK);

            let stat = read("/proc/stat").unwrap_or_default();
            let cur_cpu = cpu::parse_aggregate(&stat);
            let cpu_pct = match (prev_cpu, cur_cpu) {
                (Some(p), Some(c)) => cpu::cpu_pct(&p, &c),
                _ => 0.0,
            };
            prev_cpu = cur_cpu;

            let mem_pct = mem::pressure_pct();

            let cur_net = net::read_counters();
            let (down, up) = net::rate(&prev_net, &cur_net, TICK.as_secs_f32());

            let gpu = gpu::lite();

            let detail = match active_metric() {
                Some(Metric::Cpu) => Some(Detail::Cpu(cpu::detail(&stat, &[]))),
                Some(Metric::Mem) => Some(Detail::Mem(mem::detail())),
                Some(Metric::Net) => Some(Detail::Net(net::detail(&cur_net))),
                Some(Metric::Gpu) => gpu::detail().map(Detail::Gpu),
                None => None,
            };

            prev_net = cur_net;

            let snap = Arc::new(Snapshot {
                cpu_pct,
                mem_pct,
                net_down: down,
                net_up: up,
                gpu,
                detail,
            });
            if tx.unbounded_send(snap).is_err() {
                break;
            }
        }
    });

    rx
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Color;

    #[test]
    fn threshold_color_bands() {
        // neutral below warn, amber in [warn,crit), red at/above crit
        let neutral = Color::from_rgb(0.902, 0.929, 0.953); // #e6edf3
        assert_eq!(level_color(10.0, neutral), neutral);
        assert_eq!(level_color(80.0, neutral), AMBER);
        assert_eq!(level_color(95.0, neutral), RED);
        // boundaries
        assert_eq!(level_color(WARN_PCT, neutral), AMBER);
        assert_eq!(level_color(CRIT_PCT, neutral), RED);
    }

    #[test]
    fn history_keeps_last_n() {
        let mut h = History::new(3);
        for v in [1.0, 2.0, 3.0, 4.0] {
            h.push(v);
        }
        assert_eq!(h.samples(), &[2.0, 3.0, 4.0]);
    }

    #[test]
    fn history_peak() {
        let mut h = History::new(4);
        for v in [5.0, 9.0, 3.0] {
            h.push(v);
        }
        assert_eq!(h.peak(), 9.0);
    }
}
