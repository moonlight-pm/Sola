# Menubar System Monitors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add live CPU · GPU · MEM · NET indicators to the `sola-shell` menubar (left of the clock), each a click target opening a rich detail dropdown.

**Architecture:** A new `sola-shell/src/stats/` module samples the system on a background thread (mirroring the kit's `bus_stream` poller) and pushes `Snapshot`s into iced via `Msg::StatsTick`. Two tiers: cheap aggregates always (for the bar), expensive per-metric detail only while that metric's panel is open (gated by a process-global `ACTIVE_METRIC`). Dropdowns reuse the calendar's Menu-window panel mechanism, generalized from `current_open_is_calendar: bool` to `open_panel: Option<Panel>`.

**Tech Stack:** Rust, iced 0.14 (wgpu/canvas), `/proc` parsing (std only), `nvml-wrapper` for NVIDIA GPU, `nix` for `getifaddrs`.

**Reference (read before starting):** the calendar feature is the template for everything panel-related — `crates/sola-shell/src/calendar.rs`, `crates/sola-shell/src/menu/view.rs::calendar_panel`, and the `ToggleCalendar`/`current_open_is_calendar` handling in `crates/sola-shell/src/app.rs`. The background-thread subscription template is `crates/sola-kit/src/app.rs::bus_stream` (lines ~293–340).

**Spec:** `docs/specs/2026-06-16-menubar-system-monitors-design.md`

**Conventions:** Run `cargo make build sola-shell` to compile (never raw `cargo build`). Run tests with `cargo test -p sola-shell`. Commit after each task. Do NOT install. Work directly on `master` (single session).

---

## File Structure

Create under `crates/sola-shell/src/`:

- `stats/mod.rs` — `Metric`, `Snapshot` + per-metric stat structs, `History` ring buffers, `ACTIVE_METRIC` global, threshold→color helper, the sampler thread + `subscription()`.
- `stats/cpu.rs` — `/proc/stat`, `/proc/loadavg`, `/proc/uptime`, top-by-CPU parsing.
- `stats/mem.rs` — `/proc/meminfo` + top-by-RSS parsing.
- `stats/net.rs` — `/proc/net/dev` + default-route iface + `getifaddrs` IP.
- `stats/gpu.rs` — NVML reads, graceful absence.
- `stats/view.rs` — `history_graph` canvas widget, `stat_card` shell helper, and the four `*_panel` functions.

Modify:

- `crates/sola-shell/src/main.rs` — add `pub mod stats;`.
- `crates/sola-shell/src/app.rs` — `open_panel`, stats fields, `Msg` variants + handlers, subscription wiring.
- `crates/sola-shell/src/menubar/view.rs` — the four indicators in the right cluster.
- `crates/sola-shell/src/menu/view.rs` — dispatch `Panel::Stat(m)` to `stats::view`.
- `crates/sola-shell/Cargo.toml` — add `nvml-wrapper`, `nix`.

---

## Phase 1 — Stats core + live CPU % in the bar

### Task 1: Metric enum, threshold color, module skeleton

**Files:**
- Create: `crates/sola-shell/src/stats/mod.rs`
- Modify: `crates/sola-shell/src/main.rs` (add `pub mod stats;` after `pub mod menubar;`)

- [ ] **Step 1: Write the failing test** — append to `crates/sola-shell/src/stats/mod.rs`:

```rust
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
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sola-shell stats::tests::threshold_color_bands`
Expected: FAIL — `level_color`, `AMBER`, etc. not found.

- [ ] **Step 3: Write the module** — put this at the top of `crates/sola-shell/src/stats/mod.rs`:

```rust
//! Menubar system monitors: background sampling + the snapshot model.
//! See docs/specs/2026-06-16-menubar-system-monitors-design.md.

use iced::Color;

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
```

- [ ] **Step 4: Add the module** — in `crates/sola-shell/src/main.rs`, after the line `pub mod menubar;` add:

```rust
pub mod stats;
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p sola-shell stats::tests::threshold_color_bands`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-shell/src/stats/mod.rs crates/sola-shell/src/main.rs
git commit -m "feat(sola-shell): stats module skeleton + threshold colors"
```

---

### Task 2: CPU aggregate parser (`/proc/stat`)

**Files:**
- Create: `crates/sola-shell/src/stats/cpu.rs`
- Modify: `crates/sola-shell/src/stats/mod.rs` (add `pub mod cpu;`)

- [ ] **Step 1: Write the failing test** — append to `crates/sola-shell/src/stats/cpu.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  100 0 50 1000 20 0 10 0 0 0\ncpu0 50 0 25 500 10 0 5 0 0 0\ncpu1 50 0 25 500 10 0 5 0 0 0\n";

    #[test]
    fn parses_aggregate_idle_and_total() {
        let t = parse_cpu_line("cpu  100 0 50 1000 20 0 10 0 0 0").unwrap();
        // idle = idle(1000) + iowait(20) = 1020
        assert_eq!(t.idle, 1020);
        // total = sum of all = 100+0+50+1000+20+0+10 = 1180
        assert_eq!(t.total, 1180);
    }

    #[test]
    fn pct_from_delta() {
        let prev = CpuTimes { idle: 1000, total: 1100 };
        let cur = CpuTimes { idle: 1050, total: 1200 };
        // busy delta = total_d(100) - idle_d(50) = 50; pct = 50/100 = 50%
        assert!((cpu_pct(&prev, &cur) - 50.0).abs() < 0.01);
    }

    #[test]
    fn pct_zero_when_no_delta() {
        let t = CpuTimes { idle: 10, total: 20 };
        assert_eq!(cpu_pct(&t, &t), 0.0);
    }

    #[test]
    fn per_core_lines_parsed_in_order() {
        let cores = parse_per_core(STAT);
        assert_eq!(cores.len(), 2);
        assert_eq!(cores[0].total, 590); // 50+25+500+10+5
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sola-shell stats::cpu`
Expected: FAIL — `parse_cpu_line` etc. undefined.

- [ ] **Step 3: Implement** — put at the top of `crates/sola-shell/src/stats/cpu.rs`:

```rust
//! CPU sampling from /proc/stat, /proc/loadavg, /proc/uptime, /proc/<pid>/stat.

/// Cumulative jiffies for one cpu line: idle (idle+iowait) and grand total.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

/// Parse one `cpu...` line of /proc/stat into idle/total jiffies.
/// Fields after the label: user nice system idle iowait irq softirq steal ...
pub fn parse_cpu_line(line: &str) -> Option<CpuTimes> {
    let mut it = line.split_whitespace();
    let label = it.next()?;
    if !label.starts_with("cpu") {
        return None;
    }
    let vals: Vec<u64> = it.filter_map(|v| v.parse().ok()).collect();
    if vals.len() < 4 {
        return None;
    }
    let idle = vals[3] + vals.get(4).copied().unwrap_or(0); // idle + iowait
    let total: u64 = vals.iter().sum();
    Some(CpuTimes { idle, total })
}

/// Busy percentage between two cumulative samples.
pub fn cpu_pct(prev: &CpuTimes, cur: &CpuTimes) -> f32 {
    let total_d = cur.total.saturating_sub(prev.total);
    let idle_d = cur.idle.saturating_sub(prev.idle);
    if total_d == 0 {
        return 0.0;
    }
    let busy = total_d.saturating_sub(idle_d) as f32;
    (busy / total_d as f32) * 100.0
}

/// Per-core cumulative times (the `cpu0`, `cpu1`, ... lines) in order.
pub fn parse_per_core(stat: &str) -> Vec<CpuTimes> {
    stat.lines()
        .filter(|l| {
            l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(|b| b.is_ascii_digit())
        })
        .filter_map(parse_cpu_line)
        .collect()
}

/// The aggregate (`cpu `) line, if present.
pub fn parse_aggregate(stat: &str) -> Option<CpuTimes> {
    stat.lines().find(|l| l.starts_with("cpu ")).and_then(parse_cpu_line)
}
```

- [ ] **Step 4: Register the submodule** — in `crates/sola-shell/src/stats/mod.rs`, under the doc comment / `use` lines, add:

```rust
pub mod cpu;
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p sola-shell stats::cpu`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/sola-shell/src/stats/cpu.rs crates/sola-shell/src/stats/mod.rs
git commit -m "feat(sola-shell): /proc/stat CPU parser"
```

---

### Task 3: History ring buffer

**Files:**
- Modify: `crates/sola-shell/src/stats/mod.rs`

- [ ] **Step 1: Write the failing test** — add inside the existing `mod tests` in `stats/mod.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sola-shell stats::tests::history`
Expected: FAIL — `History` undefined.

- [ ] **Step 3: Implement** — add to `stats/mod.rs` (after `level_color`):

```rust
use std::collections::VecDeque;

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
```

> Note: `samples()` takes `&mut self` (it compacts). Callers in the view hold `&mut` history; that's fine — see Task 11.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sola-shell stats::tests::history`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-shell/src/stats/mod.rs
git commit -m "feat(sola-shell): stats History ring buffer"
```

---

### Task 4: Snapshot model + sampler thread + subscription

**Files:**
- Modify: `crates/sola-shell/src/stats/mod.rs`

This task adds the `Snapshot` model, the `ACTIVE_METRIC` global, and the background sampler `subscription()`. CPU is the only metric wired now; mem/net/gpu default to zero/empty until their phases.

- [ ] **Step 1: Add the snapshot + active-metric types** — add to `stats/mod.rs`:

```rust
use std::sync::Mutex;

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
```

> `mem::MemDetail`, `net::NetDetail`, `gpu::GpuDetail`, and `gpu` module are added in later phases. To keep this task compiling, also add **stub modules** now (Step 2).

- [ ] **Step 2: Add stub submodules** so `mod.rs` compiles before later phases. Create three files with a single placeholder type each:

`crates/sola-shell/src/stats/mem.rs`:
```rust
//! Memory sampling (filled in Phase 4).
#[derive(Clone, Debug, Default)]
pub struct MemDetail;
```

`crates/sola-shell/src/stats/net.rs`:
```rust
//! Network sampling (filled in Phase 5).
#[derive(Clone, Debug, Default)]
pub struct NetDetail;
```

`crates/sola-shell/src/stats/gpu.rs`:
```rust
//! GPU sampling via NVML (filled in Phase 6).
#[derive(Clone, Debug, Default)]
pub struct GpuDetail;
```

And in `stats/mod.rs` register them next to `pub mod cpu;`:
```rust
pub mod gpu;
pub mod mem;
pub mod net;
```

Also add the CPU detail type now in `crates/sola-shell/src/stats/cpu.rs` (used by Phase 3, declared here so `Detail::Cpu` resolves):
```rust
/// A process row for a "top processes" list.
#[derive(Clone, Debug)]
pub struct Proc {
    pub name: String,
    pub value: f32, // percent (cpu) or MB (mem) depending on the list
}

/// Tier-2 CPU detail.
#[derive(Clone, Debug, Default)]
pub struct CpuDetail {
    pub per_core: Vec<f32>,
    pub load: [f32; 3],
    pub uptime_secs: u64,
    pub top: Vec<Proc>,
}
```

- [ ] **Step 3: Add the sampler thread + subscription** — add to `stats/mod.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;
use iced::futures::Stream;
use iced::Subscription;

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
            prev_net = cur_net;

            let gpu = gpu::lite();

            let detail = match active_metric() {
                Some(Metric::Cpu) => Some(Detail::Cpu(cpu::detail(&stat, &prev_cpu_for_cores()))),
                Some(Metric::Mem) => Some(Detail::Mem(mem::detail())),
                Some(Metric::Net) => Some(Detail::Net(net::detail(&cur_net))),
                Some(Metric::Gpu) => gpu::detail().map(Detail::Gpu),
                None => None,
            };

            let snap = Arc::new(Snapshot { cpu_pct, mem_pct, net_down: down, net_up: up, gpu, detail });
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
```

> **Important:** the `detail` arm above references functions defined in later phases (`cpu::detail`, `mem::detail`, `net::detail`/`read_counters`/`rate`, `gpu::lite`/`detail`). To keep Phase-1 compiling, in THIS task add minimal stubs returning defaults, and replace them in their phases:
>
> In `cpu.rs`: `pub fn detail(_stat: &str, _prev: &[CpuTimes]) -> CpuDetail { CpuDetail::default() }` and remove the `prev_cpu_for_cores()` call — for Phase 1 simplify the CPU detail arm to `Some(Detail::Cpu(cpu::detail(&stat, &[])))`.
>
> In `mem.rs`: `pub fn pressure_pct() -> f32 { 0.0 }` and `pub fn detail() -> MemDetail { MemDetail }`.
>
> In `net.rs`: `#[derive(Clone, Debug, Default)] pub struct Counters; pub fn read_counters() -> Counters { Counters } pub fn rate(_p: &Counters, _c: &Counters, _dt: f32) -> (f32, f32) { (0.0, 0.0) } pub fn detail(_c: &Counters) -> NetDetail { NetDetail }`.
>
> In `gpu.rs`: `pub fn lite() -> Option<GpuLite> { None }` — wait, `GpuLite` lives in `mod.rs`; use `pub fn lite() -> Option<crate::stats::GpuLite> { None }` and `pub fn detail() -> Option<GpuDetail> { None }`.
>
> Simplify the Phase-1 `detail` match to only the `Cpu` arm returning `cpu::detail(&stat, &[])`; leave the others producing `None`/defaults via the stubs. The phases replace each stub with the real implementation and re-point the match arm.

- [ ] **Step 4: Build (no test — threading)**

Run: `cargo make build sola-shell`
Expected: compiles clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-shell/src/stats/
git commit -m "feat(sola-shell): stats Snapshot model + sampler thread"
```

---

### Task 5: Wire StatsTick into the shell + CPU bar indicator

**Files:**
- Modify: `crates/sola-shell/src/app.rs` (Shell struct, `Msg`, `new`, `update`, `subscription`)
- Modify: `crates/sola-shell/src/menubar/view.rs`

- [ ] **Step 1: Add shell state** — in `app.rs`, in the `Shell` struct (near `pub menubar: MenubarState,`), add:

```rust
    /// Latest system-stats sample for the menubar indicators + panels.
    pub stats: std::sync::Arc<crate::stats::Snapshot>,
    /// Per-metric history for the dropdown graphs (cpu, mem, net-down, net-up).
    pub cpu_hist: crate::stats::History,
    pub mem_hist: crate::stats::History,
    pub net_down_hist: crate::stats::History,
    pub net_up_hist: crate::stats::History,
    pub gpu_hist: crate::stats::History,
```

- [ ] **Step 2: Initialize them** — in `Shell::new`/boot constructor literal (where `menubar: MenubarState::...` is set), add:

```rust
            stats: std::sync::Arc::new(crate::stats::Snapshot::default()),
            cpu_hist: crate::stats::History::new(60),
            mem_hist: crate::stats::History::new(60),
            net_down_hist: crate::stats::History::new(60),
            net_up_hist: crate::stats::History::new(60),
            gpu_hist: crate::stats::History::new(60),
```

- [ ] **Step 3: Add the Msg variant** — in `app.rs`, in `enum Msg`, after `ClockTick,` and the calendar variants add:

```rust
    /// New system-stats sample from the background sampler.
    StatsTick(std::sync::Arc<crate::stats::Snapshot>),
```

- [ ] **Step 4: Handle it** — in `update`, after the `Msg::ClockTick => { ... }` arm add:

```rust
            Msg::StatsTick(snap) => {
                self.cpu_hist.push(snap.cpu_pct);
                self.mem_hist.push(snap.mem_pct);
                self.net_down_hist.push(snap.net_down);
                self.net_up_hist.push(snap.net_up);
                if let Some(g) = snap.gpu {
                    self.gpu_hist.push(g.util);
                }
                self.stats = snap;
                iced::Task::none()
            }
```

- [ ] **Step 5: Add the subscription** — in `Shell::subscription`, extend the `subs` vec:

```rust
            crate::stats::subscription().map(Msg::StatsTick),
```

- [ ] **Step 6: Add the CPU indicator** — in `menubar/view.rs`, replace the right-cluster assembly (the `row![ row(left), Space, toast, clock ]`) so an indicators group sits before the clock. Add a helper near the bottom of the file:

```rust
/// One numbers-only menubar indicator: muted label + mono value.
fn stat_indicator<'a>(label: &'a str, value: String, color: iced::Color) -> Element<'a, Msg> {
    use iced::widget::{row, text};
    row![
        text(label).font(sola_kit::fonts::INTER).size(10)
            .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(iced::Color { a: 0.6, ..color }) }),
        text(value).font(sola_kit::fonts::MONO).size(13)
            .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(color) }),
    ]
    .spacing(5)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
