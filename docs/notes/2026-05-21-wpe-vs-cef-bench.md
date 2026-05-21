# WPE vs CEF — benchmark + framerate investigation

> 30-second runs on four URLs, same iced chrome, same hardware
> (NVIDIA proprietary, RTX 3090 Ti, 120 Hz display), clean GPU
> baseline. Raw CSVs in `docs/notes/data/2026-05-21b_*`. Harness:
> `bench/run-bench.sh`, `bench/summarize.py`.

> **Earlier draft retracted.** The first version of this report
> showed every URL with near-identical numbers and concluded WPE
> always emitted ~110 fps. That was a bug in the binaries —
> the URL was hardcoded to `slate.auto` and the harness's URL
> argument was being ignored. Fixed by adding argv parsing
> (`std::env::args().nth(1)`); these numbers come from honest
> per-URL runs.

## Headline numbers (medians across 30 s)

| URL                | engine | tree CPU% | RSS MiB | shader FPS  | GPU util% |
| ------------------ | ------ | --------: | ------: | ----------: | --------: |
| about:blank        | WPE    |   **6.2** |     451 |       0     |        24 |
|                    | CEF    |       7.4 |    1078 |       0     |        23 |
| slate.auto         | WPE    |     54.7  |    1208 |     111     |        33 |
|                    | CEF    |   **25.5**|    1659 |       0.8 ⁱ |        24 |
| github.com         | WPE    |   **6.4** |     511 |       0     |        23 |
|                    | CEF    |      52.0 |    1626 |      30     |        35 |
| WebGL Aquarium     | WPE    |     38.4  |     805 |      62     |        33 |
|                    | CEF    |      41.0 |    1405 |      30     |        34 |

ⁱ One sample over 30 s. CEF only paints on actual change; with
slate.auto mostly static after first paint, on_paint went silent.

## Big-picture findings

- **WPE does not "always emit frames"** — verified by
  `about:blank` and `github.com` both sitting at **0 shader fps,
  ~6 % CPU**. The earlier (URL-bug) draft showed otherwise; that
  was wrong.
- **CEF caps OSR at 30 fps by default** (`windowless_frame_rate`).
  Visible in `webgl aquarium` and `github.com` topping out at
  exactly 30. Even on a 60 fps animation, CEF will not exceed
  30 paints/s until we override this.
- **For genuine animation, WPE is roughly 2× more
  frame-efficient than CEF on this hardware.** WebGL Aquarium:
  WPE 38 % CPU at 62 fps (~0.6 % CPU per fps) vs CEF 41 % CPU
  at 30 fps (~1.4 % CPU per fps). This is dominated by CEF's
  CPU OSR readback — `on_paint` memcpys a full 1434×2132 BGRA
  buffer per frame, which is ~12 MiB at 30 Hz = ~360 MiB/s
  through main memory.
- **WPE wins memory uniformly** by 400–600 MiB. Same shape as
  before — CEF's helper-process model has more per-process
  fixed cost.

## Per-question answers

### Q1: How Chromium/CEF sends frames

The `RenderHandler::OnPaint` (or `OnAcceleratedPaint`) callback
delivers a **full-viewport BGRA buffer every time**. There are no
deltas at the callback boundary — even a 1-pixel cursor blink
emits the whole frame.

What *is* delta-aware is the *production* cost inside Chromium:

- Viz tracks damage per **tile** (typical tile size 256×256).
- The render pass only re-rasterizes dirty tiles into the GPU
  texture, then composites the whole texture out.
- On the CEF CPU-OSR path (what we use on NVIDIA), that GPU
  texture is then read back into a CPU BGRA buffer and handed to
  us in `OnPaint`. The readback is full-viewport regardless of
  damage.

The cap is `WindowInfo::windowless_frame_rate`, default **30**.
Even on a 60 fps animated page, CEF will not deliver more than
30 paints/s through OSR unless this is bumped. We left it at
default; the WebGL Aquarium and github.com both topping out at
exactly 30 fps confirms it's the binding limit.

### Q2: WebGL Aquarium at 8 fps — what did that mean?

That number came from the URL-bug run — every binary was actually
loading slate.auto, not the aquarium. With the URL bug fixed:

- WebGL Aquarium in **CEF**: 30 fps. That's the
  `windowless_frame_rate` ceiling, not an aquarium-specific issue.
- WebGL Aquarium in **WPE**: 62 fps. Close to 60, slightly above
  because the headless backend ticks on monotonic time with a
  small per-frame slack rather than a hard 16.667 ms cap.

Both are animating; CEF is just capped at half.

### Q3: Why ~110 fps on a 120 Hz display?

