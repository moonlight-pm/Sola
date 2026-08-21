# Menubar System Monitors — Design

**Status:** Design approved (brainstorm). Next: implementation plan.
**Date:** 2026-06-16
**Visual reference:** paper.design "OS Stats" — <https://app.paper.design/file/01KV9H6Q3PQMG1GTAFADB88SZ3/1-0>

## Goal

Add iStatMenus-style system-monitor indicators to the `sola-shell` menubar,
sitting to the **left of the clock**. Each indicator shows a live value and is a
click target that opens a detail dropdown for that metric. This reuses the
menubar-panel mechanism built for the clock calendar.

## Scope

Four indicators, left → right, then the divider and clock:

```
… Terminal  File  Edit  View              CPU 34%  GPU 22%  MEM 61%  ↓2.4 ↑0.1 MB/s │ 14:32 Tue Jun 16
```

- **CPU** — overall activity %
- **GPU** — utilization % (NVIDIA only)
- **MEM** — memory pressure %
- **NET** — download / upload rate (stacked ↓ / ↑)

**Decisions made during brainstorming:**

- **Bar style = "numbers only"** (Treatment A): a small muted label + a mono
  value per metric; network is a two-line `↓ rate / ↑ rate` stack. No
  sparklines in the bar itself — trend lives in the dropdown.
- **Disk usage was dropped** from this iteration (was a fifth indicator). May
  return later.
- **GPU is NVIDIA-only** (dev box is an RTX 3090 Ti). The indicator hides
  gracefully when no NVIDIA GPU / NVML is present.
- **Rich dropdowns** (see template below) — not trimmed.
- **Shell-direct sampling** — no new daemon, no new bus topic. `sola-shell`
  samples `/proc` and NVML itself on a background thread.

## Visual design

Matches the existing shell design system: near-black menubar `#0d1117`, panel
cards `#161b22` with a `#30363d` hairline border and 12px radius, Inter for
labels, **JetBrains Mono for all numeric readouts** (tabular — no jitter as
values change), cyan `#00d4ff` accent, muted text `#8b949e`.

### Bar indicators (numbers only)

Each indicator is a button (like the clock) with a small uppercase muted label
and a mono value, e.g. `CPU 34%`. Network is a right-aligned two-line stack:

```
↓ 2.4 MB/s
↑ 0.1 MB/s
```

**Threshold coloring:** values are neutral (`#e6edf3`) until a metric crosses a
threshold, then the value (not the whole indicator) tints — amber `#d29922`
above ~75%, red `#f85149` above ~90%. Net has no threshold (rate, not a level).
Thresholds are constants, tunable later.

### Dropdown template

Validated in the paper.design doc with the CPU panel. A ~320px card, same chrome
as the calendar/menu popover, structured as:

1. **Header** — metric label + large mono value (e.g. `34 %`), with the hardware
   identity right-aligned (`Ryzen 9 5950X · 16C / 32T · 3.8 GHz`).
2. **History graph** — 60-second area/line chart of the metric (cyan), with a
   caption (`Last 60 seconds`) and a peak readout.
3. **Per-metric middle section** (differs by metric — see below).
4. **Top processes** — ranked list (name + %), with a thin proportion bar per
   row.
5. **Footer** — two metric-specific stat pairs (e.g. `LOAD AVG 4.2 3.8 3.1` /
   `UPTIME 3d 4h 12m`).

Per-metric middle sections and footers:

| Metric | Middle section | Footer stats | Top processes |
| ------ | -------------- | ------------ | ------------- |
| **CPU** | Per-thread load — row of 32 small vertical meters | Load average · Uptime | by CPU % |
| **GPU** | VRAM used/total bar (e.g. `2.1 / 24 GB`) | Temp · Power · Fan · Clock | by GPU (SM %) then by VRAM (compute + graphics) |
| **MEM** | Segmented bar: used / cache+buffers / free | Swap used/total · Total RAM | by RSS |
| **NET** | Dual ↓/↑ history (the graph itself is dual-series) | Interface + local IP · Session ↓/↑ totals | *deferred (see below)* |

## Architecture — shell-direct, two-tier sampling

A new `sola-shell/src/stats/` module owns sampling and the snapshot model. There
is **no separate process and no bus topic**; the shell reads the system directly.

### Two sampling tiers

- **Tier 1 — always on, ~1s tick.** The cheap aggregates that feed the bar:
  CPU % (overall), GPU util + temp, memory pressure %, net ↓/↑ rate. These are a
  handful of small `/proc` reads plus a few NVML calls — negligible cost.
- **Tier 2 — only while a dropdown is open, ~1s tick.** The expensive per-metric
  detail for the *one* open metric: per-core array, top-process enumeration
  (`/proc/*/stat` / `/proc/*/status` scan over all PIDs), GPU per-process list,
  VRAM/power/clocks. Skipped entirely when no panel is open, so the shell stays
  light at rest.

The sampler runs on a **dedicated background thread** with an `mpsc` channel,
surfaced to iced as a `Subscription<Msg>` (mirroring the kit's `bus_subscription`
polling-thread pattern). Sampling never runs on the render thread. Each tick the
thread sends `Msg::StatsTick(Snapshot)` into `update`.

The thread learns which metric (if any) has an open panel via a shared
`Arc<Mutex<Option<Metric>>>` (or an atomic) updated by the shell when a panel
opens/closes; it includes that metric's tier-2 detail in the next snapshot.

### History

Per-metric ring buffers (~60 `f32` samples ≈ 60s at the ~1s tick) live in shell
state and feed the dropdown history graphs. The tier-1 aggregates fill the
buffers continuously (open or not), so a graph has history the moment its panel
opens. For NET the buffer holds `(down, up)` pairs.