```

Then build the indicators cluster (CPU only for now) and put it left of the clock. In the assembly, replace:

```rust
    row![
        row(left),
        iced::widget::Space::new().width(iced::Length::Fill),
        toast,
        container(clock).padding([2, 8]),
    ]
```

with:

```rust
    let neutral = iced::Color::from_rgb(0.902, 0.929, 0.953); // #e6edf3
    let cpu_pct = shell.stats.cpu_pct;
    let cpu_btn: Element<'_, Msg> = iced::widget::button(
        stat_indicator("CPU", format!("{:.0}%", cpu_pct), crate::stats::level_color(cpu_pct, neutral)),
    )
    .style(kit_btn::menubar(false))
    .padding([2, 8])
    .on_press(Msg::Noop) // becomes ToggleStatPanel in Phase 2
    .into();

    row![
        row(left),
        iced::widget::Space::new().width(iced::Length::Fill),
        toast,
        cpu_btn,
        clock,
    ]
```

> Note: `clock` is already a button element from the calendar task; keep it last. `kit_btn` is already imported in this file.

- [ ] **Step 7: Build + manual verify**

Run: `cargo make build sola-shell`
Expected: compiles. (Runtime check is the user's: after they install, the menubar shows a live `CPU NN%` left of the clock.)

- [ ] **Step 8: Commit**

```bash
git add crates/sola-shell/src/app.rs crates/sola-shell/src/menubar/view.rs
git commit -m "feat(sola-shell): live CPU% menubar indicator"
```

---

## Phase 2 — Panel mechanism + clickable CPU dropdown (minimal)

### Task 6: Generalize the calendar flag to `open_panel`

**Files:**
- Modify: `crates/sola-shell/src/app.rs`, `crates/sola-shell/src/menu/view.rs`, `crates/sola-shell/src/menubar/view.rs`

Replace the boolean `current_open_is_calendar` with an enum so the Menu window can host either the calendar or a stat panel.

- [ ] **Step 1: Define the enum** — in `app.rs`, near the `Shell` struct, add:

```rust
/// Which non-menu panel the Menu window is hosting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Calendar,
    Stat(crate::stats::Metric),
}
```

- [ ] **Step 2: Replace the field** — in the `Shell` struct, replace:

```rust
    pub current_open_is_calendar: bool,