Almost certainly the iced/wgpu redraw path being vsync-bound on
your 120 Hz monitor, not anything inside WPE.

- WPE WebProcess produces frames on its own internal compositor
  schedule (no precise vsync; tick driven by GLib timer).
- Our `frame_stream` subscription puts each frame in
  `slot.pending` and posts a `Msg::NewFrame`. Multiple new frames
  arriving before iced has redrawn are collapsed — iced batches
  redraws to **one per surface present**.
- wgpu's surface present mode is iced's default = `Fifo` (vsync).
- On a 120 Hz monitor, that's ~120 Hz max with overhead
  → ~110–117 fps in steady state, exactly what we measure.

So: on a 60 Hz display the same WPE binary would show ~58 fps on
slate.auto, not 111. The cap is your display, not WPE.

What WPE *does* contribute is producing frames at all on a
mostly-static page — slate.auto has continuous animation
(background gradient or similar) that triggers a new buffer
every WebProcess tick. CEF deduplicates this away to ~1 fps
because Chromium's compositor recognizes "same content, no
damage" and skips the paint.

### So is there a WPE framerate problem at all?

Less than the earlier (wrong) writeup suggested. There's still
**one** real asymmetry on animated pages: WPE keeps the iced
redraw loop saturated at vsync because it emits a new buffer for
every WebProcess compositor tick, even if the visible delta is
imperceptible. CEF's deduplication saves work that WPE doesn't.
On slate.auto specifically that's the 54 % vs 25 % gap.

Mitigations from the earlier draft still apply if you want WPE
to match CEF's "only redraw on visible change" behaviour:

1. **Client-side throttle in `shader::prepare`** (~10 LoC). Skip
   the wgpu work if a frame arrives <16.67 ms after the previous,
   but still `Cmd::Release` so WPE doesn't stall. Halves *our*
   redraw cost, doesn't help WebProcess.
2. **Delayed `Cmd::Release`** (~20 LoC). Hold buffers ~16 ms
   before releasing. WPE's buffer pool fills, WebProcess
   backpressures. Saves WPE-side CPU too.
3. **Content-hash skip** — keep a SHA of the last imported
   frame's pixels, skip import if identical. Closest to CEF's
   behaviour but cost-shifts the work to the hash. Likely not
   worth it.

The previous draft also pointed at a suspected bug in
`WPEViewHeadless.cpp`'s rate limiter (`lastFrameTime = now`
assignment outside the throttle branch). That code is still
fishy, but it's *not* the source of the high fps numbers we
saw — those were the URL bug.

## Memory tradeoff still holds

CEF runs ~9 processes, WPE ~8. CEF's 400–600 MiB premium across
URLs reflects per-process V8/Skia/sandbox fixed cost. For
multi-window/multi-tab use, CEF's memory will grow faster than
WPE's because more state is per-process in Chromium.

## Visual quality

Side-by-side at the same viewport size: WPE text reads slightly
softer, CEF crisper. Same Skia raster backend, different glyph
hinting / subpixel positioning. Not a defect either way.

## Caveats baked in

- CEF is on **CPU OSR** (`on_paint` → `queue.write_texture`)
  because NVIDIA proprietary can't drive `on_accelerated_paint`.
  On a Mesa stack (Intel / AMD / NVK), CEF's CPU% on animated
  pages should drop dramatically because the GPU process would
  render directly into a dma-buf instead of reading back to a
  CPU buffer.
- WPE is on **GPU dma-buf** (zero-copy modifier-aware Vulkan
  import) — best path on NVIDIA.
- `ps -o %cpu` is averaged since process start, not interval.
  Good for "2× delta" calls, less so for "5% delta" calls.
- One 30 s run per (engine, URL). Variance not measured.
- `windowless_frame_rate=30` left at default — CEF would be
  closer to WPE on animated pages if bumped to 60 or 120.

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
  bench/run-bench.sh wpe "$url" 30 "docs/notes/data/2026-05-21b_wpe-${site}"
  bench/run-bench.sh cef "$url" 30 "docs/notes/data/2026-05-21b_cef-${site}"
done
```

## Next experiments

- **Bump CEF `windowless_frame_rate` to 60 or 120** — would let CEF
  match WPE's frame rate on the WebGL aquarium and is the
  fairest setting for a paint-throughput comparison. One line
  in `cef.rs`'s `BrowserSettings`.
- **Run Speedometer 3** — pure JS perf (V8 vs JSC), no rendering
  bias. Useful even with the OSR caps in place since the cost is
  in the renderer process.
- **N=3 iterations per URL** to filter cold-cache noise.
- **Run on a 60 Hz display** to confirm the 110 fps shader rate
  is vsync-bound, not WPE-bound.