### Data sources

- **CPU** — `/proc/stat` (aggregate `cpu` line + per-core `cpuN` lines; % from
  the idle/total delta between two samples), `/proc/loadavg`, `/proc/uptime`.
  Top processes: scan `/proc/<pid>/stat` (utime+stime delta over the interval),
  name from `/proc/<pid>/comm`.
- **MEM** — `/proc/meminfo` (`MemTotal`, `MemAvailable`, `MemFree`, `Buffers`,
  `Cached`, `SwapTotal`, `SwapFree`). Headline pressure % = `(MemTotal −
  MemAvailable) / MemTotal`. Optionally surface `/proc/pressure/memory` (PSI
  `some avg10`) as a "pressure" stat in the dropdown. Segments: used =
  `MemTotal − MemAvailable`; cache = `Cached + Buffers`; free = `MemFree`. Top
  processes: `/proc/<pid>/status` `VmRSS` (or `statm` resident pages × page
  size).
- **NET** — `/proc/net/dev` (rx/tx byte counters per interface; rate = byte
  delta / interval). Primary interface = the default-route interface from
  `/proc/net/route` (destination `00000000`). Local IP via `getifaddrs`
  (`nix::ifaddrs`). Session totals accumulate since shell start.
- **GPU** — **`nvml-wrapper`** crate (direct NVML library calls; init the
  `Nvml` handle once and reuse). Reads: `utilization_rates()`,
  `memory_info()`, `temperature(Sensor::Gpu)`, `power_usage()`,
  `fan_speed(0)`, `clock_info(Clock::Graphics)`, `process_utilization_stats()`
  for per-process SM (compute) %, and the running compute / graphics process
  lists for the per-process VRAM table. **Fallback:** if `Nvml::init()` fails,
  optionally shell out to `nvidia-smi`; if that also fails, the GPU indicator
  is hidden. Chosen over spawning `nvidia-smi` every tick to avoid a
  per-second subprocess. SM util is Maxwell+; if the call is unsupported the
  GPU process list is omitted and VRAM ranking still shows.

## Dropdown plumbing (reuse the calendar's panel mechanism)

Generalize the calendar's single boolean into a panel enum:

- Replace `Shell::current_open_is_calendar: bool` with
  `Shell::open_panel: Option<Panel>` where
  `enum Panel { Calendar, Stat(Metric) }`.
- `menu_open` continues to gate the Menu window's composition visibility (a
  panel being open implies `menu_open`).
- `menu/view::view` dispatches on `open_panel`: `Calendar` → calendar card,
  `Stat(m)` → that metric's stat card, else the app/system menu.
- Opening a stat panel mirrors `ToggleCalendar`: set `open_panel =
  Some(Stat(m))`, clear menu index/system flags, `emit_composition()`, and tell
  the sampler the active metric. Backdrop click / `CloseMenu` clears it.

**Anchoring.** Each indicator reports its laid-out x (like menubar labels report
`label_positions`) so its panel anchors under it, reusing the right-anchored
positioning from the calendar. (The calendar is anchored to the far right; stat
panels anchor under their specific indicator.)

## Files

New, small, focused modules under `sola-shell/src/`:

- `stats/mod.rs` — `Metric { Cpu, Gpu, Mem, Net }`, `Snapshot` (tier-1 aggregates
  + optional tier-2 detail), `History` ring buffers, the sampler subscription,
  threshold→color helper.
- `stats/cpu.rs`, `stats/mem.rs`, `stats/net.rs`, `stats/gpu.rs` — the parsers
  and per-tick samplers (the unit-tested core).
- `stats/view.rs` — the dropdown card per metric + a reusable history-graph
  widget (an `iced::canvas` area/line chart from a sample slice).

Modified:

- `menubar/view.rs` — the four indicators in the right cluster, left of the
  clock; each a button emitting `Msg::ToggleStatPanel(Metric)`.
- `app.rs` — `open_panel`, `Msg::{ToggleStatPanel, StatsTick}` + handlers; thread
  the open-metric to the sampler; the stats subscription.
- `menu/view.rs` — dispatch `Stat(m)` to `stats::view`.
- `Cargo.toml` — add `nvml-wrapper`; `nix` if needed for `getifaddrs`.

## Cadence & sizing

- Both tiers tick ~1s — tier-1 cheap aggregates always, tier-2 detail only while
  a panel is open. History window: ~60 samples ≈ 60s.
- Dropdown card width ~320px, same chrome as the calendar.

## Error handling / edge cases

- First tick has no prior sample → rates/percentages show `—` or `0` until the
  second sample.
- Missing `/proc` fields → treated as zero, never panics.
- NVML init failure → GPU indicator hidden (and its panel unavailable).
- No default route → NET shows `0` / hides the interface line.
- Per-PID scans tolerate races (PIDs vanishing mid-scan) — skip and continue.

## Testing

Pure logic is unit-tested against fixtures; views are verified manually.

- Parse fixtures: `/proc/stat` (aggregate + per-core), `/proc/meminfo`,
  `/proc/net/dev`, `/proc/loadavg` → structs.
- Rate / percentage computation from two consecutive snapshots (idle-delta CPU
  %, byte-delta net rate).
- Ring-buffer push + windowing.
- Threshold → color mapping (neutral / amber / red boundaries).
- Memory segment math (used / cache / free sum to total).

## Out of scope (future)

- **Disk usage** indicator (the dropped fifth metric).
- **Per-app network** in the NET dropdown — needs eBPF/`nethogs`-style
  accounting on Linux; deferred. The NET dropdown ships without a top-processes
  list.
- **Configurability** — which indicators are shown / their order. Ships with the
  fixed `CPU GPU MEM NET` set; per-indicator toggles are a later addition.