```

with:

```rust
    pub open_panel: Option<Panel>,
```

In the constructor, replace `current_open_is_calendar: false,` with `open_panel: None,`.

- [ ] **Step 3: Update calendar handlers** — in `update`, in `Msg::ToggleCalendar`, replace the `self.current_open_is_calendar` reads/writes:
  - Open: `self.open_panel = Some(Panel::Calendar);`
  - The "already showing the calendar" test: `if self.menu_open && self.open_panel == Some(Panel::Calendar)`.
  - Dismiss: `self.open_panel = None;`
  In `Msg::OpenMenu` and `Msg::HoverMenu` replace `self.current_open_is_calendar = false;` with `self.open_panel = None;`.
  In `Msg::CloseMenu` replace `self.current_open_is_calendar = false;` with `self.open_panel = None;` and add `crate::stats::set_active_metric(None);`.

- [ ] **Step 4: Update the menu view dispatch** — in `menu/view.rs::view`, replace:

```rust
    if shell.current_open_is_calendar {
        return calendar_panel(shell);
    }
```

with:

```rust
    match shell.open_panel {
        Some(crate::app::Panel::Calendar) => return calendar_panel(shell),
        Some(crate::app::Panel::Stat(m)) => return crate::stats::view::panel(shell, m),
        None => {}
    }
```

> `crate::stats::view::panel` is created in Task 7 — this won't compile until then; that's fine within the task pair, but to keep Task 6 self-contained, temporarily make the `Stat` arm `Some(crate::app::Panel::Stat(_)) => {}` and switch it to the real call in Task 7.

- [ ] **Step 5: Update the menubar clock active check** — in `menubar/view.rs`, replace `shell.current_open_is_calendar` with `shell.open_panel == Some(crate::app::Panel::Calendar)`.

- [ ] **Step 6: Build + verify calendar still works**

Run: `cargo make build sola-shell && cargo test -p sola-shell`
Expected: compiles, all tests pass. (User confirms the calendar still opens after install.)

- [ ] **Step 7: Commit**

```bash
git add crates/sola-shell/src/app.rs crates/sola-shell/src/menu/view.rs crates/sola-shell/src/menubar/view.rs
git commit -m "refactor(sola-shell): open_panel enum (calendar + stat panels)"
```

---

### Task 7: ToggleStatPanel + minimal CPU panel

**Files:**
- Create: `crates/sola-shell/src/stats/view.rs`
- Modify: `crates/sola-shell/src/stats/mod.rs` (`pub mod view;`), `crates/sola-shell/src/app.rs`, `crates/sola-shell/src/menubar/view.rs`, `crates/sola-shell/src/menu/view.rs`

- [ ] **Step 1: Add the Msg variant + handler** — in `app.rs` `enum Msg` add:

```rust
    /// Toggle a stat detail panel (clicking a menubar indicator).
    ToggleStatPanel(crate::stats::Metric),
```

In `update`, after `Msg::ToggleCalendar`, add (mirrors ToggleCalendar):

```rust
            Msg::ToggleStatPanel(m) => {
                if self.menu_open && self.open_panel == Some(crate::app::Panel::Stat(m)) {
                    self.menu_open = false;
                    self.open_panel = None;
                    crate::stats::set_active_metric(None);
                } else {
                    self.menu_open = true;
                    self.open_panel = Some(crate::app::Panel::Stat(m));
                    self.current_open_index = None;
                    self.current_open_is_system = false;
                    crate::stats::set_active_metric(Some(m));
                }
                self.emit_composition();
                self.emit_registered_chords();
                iced::Task::none()
            }
```

- [ ] **Step 2: Create the panel module** — `crates/sola-shell/src/stats/view.rs`:

```rust
//! Stat detail dropdown panels, rendered in the Menu window.

use iced::widget::{column, container, text};
use iced::{Element, Length, Padding};

use crate::app::{Msg, Shell};
use crate::stats::Metric;
use sola_kit::components::popover;

pub const CARD_WIDTH: f32 = 320.0;

/// Build the right-anchored panel for `metric`, over a dismiss backdrop.
/// Mirrors `crate::menu::view::calendar_panel`.
pub fn panel(shell: &Shell, metric: Metric) -> Element<'_, Msg> {
    use iced::widget::{mouse_area, stack};

    let card = match metric {
        Metric::Cpu => cpu_card(shell),
        Metric::Gpu => placeholder("GPU"),
        Metric::Mem => placeholder("Memory"),
        Metric::Net => placeholder("Network"),
    };

    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    let left = (output_w - CARD_WIDTH - 8.0).max(0.0);

    let positioned: Element<'_, Msg> = container(card)
        .padding(Padding { top: 0.0, left, right: 0.0, bottom: 0.0 })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Top)
        .into();

    let backdrop: Element<'_, Msg> = mouse_area(
        container(text("")).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Msg::CloseMenu)
    .into();

    stack![backdrop, positioned].width(Length::Fill).height(Length::Fill).into()
}

fn placeholder(label: &str) -> Element<'static, Msg> {
    popover(
        column![text(label.to_string()).size(14)].padding(4),
    )
    .padding(Padding::new(8.0))
    .width(Length::Fixed(CARD_WIDTH))
    .into()
}

/// Minimal CPU card (header only) — fleshed out in Phase 3.
fn cpu_card(shell: &Shell) -> Element<'_, Msg> {
    let pct = shell.stats.cpu_pct;
    popover(
        column![
            text("CPU").size(11).style(sola_kit::components::text::muted),
            text(format!("{:.0}%", pct)).font(sola_kit::fonts::MONO).size(28),
        ]
        .spacing(4)
        .padding(4),
    )
    .padding(Padding::new(8.0))
    .width(Length::Fixed(CARD_WIDTH))
    .into()
}
```

- [ ] **Step 3: Register + wire dispatch** — in `stats/mod.rs` add `pub mod view;`. In `menu/view.rs`, set the `Stat` arm (from Task 6 Step 4) to:

```rust
        Some(crate::app::Panel::Stat(m)) => return crate::stats::view::panel(shell, m),
```

- [ ] **Step 4: Make the CPU indicator open the panel** — in `menubar/view.rs`, change the CPU button's `.on_press(Msg::Noop)` to:

```rust
    .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Cpu))
```

Also set its active style: `kit_btn::menubar(shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Cpu)))`.

- [ ] **Step 5: Build + manual verify**

Run: `cargo make build sola-shell && cargo test -p sola-shell`
Expected: compiles, tests pass. (User: clicking CPU opens a small card showing the live %.)

- [ ] **Step 6: Commit**

```bash
git add crates/sola-shell/src/stats/ crates/sola-shell/src/app.rs crates/sola-shell/src/menubar/view.rs crates/sola-shell/src/menu/view.rs
git commit -m "feat(sola-shell): clickable CPU stat panel (minimal)"
```

---

## Phase 3 — Full CPU dropdown (graph, per-core, processes, footer)

### Task 8: History-graph canvas widget

**Files:**
- Modify: `crates/sola-shell/src/stats/view.rs`

- [ ] **Step 1: Implement the widget** — add to `stats/view.rs`:

```rust
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Point, Rectangle, Renderer, Theme};

/// A 60-sample area+line history chart. `max` is the value mapped to the top
/// (e.g. 100.0 for percentages, or the buffer peak for rates).
pub struct Graph {
    pub samples: Vec<f32>,
    pub max: f32,
    pub color: Color,
}

impl<Message> canvas::Program<Message> for Graph {
    type State = ();
    fn draw(&self, _s: &(), renderer: &Renderer, _t: &Theme, bounds: Rectangle, _c: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let n = self.samples.len();
        if n < 2 || self.max <= 0.0 {
            return vec![frame.into_geometry()];
        }
        let w = bounds.width;
        let h = bounds.height;
        let x = |i: usize| (i as f32 / (n - 1) as f32) * w;
        let y = |v: f32| h - (v / self.max).clamp(0.0, 1.0) * h;

        let line = Path::new(|p| {
            p.move_to(Point::new(x(0), y(self.samples[0])));
            for i in 1..n {
                p.line_to(Point::new(x(i), y(self.samples[i])));
            }
        });
        let area = Path::new(|p| {
            p.move_to(Point::new(x(0), h));
            for i in 0..n {
                p.line_to(Point::new(x(i), y(self.samples[i])));
            }
            p.line_to(Point::new(x(n - 1), h));
            p.close();
        });
        frame.fill(&area, Color { a: 0.25, ..self.color });
        frame.stroke(&line, Stroke::default().with_color(self.color).with_width(1.5));
        vec![frame.into_geometry()]
    }
}

