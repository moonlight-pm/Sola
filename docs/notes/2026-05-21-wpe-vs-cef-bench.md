# WPE vs CEF — benchmark + framerate investigation

## Decision (2026-05-21): **WPE**

Sola ships `sola-browser` on WPE. The structural CPU advantage on
NVIDIA proprietary (CEF can't use `on_accelerated_paint`, has to
pay ~720 MiB/s of host-memory readback per frame on animated
content) is decisive for our hardware target. Memory is also
~400–700 MiB cheaper across the board.

`sola-browser-cef` is kept in-tree as an archive — workspace-
excluded, builds on demand, but no further feature work. Revisit
only if we (a) start running on Mesa hosts where CEF's GPU
transport works or (b) find another decisive reason to switch.

---

> 30-second runs on four URLs, same iced chrome, same hardware
> (NVIDIA proprietary, RTX 3090 Ti, 120 Hz display), clean GPU
> baseline. CEF's `windowless_frame_rate` raised from default 30
> to **60** so animated pages aren't gated below WPE's natural
> rate. Raw CSVs in `docs/notes/data/2026-05-21c_*`. Harness:
> `bench/run-bench.sh`, `bench/summarize.py`.

> **Two earlier drafts retracted.** The first run had a URL bug
> (every binary loaded slate.auto regardless of arg). The second
> run was honest per-URL but had CEF at its default 30 fps cap,
> making animated comparisons unfair. This third pass fixes both.

## Headline numbers (medians across 30 s)

| URL                | engine | tree CPU% | RSS MiB |     fps | CPU/fps |
| ------------------ | ------ | --------: | ------: | ------: | ------: |
| about:blank        | WPE    |       6.6 |     451 |    0    | —       |
|                    | CEF    |       7.5 |    1081 |    0    | —       |
| slate.auto         | WPE    |      52.9 |    1228 |  110    | 0.48 %  |
|                    | CEF    |  **27.6** |    1707 |   17    | 1.62 %  |
| github.com         | WPE    |      77.8 |    1120 |  101    | 0.77 %  |
|                    | CEF    |      69.6 |    1528 |   60 ¹  | 1.16 %  |
| WebGL Aquarium     | WPE    |  **37.9** |     726 |   62    | **0.61 %** |
|                    | CEF    |      62.8 |    1424 |   60 ¹  | **1.05 %** |

¹ CEF hitting the `windowless_frame_rate = 60` cap exactly. We
raised this from the C-API default of 30; CEF won't go past 60
without a different OSR transport.

## Read in one line

For genuinely animated workloads, **WPE is ~1.7× more
frame-efficient than CEF on this hardware**, driven almost
entirely by CEF's CPU OSR readback. For mostly-static pages,
CEF's frame-deduplication wins large (slate.auto: 27 % vs 53 %
CPU). Memory is consistently WPE-favorable by 400–700 MiB.

## The fair comparison: WebGL Aquarium

Both engines run a continuous animation at ~60 fps with no
deduplication available. CEF needs **70 % more CPU per frame**
to keep up:

- **WPE**: 38 % CPU at 62 fps → 0.61 % CPU per fps
- **CEF**: 63 % CPU at 60 fps → 1.05 % CPU per fps

Where CEF's extra CPU goes:

- Every frame, CEF's GPU process composites into a GPU texture,
  then **reads back the full 1434×2132 BGRA buffer (≈12 MiB)
  to host memory** because `shared_texture_enabled = 0` on
  NVIDIA proprietary.
- That handoff appears in `OnPaint` as a `*const u8` we then
  `queue.write_texture` back to wgpu — a second copy through
  CPU.
- 12 MiB × 60 fps = **~720 MiB/s sustained host memory
  bandwidth** for the readback path alone.
- WPE on the same workload hands us a dma-buf FD; we never
  touch the pixels with the CPU.

**This is the dominant cost.** On a Mesa stack (Intel / AMD /
NVK) CEF could use `on_accelerated_paint` and the readback
goes away — the CPU gap would likely close to within noise.
**On NVIDIA proprietary, WPE will keep winning per-frame
efficiency until CEF gains a working dma-buf path.**

## Per-question answers

### Q1: How does Chromium send frames?

`OnPaint` / `OnAcceleratedPaint` delivers a **full-viewport
BGRA buffer every paint**. No deltas at the callback boundary —
even a 1-pixel cursor blink emits the whole frame.

The damage tracking is *inside* the Viz compositor (per-tile
re-rasterization), but the consumer-facing surface is always
full-frame. OSR rate is hard-capped by
`WindowInfo::windowless_frame_rate`, default 30, ceiling 60.
With our bump to 60, both WebGL Aquarium and github.com top
out at exactly 60 — see numbers above.

### Q2: WebGL Aquarium at 8 fps — what did that mean?

The original 8-fps result was an artifact of the URL bug —
every binary was loading slate.auto. With the URL bug and the
30-fps cap both fixed, the aquarium runs at:

- **WPE: 62 fps** (close to 60, slack from the headless backend
  not enforcing a hard cap)
- **CEF: 60 fps** (the `windowless_frame_rate` ceiling)

Both engines are now animating the aquarium properly; CEF is
just structurally more expensive per frame on NVIDIA.

### Q3: Why ~110 fps on a 120 Hz display?

Almost certainly iced/wgpu's vsync pacing, not anything in WPE.

- WPE WebProcess produces frames on its own internal compositor
  schedule (GLib timer; no real vsync source on headless).
- Our `frame_stream` subscription puts each frame in
  `slot.pending` and posts `Msg::NewFrame`. Multiple new frames
  arriving before iced has redrawn are **collapsed** — iced
  batches redraws to one per surface present.
- wgpu's surface present mode is iced's default `Fifo` (vsync).
- On a 120 Hz monitor, that's ~120 Hz max with overhead
  → ~110 fps in steady state, exactly what we measure.

So: on a 60 Hz display the same WPE binary would show ~58 fps
on slate.auto, not 110. The cap is your display, not WPE.

What WPE *does* contribute is producing frames at all on a
mostly-static page — slate.auto has continuous animation
(background gradient or similar) that triggers a new buffer
every WebProcess tick. CEF's compositor recognizes "same
content, no damage" and skips the paint, batching it to ~1 fps
in our run.

## Memory tradeoff still holds

CEF runs ~9 processes, WPE ~8. CEF's 400–700 MiB premium across
URLs reflects per-process V8/Skia/sandbox fixed cost. For
multi-window/multi-tab use, CEF's memory will grow faster than
WPE's because more state is per-process in Chromium.

## Variance is real

The github.com row deserves a footnote: this run showed
**WPE at 78 % CPU / 101 fps**, but our previous run (same URL,
same hardware, default CEF rate so it wasn't directly
comparable) showed **WPE at 6 % / 0 fps**. github.com's render
behaviour is content-driven (notification badges, hover states,
animations triggered by visibility) and apparently variable
between sessions.

**Takeaway**: single-run benchmarks are not safe for any URL
with dynamic content. For final claims, run N=3 with cold-cache
resets and report variance.

## Mitigations from earlier drafts still apply

For animated pages where you want WPE to behave more like CEF's
deduplicating compositor:

1. **Client-side throttle in `shader::prepare`** (~10 LoC). Skip
   the wgpu work if a frame arrives <16.67 ms after the previous,
   still `Cmd::Release` so WPE doesn't stall.
2. **Delayed `Cmd::Release`** (~20 LoC). Hold buffers ~16 ms
   before releasing. WPE's buffer pool fills, WebProcess
   backpressures.
3. **Content-hash skip**. Hash the imported pixels, skip render
   on identical hash. Closest to CEF behaviour but cost-shifts
   to the hash.

None of these change the per-frame cost ratio on animated
content — that's the dma-buf vs CPU-readback structural issue.

## Visual quality

Side-by-side at the same viewport size: WPE text reads slightly
softer, CEF crisper. Same Skia raster backend, different glyph
hinting / subpixel positioning. Not a defect either way.

## Caveats baked in

- CEF is on **CPU OSR** (`on_paint` → `queue.write_texture`)
  because NVIDIA proprietary can't drive `on_accelerated_paint`.
  On Mesa, this analysis would need redoing because the readback
  cost evaporates.
- WPE is on **GPU dma-buf** (zero-copy modifier-aware Vulkan
  import) — best path on NVIDIA.
- `ps -o %cpu` is averaged since process start. Good for 2×
  deltas, less so for 5 %.
- One 30 s run per (engine, URL).
- `windowless_frame_rate` raised from default 30 to 60. CEF
  hard-caps at 60 in the C-API.

## Reproduce

```sh
cargo make build sola-browser-wpe
cargo make build sola-browser-cef

for site in blank slate github webgl; do
  case "$site" in
    blank)  url="about:blank" ;;
    slate)  url="https://slate.auto" ;;
    github) url="https://github.com" ;;
    webgl)  url="https://webglsamples.org/aquarium/aquarium.html" ;;
  esac
  bench/run-bench.sh wpe "$url" 30 "docs/notes/data/2026-05-21c_wpe-${site}"
  bench/run-bench.sh cef "$url" 30 "docs/notes/data/2026-05-21c_cef-${site}"
done
```

## Next experiments

- **N=3 iterations** with cold-cache resets to filter the
  github.com-style variance.
- **Speedometer 3** for pure JS perf (V8 vs JSC) where rendering
  cost is sidelined.
- **Mesa stack run** if/when this box gets NVIDIA Open + NVK.
  Would let us test CEF's `shared_texture_enabled = 1` and see
  if the per-frame gap closes.
- **`sola-browser-wpe` with delayed Cmd::Release** to confirm
  the upper-bound WPE CPU we'd get if we voluntarily throttled
  to 60 fps on a 120 Hz display.