/// Convenience: a fixed-height graph element from samples.
pub fn history_graph<'a, Message: 'a>(samples: Vec<f32>, max: f32, color: Color) -> Element<'a, Message> {
    canvas(Graph { samples, max, color }).width(Length::Fill).height(Length::Fixed(58.0)).into()
}
```

Add `use iced::widget::canvas;` to the imports.

- [ ] **Step 2: Build**

Run: `cargo make build sola-shell`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-shell/src/stats/view.rs
git commit -m "feat(sola-shell): history-graph canvas widget"
```

---

### Task 9: CPU tier-2 detail sampling

**Files:**
- Modify: `crates/sola-shell/src/stats/cpu.rs`, `crates/sola-shell/src/stats/mod.rs`

- [ ] **Step 1: Write the failing test** — add to `cpu.rs` tests:

```rust
    #[test]
    fn loadavg_parsed() {
        assert_eq!(parse_loadavg("4.20 3.80 3.10 2/1234 5678"), [4.20, 3.80, 3.10]);
    }

    #[test]
    fn top_sorted_desc_and_capped() {
        let mut rows = vec![
            Proc { name: "a".into(), value: 5.0 },
            Proc { name: "b".into(), value: 22.0 },
            Proc { name: "c".into(), value: 7.0 },
        ];
        cap_top(&mut rows, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "b");
        assert_eq!(rows[1].name, "c");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sola-shell stats::cpu`
Expected: FAIL.

- [ ] **Step 3: Implement** — replace the Phase-1 stub `detail()` in `cpu.rs` with the real parsers:

```rust
pub fn parse_loadavg(s: &str) -> [f32; 3] {
    let mut it = s.split_whitespace().filter_map(|v| v.parse::<f32>().ok());
    [it.next().unwrap_or(0.0), it.next().unwrap_or(0.0), it.next().unwrap_or(0.0)]
}

/// Sort processes by value descending and keep the top `n`.
pub fn cap_top(rows: &mut Vec<Proc>, n: usize) {
    rows.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    rows.truncate(n);
}

/// Build tier-2 CPU detail. `per_core_pct` is computed by the sampler from
/// successive /proc/stat snapshots (see mod.rs); load/uptime/top read here.
pub fn detail(per_core_pct: Vec<f32>, top: Vec<Proc>) -> CpuDetail {
    let load = parse_loadavg(&std::fs::read_to_string("/proc/loadavg").unwrap_or_default());
    let uptime_secs = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f32>().ok()))
        .map(|f| f as u64)
        .unwrap_or(0);
    CpuDetail { per_core: per_core_pct, load, uptime_secs, top }
}

/// Top processes by CPU between two scans of /proc/<pid>/stat (utime+stime).
/// `total_delta` is the aggregate cpu total-jiffies delta over the interval.
pub fn top_processes(prev: &std::collections::HashMap<i32, u64>, total_delta: u64, ncpu: usize) -> (std::collections::HashMap<i32, u64>, Vec<Proc>) {
    use std::collections::HashMap;
    let mut cur: HashMap<i32, u64> = HashMap::new();
    let mut rows: Vec<Proc> = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return (cur, rows) };
    for ent in dir.flatten() {
        let Ok(pid) = ent.file_name().to_string_lossy().parse::<i32>() else { continue };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
        // comm is in parens (field 2); split after the closing paren to avoid spaces in names.
        let Some(rparen) = stat.rfind(')') else { continue };
        let rest: Vec<&str> = stat[rparen + 2..].split_whitespace().collect();
        // After comm, field indices: state=0, ... utime=11, stime=12 (0-based in `rest`).
        let (Some(utime), Some(stime)) = (rest.get(11).and_then(|v| v.parse::<u64>().ok()), rest.get(12).and_then(|v| v.parse::<u64>().ok())) else { continue };
        let jiffies = utime + stime;
        cur.insert(pid, jiffies);
        if total_delta > 0 {
            if let Some(p) = prev.get(&pid) {
                let d = jiffies.saturating_sub(*p) as f32;
                // % of one core summed across the machine: scale by ncpu.
                let pct = (d / total_delta as f32) * 100.0 * ncpu as f32;
                if pct >= 0.5 {
                    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim().to_string();
                    rows.push(Proc { name, value: pct });
                }
            }
        }
    }
    cap_top(&mut rows, 4);
    (cur, rows)
}
```

- [ ] **Step 4: Wire detail in the sampler** — in `stats/mod.rs::stats_stream`, maintain per-core prev + a per-PID prev map, and build CPU detail only when active. Replace the Phase-1 CPU detail arm:

```rust
    // before the loop:
    let mut prev_cores: Vec<cpu::CpuTimes> = cpu::parse_per_core(&read("/proc/stat").unwrap_or_default());
    let mut prev_pids: std::collections::HashMap<i32, u64> = std::collections::HashMap::new();
    let ncpu = prev_cores.len().max(1);
```

```rust
    // inside the loop, after computing cpu_pct and reading `stat`:
    let detail = match active_metric() {
        Some(Metric::Cpu) => {
            let cur_cores = cpu::parse_per_core(&stat);
            let per_core: Vec<f32> = prev_cores.iter().zip(&cur_cores)
                .map(|(p, c)| cpu::cpu_pct(p, c)).collect();
            prev_cores = cur_cores;
            let total_delta = match (prev_cpu, cur_cpu) { (Some(p), Some(c)) => c.total.saturating_sub(p.total), _ => 0 };
            let (cur_pids, top) = cpu::top_processes(&prev_pids, total_delta, ncpu);
            prev_pids = cur_pids;
            Some(Detail::Cpu(cpu::detail(per_core, top)))
        }
        _ => None, // mem/net/gpu arms added in their phases
    };
```

> Note: `prev_cpu` is reassigned earlier in the loop; capture `total_delta` BEFORE reassigning `prev_cpu`, or compute it from a saved copy. Reorder so `total_delta` uses the pre-update `prev_cpu`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p sola-shell stats::cpu`
Expected: PASS. Then `cargo make build sola-shell` compiles.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-shell/src/stats/
git commit -m "feat(sola-shell): CPU tier-2 detail (per-core, loadavg, top procs)"
```

---

### Task 10: `stat_card` shell + full CPU panel

**Files:**
- Modify: `crates/sola-shell/src/stats/view.rs`

- [ ] **Step 1: Add the card shell helper** — add to `stats/view.rs`:

```rust
use iced::widget::row;

/// Compose a stat card: header (label/value/identity) + body sections.
/// `body` items are stacked with dividers handled by the caller.
fn stat_card<'a>(
    label: &'a str,
    value: String,
    value_color: Color,
    identity: Vec<Element<'a, Msg>>,
    body: Vec<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let header = row![
        column![
            text(label).size(11).style(sola_kit::components::text::muted),
            row![
                text(value).font(sola_kit::fonts::MONO).size(30)
                    .style(move |_: &Theme| iced::widget::text::Style { color: Some(value_color) }),
            ],
        ]
        .spacing(3),
        iced::widget::Space::new().width(Length::Fill),
        column(identity).spacing(3).align_x(iced::alignment::Horizontal::Right),
    ]
    .align_y(iced::alignment::Vertical::Top);

    let mut col = column![header].spacing(14);
    for el in body {
        col = col.push(el);
    }
    popover(col.padding(4)).padding(Padding::new(8.0)).width(Length::Fixed(CARD_WIDTH)).into()
}

/// A thin labeled caption row used above sub-sections.
fn caption<'a>(left: &'a str, right: String) -> Element<'a, Msg> {
    row![
        text(left).size(11).style(sola_kit::components::text::muted),
        iced::widget::Space::new().width(Length::Fill),
        text(right).font(sola_kit::fonts::MONO).size(11)
            .style(|_: &Theme| iced::widget::text::Style { color: Some(Color { a: 0.5, ..Color::from_rgb(0.902,0.929,0.953) }) }),
    ].into()
}
```

- [ ] **Step 2: Replace `cpu_card` with the full panel** — in `stats/view.rs`:

```rust
fn cpu_card(shell: &Shell) -> Element<'_, Msg> {
    let s = &shell.stats;
    let neutral = Color::from_rgb(0.902, 0.929, 0.953);
    let detail = match &s.detail {
        Some(crate::stats::Detail::Cpu(d)) => Some(d),
        _ => None,
    };

    let identity = vec![
        text("Ryzen 9 5950X").size(12).style(|_: &Theme| iced::widget::text::Style { color: Some(Color::from_rgb(0.788,0.820,0.851)) }).into(),
        text("16C / 32T").font(sola_kit::fonts::MONO).size(11).style(sola_kit::components::text::muted).into(),
    ];

    let mut samples = shell_cpu_hist(shell);
    let graph = column![
        caption("Last 60 seconds", format!("peak {:.0}%", peak(&samples))),
        graph_box(history_graph(std::mem::take(&mut samples), 100.0, Color::from_rgb(0.0,0.831,1.0))),
    ].spacing(6).into();

    let mut body: Vec<Element<'_, Msg>> = vec![graph];

    if let Some(d) = detail {
        // per-core meters
        let bars: Vec<Element<'_, Msg>> = d.per_core.iter().map(|p| core_bar(*p)).collect();
        body.push(column![
            caption("Per-thread load", format!("{} threads", d.per_core.len())),
            row(bars).spacing(1.5).align_y(iced::alignment::Vertical::Bottom),
        ].spacing(6).into());
        // top processes
        body.push(divider());
        body.push(caption("Top processes", "by CPU".into()));
        for p in &d.top {
            body.push(proc_row(&p.name, format!("{:.0}%", p.value), p.value, d.top.first().map(|t| t.value).unwrap_or(1.0)));
        }
        // footer
        body.push(divider());
        body.push(footer_pair("LOAD AVG", format!("{:.1}  {:.1}  {:.1}", d.load[0], d.load[1], d.load[2]), "UPTIME", fmt_uptime(d.uptime_secs)));
    }

    stat_card("CPU", format!("{:.0}%", s.cpu_pct), crate::stats::level_color(s.cpu_pct, neutral), identity, body)
}
```

- [ ] **Step 3: Add the small view helpers** — add to `stats/view.rs`:

```rust
fn shell_cpu_hist(shell: &Shell) -> Vec<f32> {
    // history is &mut for compaction; clone the contents instead of borrowing mut here.
    shell.cpu_hist_samples()
}

fn peak(samples: &[f32]) -> f32 { samples.iter().copied().fold(0.0, f32::max) }

fn graph_box<'a>(inner: Element<'a, Msg>) -> Element<'a, Msg> {
    container(inner).height(Length::Fixed(58.0))
        .style(|_: &Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.051,0.067,0.090))),
            border: iced::Border { radius: 6.0.into(), width: 1.0, color: Color::from_rgb(0.129,0.149,0.176) },
            ..Default::default()
        }).into()
}

fn core_bar<'a>(pct: f32) -> Element<'a, Msg> {
    let h = (pct / 100.0 * 22.0).clamp(2.0, 22.0);
    container(text("")).width(Length::Fixed(5.0)).height(Length::Fixed(h))
        .style(|_: &Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.122,0.435,0.922))),
            border: iced::Border { radius: 1.0.into(), ..Default::default() },
            ..Default::default()
        }).into()
}

fn divider<'a>() -> Element<'a, Msg> {
    container(text("")).width(Length::Fixed(288.0)).height(Length::Fixed(1.0))
        .style(|_: &Theme| iced::widget::container::Style { background: Some(iced::Background::Color(Color::from_rgb(0.129,0.149,0.176))), ..Default::default() }).into()
}

fn proc_row<'a>(name: &'a str, val: String, value: f32, max: f32) -> Element<'a, Msg> {
    let frac = if max > 0.0 { (value / max).clamp(0.0, 1.0) } else { 0.0 };
    column![
        row![ text(name.to_string()).size(13).style(|_: &Theme| iced::widget::text::Style { color: Some(Color::from_rgb(0.788,0.820,0.851)) }),
              iced::widget::Space::new().width(Length::Fill),
              text(val).font(sola_kit::fonts::MONO).size(12) ],
        container(
            container(text("")).width(Length::FillPortion((frac * 1000.0) as u16)).height(Length::Fixed(3.0))
                .style(|_: &Theme| iced::widget::container::Style { background: Some(iced::Background::Color(Color::from_rgb(0.122,0.435,0.922))), border: iced::Border { radius: 2.0.into(), ..Default::default() }, ..Default::default() })
        ).width(Length::Fixed(288.0)).height(Length::Fixed(3.0))
         .style(|_: &Theme| iced::widget::container::Style { background: Some(iced::Background::Color(Color::from_rgb(0.102,0.122,0.153))), border: iced::Border { radius: 2.0.into(), ..Default::default() }, ..Default::default() }),
    ].spacing(3).into()
}

fn footer_pair<'a>(l1: &'a str, v1: String, l2: &'a str, v2: String) -> Element<'a, Msg> {
    let cell = |label: &'a str, val: String, right: bool| {
        let c = column![
            text(label).size(10).style(sola_kit::components::text::muted),
            text(val).font(sola_kit::fonts::MONO).size(12).style(|_: &Theme| iced::widget::text::Style { color: Some(Color::from_rgb(0.788,0.820,0.851)) }),
        ].spacing(3);
        if right { c.align_x(iced::alignment::Horizontal::Right) } else { c }
    };
    row![ cell(l1, v1, false), iced::widget::Space::new().width(Length::Fill), cell(l2, v2, true) ].into()
}

fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86400; let h = (secs % 86400) / 3600; let m = (secs % 3600) / 60;
    if d > 0 { format!("{d}d {h}h {m}m") } else { format!("{h}h {m}m") }
}
```

- [ ] **Step 4: Add the history accessor on Shell** — in `app.rs`, add a method on `impl Shell` (the view needs samples without holding `&mut`):

```rust
    pub fn cpu_hist_samples(&self) -> Vec<f32> { self.cpu_hist_vec() }
```

Since `History::samples()` needs `&mut`, add a non-mut snapshot to `History` in `stats/mod.rs`:

```rust
    /// Clone of samples oldest→newest (no compaction; for read-only views).
    pub fn to_vec(&self) -> Vec<f32> {
        let (a, b) = self.buf.as_slices();
        a.iter().chain(b).copied().collect()
    }
```

and implement `cpu_hist_vec` simply as `self.cpu_hist.to_vec()`. (Replace `shell_cpu_hist`/`cpu_hist_samples` calls accordingly — use `shell.cpu_hist.to_vec()` directly in `cpu_card` and drop the helper.)

- [ ] **Step 5: Build + manual verify**

Run: `cargo make build sola-shell && cargo test -p sola-shell`
Expected: compiles, tests pass. (User: the CPU dropdown matches the paper.design mock — graph, per-thread bars, top processes, load/uptime.)

- [ ] **Step 6: Commit**

```bash
git add crates/sola-shell/src/stats/ crates/sola-shell/src/app.rs
git commit -m "feat(sola-shell): full CPU dropdown (graph, per-core, processes, footer)"
```

---

## Phase 4 — Memory

### Task 11: `/proc/meminfo` parser + tier-1 + tier-2

**Files:**
- Modify: `crates/sola-shell/src/stats/mem.rs`, `crates/sola-shell/src/stats/mod.rs`

- [ ] **Step 1: Write the failing test** — replace `mem.rs` with tests + impl. First the test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    const MEMINFO: &str = "MemTotal:      131000000 kB\nMemFree:        2000000 kB\nMemAvailable:   40000000 kB\nBuffers:         500000 kB\nCached:        20000000 kB\nSwapTotal:       8000000 kB\nSwapFree:        8000000 kB\n";

    #[test]
    fn parses_fields_and_pressure() {
        let m = parse_meminfo(MEMINFO);
        assert_eq!(m.total_kb, 131000000);
        // pressure = (total - available)/total = (131-40)/131 ≈ 69.5%
        assert!((m.pressure_pct() - 69.46).abs() < 0.1);
    }

    #[test]
    fn segments_sum_reasonably() {
        let m = parse_meminfo(MEMINFO);
        let (used, cache, free) = m.segments_kb();
        assert_eq!(free, 2000000);
        assert_eq!(cache, 20500000); // Cached + Buffers
        assert_eq!(used, 131000000 - 40000000); // total - available
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sola-shell stats::mem`
Expected: FAIL.

- [ ] **Step 3: Implement** — `mem.rs` top:

```rust
//! Memory sampling from /proc/meminfo and /proc/<pid>/status (RSS).

use crate::stats::cpu::Proc;

#[derive(Clone, Copy, Debug, Default)]
pub struct MemInfo {
    pub total_kb: u64,
    pub avail_kb: u64,
    pub free_kb: u64,
    pub buffers_kb: u64,
    pub cached_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

impl MemInfo {
    pub fn pressure_pct(&self) -> f32 {
        if self.total_kb == 0 { return 0.0; }
        let used = self.total_kb.saturating_sub(self.avail_kb) as f32;
        (used / self.total_kb as f32) * 100.0
    }
    /// (used, cache, free) in kB. used = total-available, cache = cached+buffers.
    pub fn segments_kb(&self) -> (u64, u64, u64) {
        (self.total_kb.saturating_sub(self.avail_kb), self.cached_kb + self.buffers_kb, self.free_kb)
    }
}

pub fn parse_meminfo(s: &str) -> MemInfo {
    let mut m = MemInfo::default();
    for line in s.lines() {
        let mut it = line.split_whitespace();
        let key = it.next().unwrap_or("");
        let val: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        match key {
            "MemTotal:" => m.total_kb = val,
            "MemAvailable:" => m.avail_kb = val,
            "MemFree:" => m.free_kb = val,
            "Buffers:" => m.buffers_kb = val,
            "Cached:" => m.cached_kb = val,
            "SwapTotal:" => m.swap_total_kb = val,
            "SwapFree:" => m.swap_free_kb = val,
            _ => {}
        }
    }
    m
}

pub fn pressure_pct() -> f32 {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default()).pressure_pct()
}

#[derive(Clone, Debug, Default)]
pub struct MemDetail {
    pub info: MemInfo,
    pub top: Vec<Proc>, // by RSS, value in MB
}

pub fn detail() -> MemDetail {
    let info = parse_meminfo(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default());
    let mut top = Vec::new();
    if let Ok(dir) = std::fs::read_dir("/proc") {
        for ent in dir.flatten() {
            let Ok(pid) = ent.file_name().to_string_lossy().parse::<i32>() else { continue };
            let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else { continue };
            let rss_kb = status.lines().find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok());
            if let Some(kb) = rss_kb {
                if kb > 50_000 { // >~50MB
                    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim().to_string();
                    top.push(Proc { name, value: kb as f32 / 1024.0 });
                }
            }
        }
    }
    crate::stats::cpu::cap_top(&mut top, 4);
    MemDetail { info, top }
}
```

> Delete the Phase-1 stub `MemDetail`/`pressure_pct`/`detail` definitions you're replacing.

- [ ] **Step 4: Wire the mem detail arm** — in `stats/mod.rs::stats_stream`, set the `Some(Metric::Mem)` arm to `Some(Detail::Mem(mem::detail()))`.

- [ ] **Step 5: Run tests + build**

Run: `cargo test -p sola-shell stats::mem && cargo make build sola-shell`
Expected: PASS, compiles.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-shell/src/stats/
git commit -m "feat(sola-shell): memory sampling (/proc/meminfo + RSS)"
```

---

### Task 12: MEM indicator + dropdown

**Files:**
- Modify: `crates/sola-shell/src/menubar/view.rs`, `crates/sola-shell/src/stats/view.rs`

- [ ] **Step 1: Add the MEM indicator** — in `menubar/view.rs`, after the CPU button, add a GPU/MEM cluster. Add a `mem_btn` mirroring `cpu_btn`:

```rust
    let mem_pct = shell.stats.mem_pct;
    let mem_btn: Element<'_, Msg> = iced::widget::button(
        stat_indicator("MEM", format!("{:.0}%", mem_pct), crate::stats::level_color(mem_pct, neutral)),
    )
    .style(kit_btn::menubar(shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Mem))))
    .padding([2, 8])
    .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Mem))
    .into();
```

Insert `mem_btn` into the row between `cpu_btn` and `clock` (final order CPU, GPU, MEM, NET, clock — GPU/NET added in their phases; for now CPU, MEM).

- [ ] **Step 2: Add the mem card** — in `stats/view.rs`, replace the `Metric::Mem => placeholder("Memory")` arm with `Metric::Mem => mem_card(shell)` and add:

```rust
fn mem_card(shell: &Shell) -> Element<'_, Msg> {
    let s = &shell.stats;
    let neutral = Color::from_rgb(0.902, 0.929, 0.953);
    let detail = match &s.detail { Some(crate::stats::Detail::Mem(d)) => Some(d), _ => None };

    let total_gb = detail.map(|d| d.info.total_kb as f32 / 1024.0 / 1024.0).unwrap_or(0.0);
    let identity = vec![
        text(format!("{total_gb:.0} GB")).size(12).style(|_: &Theme| iced::widget::text::Style { color: Some(Color::from_rgb(0.788,0.820,0.851)) }).into(),
        text("RAM").font(sola_kit::fonts::MONO).size(11).style(sola_kit::components::text::muted).into(),
    ];

    let graph = column![
        caption("Last 60 seconds", format!("peak {:.0}%", peak(&shell.mem_hist.to_vec()))),
        graph_box(history_graph(shell.mem_hist.to_vec(), 100.0, Color::from_rgb(0.0,0.831,1.0))),
    ].spacing(6).into();
    let mut body: Vec<Element<'_, Msg>> = vec![graph];

    if let Some(d) = detail {
        let (used, cache, free) = d.info.segments_kb();
        body.push(column![ caption("Memory", String::new()), seg_bar(used, cache, free) ].spacing(6).into());
        body.push(divider());
        body.push(caption("Top processes", "by RAM".into()));
        let max = d.top.first().map(|t| t.value).unwrap_or(1.0);
        for p in &d.top { body.push(proc_row(&p.name, format!("{:.0} MB", p.value), p.value, max)); }
        body.push(divider());
        let swap_used = (d.info.swap_total_kb.saturating_sub(d.info.swap_free_kb)) as f32 / 1024.0 / 1024.0;
        let swap_tot = d.info.swap_total_kb as f32 / 1024.0 / 1024.0;
        body.push(footer_pair("SWAP", format!("{swap_used:.1} / {swap_tot:.0} GB"), "PRESSURE", format!("{:.0}%", s.mem_pct)));
    }

    stat_card("MEM", format!("{:.0}%", s.mem_pct), crate::stats::level_color(s.mem_pct, neutral), identity, body)
}

/// Three-segment used/cache/free bar.
fn seg_bar<'a>(used: u64, cache: u64, free: u64) -> Element<'a, Msg> {
    let total = (used + cache + free).max(1);
    let seg = |kb: u64, color: Color| container(text("")).width(Length::FillPortion(((kb as f32 / total as f32) * 1000.0) as u16)).height(Length::Fixed(8.0))
        .style(move |_: &Theme| iced::widget::container::Style { background: Some(iced::Background::Color(color)), ..Default::default() });
    container(row![
        seg(used, Color::from_rgb(0.0,0.831,1.0)),
        seg(cache, Color::from_rgb(0.122,0.435,0.922)),
        seg(free, Color::from_rgb(0.188,0.211,0.243)),
    ]).width(Length::Fixed(288.0)).height(Length::Fixed(8.0))
      .style(|_: &Theme| iced::widget::container::Style { border: iced::Border { radius: 4.0.into(), ..Default::default() }, ..Default::default() }).into()
}
```

- [ ] **Step 3: Build + manual verify**

Run: `cargo make build sola-shell`
Expected: compiles. (User: MEM indicator + dropdown with segmented bar, top-by-RAM, swap.)

- [ ] **Step 4: Commit**

```bash
git add crates/sola-shell/src/menubar/view.rs crates/sola-shell/src/stats/view.rs
git commit -m "feat(sola-shell): MEM indicator + dropdown"
```

---

## Phase 5 — Network

### Task 13: `/proc/net/dev` parser + rate + iface/IP

**Files:**
- Modify: `crates/sola-shell/src/stats/net.rs`, `crates/sola-shell/src/stats/mod.rs`, `crates/sola-shell/Cargo.toml`

- [ ] **Step 1: Add `nix`** — in `crates/sola-shell/Cargo.toml` `[dependencies]`:

```toml
nix = { version = "0.29", features = ["net"] }
```

- [ ] **Step 2: Write the failing test** — `net.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    const DEV: &str = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets\n    lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0\n  eth0: 5000 50 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n";

    #[test]
    fn parses_counters_skipping_lo() {
        let c = parse_dev(DEV);
        assert_eq!(c.get("eth0"), Some(&(5000, 2000)));
        assert_eq!(c.get("lo"), None); // loopback excluded
    }

    #[test]
    fn rate_from_delta() {
        let mut prev = Counters::default();
        prev.0.insert("eth0".into(), (1000, 500));
        let mut cur = Counters::default();
        cur.0.insert("eth0".into(), (3000, 1500));
        // over 2s: down=(3000-1000)/2=1000 B/s, up=(1500-500)/2=500 B/s
        let (d, u) = rate_for(&prev, &cur, "eth0", 2.0);
        assert_eq!((d, u), (1000.0, 500.0));
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p sola-shell stats::net`
Expected: FAIL.

- [ ] **Step 4: Implement** — `net.rs` top (replace Phase-1 stubs):

```rust
//! Network sampling from /proc/net/dev, default route, getifaddrs.

use std::collections::HashMap;

/// rx_bytes, tx_bytes per interface (loopback excluded).
#[derive(Clone, Debug, Default)]
pub struct Counters(pub HashMap<String, (u64, u64)>);
impl Counters {
    pub fn get(&self, iface: &str) -> Option<&(u64, u64)> { self.0.get(iface) }
}

pub fn parse_dev(s: &str) -> Counters {
    let mut c = Counters::default();
    for line in s.lines() {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let name = name.trim();
        if name == "lo" || name.is_empty() { continue; }
        let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        if f.len() >= 9 { c.0.insert(name.to_string(), (f[0], f[8])); }
    }
    c
}

pub fn read_counters() -> Counters {
    parse_dev(&std::fs::read_to_string("/proc/net/dev").unwrap_or_default())
}

pub fn rate_for(prev: &Counters, cur: &Counters, iface: &str, dt: f32) -> (f32, f32) {
    match (prev.get(iface), cur.get(iface)) {
        (Some(&(pr, pt)), Some(&(cr, ct))) if dt > 0.0 =>
            ((cr.saturating_sub(pr) as f32) / dt, (ct.saturating_sub(pt) as f32) / dt),
        _ => (0.0, 0.0),
    }
}

/// Default-route interface name from /proc/net/route (destination 00000000).
pub fn default_iface() -> Option<String> {
    let s = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in s.lines().skip(1) {
        let mut it = line.split_whitespace();
        let iface = it.next()?;
        let dest = it.next()?;
        if dest == "00000000" { return Some(iface.to_string()); }
    }
    None
}

/// Rate on the default interface (used by the bar).
pub fn rate(prev: &Counters, cur: &Counters, dt: f32) -> (f32, f32) {
    match default_iface() {
        Some(iface) => rate_for(prev, cur, &iface, dt),
        None => (0.0, 0.0),
    }
}

/// IPv4 address of `iface` via getifaddrs.
pub fn iface_ip(iface: &str) -> Option<String> {
    use nix::ifaddrs::getifaddrs;
    for ifa in getifaddrs().ok()? {
        if ifa.interface_name == iface {
            if let Some(addr) = ifa.address.and_then(|a| a.as_sockaddr_in().map(|s| s.ip())) {
                return Some(std::net::Ipv4Addr::from(addr).to_string());
            }
        }
    }
    None
}

#[derive(Clone, Debug, Default)]
pub struct NetDetail {
    pub iface: String,
    pub ip: String,
    pub total_down: u64, // cumulative bytes since shell start
    pub total_up: u64,
}

pub fn detail(cur: &Counters) -> NetDetail {
    let iface = default_iface().unwrap_or_default();
    let ip = iface_ip(&iface).unwrap_or_else(|| "—".into());
    let (down, up) = cur.get(&iface).copied().unwrap_or((0, 0));
    NetDetail { iface, ip, total_down: down, total_up: up }
}
```

- [ ] **Step 5: Wire the net arm** — in `stats/mod.rs::stats_stream`, set `Some(Metric::Net)` to `Some(Detail::Net(net::detail(&cur_net)))`.

- [ ] **Step 6: Run tests + build**

Run: `cargo test -p sola-shell stats::net && cargo make build sola-shell`
Expected: PASS, compiles.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-shell/src/stats/ crates/sola-shell/Cargo.toml
git commit -m "feat(sola-shell): network sampling (/proc/net/dev + iface/IP)"
```

---

### Task 14: NET indicator (2-line) + dropdown (dual graph)

**Files:**
- Modify: `crates/sola-shell/src/menubar/view.rs`, `crates/sola-shell/src/stats/view.rs`

- [ ] **Step 1: Add a rate formatter** — in `stats/view.rs`:

```rust
/// Human bytes/sec: B/s, KB/s, MB/s.
pub fn fmt_rate(bps: f32) -> String {
    if bps >= 1_000_000.0 { format!("{:.1} MB/s", bps / 1_000_000.0) }
    else if bps >= 1000.0 { format!("{:.0} KB/s", bps / 1000.0) }
    else { format!("{:.0} B/s", bps) }
}
```

- [ ] **Step 2: Add the NET indicator (two-line ↓/↑)** — in `menubar/view.rs`, after `mem_btn`:

```rust
    let net_inner = iced::widget::column![
        iced::widget::text(format!("↓ {}", crate::stats::view::fmt_rate(shell.stats.net_down)))
            .font(sola_kit::fonts::MONO).size(10),
        iced::widget::text(format!("↑ {}", crate::stats::view::fmt_rate(shell.stats.net_up)))
            .font(sola_kit::fonts::MONO).size(10),
    ].spacing(1);
    let net_btn: Element<'_, Msg> = iced::widget::button(net_inner)
        .style(kit_btn::menubar(shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Net))))
        .padding([2, 8])
        .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Net))
        .into();
```

Final indicator order in the row: `cpu_btn, gpu_btn, mem_btn, net_btn, clock` (gpu added in Phase 6 — for now `cpu_btn, mem_btn, net_btn, clock`).

- [ ] **Step 3: Add the net card** — in `stats/view.rs`, replace `Metric::Net => placeholder("Network")` with `Metric::Net => net_card(shell)` and add:

```rust
fn net_card(shell: &Shell) -> Element<'_, Msg> {
    let s = &shell.stats;
    let detail = match &s.detail { Some(crate::stats::Detail::Net(d)) => Some(d), _ => None };

    let identity = vec![
        text(format!("↓ {}", fmt_rate(s.net_down))).font(sola_kit::fonts::MONO).size(12)
            .style(|_: &Theme| iced::widget::text::Style { color: Some(Color::from_rgb(0.0,0.831,1.0)) }).into(),
        text(format!("↑ {}", fmt_rate(s.net_up))).font(sola_kit::fonts::MONO).size(12)
            .style(|_: &Theme| iced::widget::text::Style { color: Some(Color::from_rgb(0.247,0.725,0.314)) }).into(),
    ];

    let down = shell.net_down_hist.to_vec();
    let up = shell.net_up_hist.to_vec();
    let max = peak(&down).max(peak(&up)).max(1.0);
    let graph = column![
        caption("Last 60 seconds", format!("peak {}", fmt_rate(max))),
        graph_box(history_graph(down, max, Color::from_rgb(0.0,0.831,1.0))),
        graph_box(history_graph(up, max, Color::from_rgb(0.247,0.725,0.314))),
    ].spacing(6).into();

    let mut body: Vec<Element<'_, Msg>> = vec![graph];
    if let Some(d) = detail {
        body.push(divider());
        body.push(footer_pair("INTERFACE", format!("{}  {}", d.iface, d.ip), "SESSION", format!("↓{} ↑{}", fmt_bytes(d.total_down), fmt_bytes(d.total_up))));
    }
    // NET headline value uses the down rate; no threshold (rate, not level).
    stat_card("NET", fmt_rate(s.net_down), Color::from_rgb(0.902,0.929,0.953), identity, body)
}

fn fmt_bytes(b: u64) -> String {
    let f = b as f32;
    if f >= 1e9 { format!("{:.1} GB", f/1e9) } else if f >= 1e6 { format!("{:.0} MB", f/1e6) } else { format!("{:.0} KB", f/1e3) }
}
```

- [ ] **Step 4: Build + manual verify**

Run: `cargo make build sola-shell`
Expected: compiles. (User: NET shows ↓/↑ in the bar; dropdown has dual graph + iface/IP + session totals.)

- [ ] **Step 5: Commit**

```bash
git add crates/sola-shell/src/menubar/view.rs crates/sola-shell/src/stats/view.rs
git commit -m "feat(sola-shell): NET indicator + dropdown"
```

---

## Phase 6 — GPU (NVML)

### Task 15: NVML sampling with graceful absence

**Files:**
- Modify: `crates/sola-shell/src/stats/gpu.rs`, `crates/sola-shell/src/stats/mod.rs`, `crates/sola-shell/Cargo.toml`

- [ ] **Step 1: Add the dep** — in `crates/sola-shell/Cargo.toml`:

```toml
nvml-wrapper = "0.10"
```

- [ ] **Step 2: Implement** — replace `gpu.rs` stubs with:

```rust
//! GPU sampling via NVML (nvml-wrapper). All reads are best-effort; any failure
//! (no NVIDIA GPU, NVML not loadable) yields None so the indicator hides.

use std::sync::{Mutex, OnceLock};
use nvml_wrapper::Nvml;

use crate::stats::cpu::Proc;
use crate::stats::GpuLite;

fn nvml() -> Option<&'static Nvml> {
    static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
    NVML.get_or_init(|| Nvml::init().ok()).as_ref()
}

/// Tier-1 summary for the bar. None when no GPU/NVML.
pub fn lite() -> Option<GpuLite> {
    let dev = nvml()?.device_by_index(0).ok()?;
    let util = dev.utilization_rates().ok()?.gpu as f32;
    let temp = dev.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu).ok().unwrap_or(0) as f32;
    Some(GpuLite { util, temp_c: temp })
}

#[derive(Clone, Debug, Default)]
pub struct GpuDetail {
    pub name: String,
    pub util: f32,
    pub mem_used_mb: f32,
    pub mem_total_mb: f32,
    pub temp_c: f32,
    pub power_w: f32,
    pub fan_pct: f32,
    pub clock_mhz: u32,
    pub top: Vec<Proc>, // by VRAM (MB)
}

pub fn detail() -> Option<GpuDetail> {
    use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
    let n = nvml()?;
    let dev = n.device_by_index(0).ok()?;
    let mem = dev.memory_info().ok()?;
    let mut top = Vec::new();
    if let Ok(procs) = dev.running_compute_processes() {
        for p in procs {
            let used = match p.used_gpu_memory { nvml_wrapper::enums::device::UsedGpuMemory::Used(b) => b, _ => 0 };
            let name = std::fs::read_to_string(format!("/proc/{}/comm", p.pid)).unwrap_or_default().trim().to_string();
            top.push(Proc { name, value: used as f32 / 1024.0 / 1024.0 });
        }
    }
    crate::stats::cpu::cap_top(&mut top, 4);
    Some(GpuDetail {
        name: dev.name().unwrap_or_default(),
        util: dev.utilization_rates().ok().map(|u| u.gpu as f32).unwrap_or(0.0),
        mem_used_mb: mem.used as f32 / 1024.0 / 1024.0,
        mem_total_mb: mem.total as f32 / 1024.0 / 1024.0,
        temp_c: dev.temperature(TemperatureSensor::Gpu).unwrap_or(0) as f32,
        power_w: dev.power_usage().unwrap_or(0) as f32 / 1000.0,
        fan_pct: dev.fan_speed(0).unwrap_or(0) as f32,
        clock_mhz: dev.clock_info(Clock::Graphics).unwrap_or(0),
        top,
    })
}

// Keep a Mutex import referenced if needed by future caching; suppress unused.
#[allow(unused_imports)]
use std::sync::Mutex as _Mutex;
```

> Delete the Phase-1 `GpuDetail`/`lite`/`detail` stubs being replaced. Confirm the exact `nvml-wrapper` 0.10 API names with `cargo doc -p nvml-wrapper --open` if a signature differs (e.g. `used_gpu_memory` enum path).

- [ ] **Step 3: Wire the gpu arm** — in `stats/mod.rs::stats_stream`, the gpu detail arm is already `Some(Metric::Gpu) => gpu::detail().map(Detail::Gpu)`. Confirm `gpu::lite()` is called for tier-1 each tick (it is, in the `let gpu = gpu::lite();` line).

- [ ] **Step 4: Build**

Run: `cargo make build sola-shell`
Expected: compiles (NVML links; on this NVIDIA box `lite()` returns Some at runtime).

- [ ] **Step 5: Commit**

```bash
git add crates/sola-shell/src/stats/ crates/sola-shell/Cargo.toml
git commit -m "feat(sola-shell): GPU sampling via NVML"
```

---

### Task 16: GPU indicator (hide if absent) + dropdown

**Files:**
- Modify: `crates/sola-shell/src/menubar/view.rs`, `crates/sola-shell/src/stats/view.rs`

- [ ] **Step 1: Add the GPU indicator, conditionally** — in `menubar/view.rs`, build the right cluster as a `Vec` so GPU can be omitted when absent:

```rust
    let mut cluster: Vec<Element<'_, Msg>> = vec![cpu_btn];
    if let Some(g) = shell.stats.gpu {
        let gpu_btn: Element<'_, Msg> = iced::widget::button(
            stat_indicator("GPU", format!("{:.0}%", g.util), crate::stats::level_color(g.util, neutral)),
        )
        .style(kit_btn::menubar(shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Gpu))))
        .padding([2, 8])
        .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Gpu))
        .into();
        cluster.push(gpu_btn);
    }
    cluster.push(mem_btn);
    cluster.push(net_btn);
    cluster.push(clock);
```

Then assemble: `row![ row(left), Space::Fill, toast, row(cluster).spacing(16).align_y(Center) ]`.

- [ ] **Step 2: Add the gpu card** — in `stats/view.rs`, replace `Metric::Gpu => placeholder("GPU")` with `Metric::Gpu => gpu_card(shell)` and add:

```rust
fn gpu_card(shell: &Shell) -> Element<'_, Msg> {
    let s = &shell.stats;
    let neutral = Color::from_rgb(0.902, 0.929, 0.953);
    let util = s.gpu.map(|g| g.util).unwrap_or(0.0);
    let detail = match &s.detail { Some(crate::stats::Detail::Gpu(d)) => Some(d), _ => None };

    let identity = vec![
        text(detail.map(|d| short_gpu(&d.name)).unwrap_or_else(|| "GPU".into())).size(12)
            .style(|_: &Theme| iced::widget::text::Style { color: Some(Color::from_rgb(0.788,0.820,0.851)) }).into(),
        text(detail.map(|d| format!("{:.0} GB", d.mem_total_mb/1024.0)).unwrap_or_default())
            .font(sola_kit::fonts::MONO).size(11).style(sola_kit::components::text::muted).into(),
    ];

    let graph = column![
        caption("Last 60 seconds", format!("peak {:.0}%", peak(&shell.gpu_hist.to_vec()))),
        graph_box(history_graph(shell.gpu_hist.to_vec(), 100.0, Color::from_rgb(0.0,0.831,1.0))),
    ].spacing(6).into();
    let mut body: Vec<Element<'_, Msg>> = vec![graph];

    if let Some(d) = detail {
        // VRAM bar
        let frac = if d.mem_total_mb > 0.0 { d.mem_used_mb / d.mem_total_mb } else { 0.0 };
        body.push(column![
            caption("VRAM", format!("{:.1} / {:.0} GB", d.mem_used_mb/1024.0, d.mem_total_mb/1024.0)),
            level_bar(frac, Color::from_rgb(0.0,0.831,1.0)),
        ].spacing(6).into());
        body.push(divider());
        body.push(footer_pair("TEMP", format!("{:.0}°C", d.temp_c), "POWER", format!("{:.0} W", d.power_w)));
        body.push(footer_pair("FAN", format!("{:.0}%", d.fan_pct), "CLOCK", format!("{} MHz", d.clock_mhz)));
        if !d.top.is_empty() {
            body.push(divider());
            body.push(caption("Top processes", "by VRAM".into()));
            let max = d.top.first().map(|t| t.value).unwrap_or(1.0);
            for p in &d.top { body.push(proc_row(&p.name, format!("{:.0} MB", p.value), p.value, max)); }
        }
    }

    stat_card("GPU", format!("{:.0}%", util), crate::stats::level_color(util, neutral), identity, body)
}

/// "NVIDIA GeForce RTX 3090 Ti" → "RTX 3090 Ti".
fn short_gpu(name: &str) -> String {
    name.rsplit("GeForce ").next().unwrap_or(name).to_string()
}

/// Single horizontal fill bar (0..1).
fn level_bar<'a>(frac: f32, color: Color) -> Element<'a, Msg> {
    container(
        container(text("")).width(Length::FillPortion((frac.clamp(0.0,1.0) * 1000.0) as u16)).height(Length::Fixed(8.0))
            .style(move |_: &Theme| iced::widget::container::Style { background: Some(iced::Background::Color(color)), border: iced::Border { radius: 4.0.into(), ..Default::default() }, ..Default::default() })
    ).width(Length::Fixed(288.0)).height(Length::Fixed(8.0))
     .style(|_: &Theme| iced::widget::container::Style { background: Some(iced::Background::Color(Color::from_rgb(0.188,0.211,0.243))), border: iced::Border { radius: 4.0.into(), ..Default::default() }, ..Default::default() }).into()
}
```

- [ ] **Step 3: Build + manual verify**

Run: `cargo make build sola-shell && cargo test -p sola-shell`
Expected: compiles, tests pass. (User: GPU indicator + dropdown with VRAM bar, temp/power/fan/clock, per-process VRAM.)

- [ ] **Step 4: Commit**

```bash
git add crates/sola-shell/src/menubar/view.rs crates/sola-shell/src/stats/view.rs
git commit -m "feat(sola-shell): GPU indicator + dropdown"
```

---

## Phase 7 — Polish

### Task 17: First-tick `—`, panel-close clears active metric, final review

**Files:**
- Modify: `crates/sola-shell/src/menubar/view.rs`, `crates/sola-shell/src/app.rs`

- [ ] **Step 1: Guard against the empty first sample** — in `menubar/view.rs`, before building indicators, if `shell.stats` is the default (all zero AND no history yet) the values read `0%` which is acceptable; no change required unless you prefer `—`. To show `—` until the first real tick, gate on `shell.cpu_hist.to_vec().is_empty()`:

```rust
    let cpu_label = if shell.cpu_hist.to_vec().is_empty() { "—".to_string() } else { format!("{:.0}%", cpu_pct) };
```

Apply the same pattern to mem/gpu values. (Net already reads `0 B/s`, fine.)

- [ ] **Step 2: Verify CloseMenu clears the sampler** — confirm `Msg::CloseMenu` in `app.rs` contains `crate::stats::set_active_metric(None);` (added in Task 6 Step 3). Confirm `Msg::ToggleCalendar` opening path and `Msg::OpenMenu` also call `set_active_metric(None)` so switching from a stat panel to the calendar/menu stops tier-2 sampling. Add `crate::stats::set_active_metric(None);` to the `Msg::ToggleCalendar` open branch and the `Msg::OpenMenu` open path if missing.

- [ ] **Step 3: Full build + test**

Run: `cargo make build && cargo test -p sola-shell`
Expected: whole workspace compiles, all shell tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-shell/src/
git commit -m "feat(sola-shell): stats polish — first-tick dash, sampler gating"
```

---

## Done

After Task 17 the feature is complete: four live indicators (CPU · GPU · MEM · NET) left of the clock, each opening a rich detail dropdown, sampled shell-direct with two tiers, GPU via NVML. The user installs (`cargo make install sola-shell`) and verifies against the paper.design "OS Stats" doc.

**Deferred (per spec, not in this plan):** disk-usage indicator, per-app network in the NET dropdown, per-indicator configurability.
