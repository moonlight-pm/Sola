# WPE vs CEF — benchmark + WPE framerate investigation

> 30-second runs of `sola-browser-{wpe,cef}` across four URLs. Same
> iced chrome, same hardware (NVIDIA proprietary, RTX 3090 Ti),
> clean GPU baseline (no other GPU workload running). Raw CSVs in
> `docs/notes/data/2026-05-21_*`. Harness: `bench/run-bench.sh`,
> `bench/summarize.py`.

## Headline numbers (medians across 30 s)

| URL              | engine | tree CPU% | tree RSS (MiB) | shader FPS  | GPU util% | GPU mem MiB |
| ---------------- | ------ | --------: | -------------: | ----------: | --------: | ----------: |
| about:blank      | WPE    |    54.2   |     1214.7     |    110.7    |    32.5   |    1.4 GiB¹ |
|                  | CEF    |    26.3   |     1666.9     |     8.0 ²   |    27.0   |    1.4 GiB¹ |
| slate.auto       | WPE    |    56.9   |     1234.6     |    110.9    |    32.0   |    1.4 GiB¹ |
|                  | CEF    |    27.0   |     1667.7     |     0.7 ²   |    22.0   |    1.4 GiB¹ |
| github.com       | WPE    |    54.9   |     1211.6     |    111.1    |    33.0   |    1.4 GiB¹ |
|                  | CEF    |    26.0   |     1669.7     |     9.5 ²   |    23.0   |    1.4 GiB¹ |
| WebGL Aquarium ³ | WPE    |    53.6   |     1217.4     |    110.0    |    33.0   |    1.4 GiB¹ |
|                  | CEF    |    26.2   |     1663.9     |     8.0 ²   |    24.0   |    1.4 GiB¹ |

¹ GPU memory is whole-card (NVIDIA doesn't expose per-process via nvidia-smi). System baseline before launch was ~1.3 GiB (sola-shell, compositor, etc.) so the per-engine contribution is ~50–100 MiB, well within noise.

² CEF FPS samples are sparse because Chromium only repaints on actual change. Most runs registered exactly one fps sample (a single second with a visible paint).

³ webglsamples.org/aquarium — JS+WebGL canvas. CEF's CPU stayed flat at 26 %, suggesting the WebGL canvas isn't actually driving extra on_paint events through CEF's CPU-OSR pipeline at the rate the page expects (consistent with CEF's default `windowless_frame_rate = 30` cap, but still surprising that we only see ~8 imports/s).

## Read in one line

CEF wins steady-state CPU by ~2×. WPE wins memory by ~25 %. **The CPU gap is dominated by WPE's pipeline producing 110+ frames/sec even on `about:blank`** — once that's mitigated the CPU comparison would tighten significantly.

## WPE framerate — what's going on

WPE's headless backend re-emits frames at the WebProcess's natural render rate (110+ fps here) regardless of whether anything on the page changed. CEF, by contrast, only emits on actual visual change. That asymmetry shows up directly in CPU% — most of WPE's overhead is doing work the page didn't ask for.

### Why the env-var fix doesn't apply

WebKit ships `WEBKIT_DISPLAY_REFRESH_THROTTLE_FPS=N` for exactly this kind of throttling. It's read by `Source/WebKit/UIProcess/glib/DisplayLinkGLib.cpp` and gates the *DisplayLink* on top of a real DRM VBlank source. With our `WPEDisplayHeadless`, there is no DRM CRTC, no vblank monitor, no DisplayLink — so the env var is silently ignored. Verified empirically: setting `WEBKIT_DISPLAY_REFRESH_THROTTLE_FPS=30` gave us 110 fps anyway.

### What the headless backend actually does

`Source/WebKit/WPEPlatform/wpe/headless/WPEViewHeadless.cpp` (`wpewebkit-2.52.3` branch, identical to `main`) drives frame emission with a GSource timer. The intent reads like a 60 fps throttle:

```cpp
static gboolean wpeViewHeadlessRenderBuffer(WPEView* view, WPEBuffer* buffer, ...) {
    auto* priv = WPE_VIEW_HEADLESS(view)->priv;
    priv->pendingBuffer = buffer;
    auto now = g_get_monotonic_time();
    if (!priv->lastFrameTime)
        priv->lastFrameTime = now;
    auto next = priv->lastFrameTime + (G_USEC_PER_SEC / 60); // ← 16.667 ms slot
    priv->lastFrameTime = now;                                // ← BUG-ish (see below)
    if (next <= now)
        g_source_set_ready_time(priv->frameSource.get(), 0);
    else
        g_source_set_ready_time(priv->frameSource.get(), next);
    return TRUE;
}
```

`next` is intended to be 16.667 ms after the previous frame, but `lastFrameTime` is reassigned to `now` (the time of the current `render_buffer` *call*, not the time of the last frame *emission*). When WebProcess submits at 8 ms intervals (120 fps), the third call computes `next = 8 ms + 16.667 ms = 24.667 ms`, which is in the past by the time the fourth call lands at 24 ms — so `ready_time = 0` (fire immediately). The timer effectively becomes "single-frame coalescing" with no actual rate cap.

Net effect: WPE headless emits frames at the WebProcess's submission rate. Our WebProcess is rendering at ~110 fps and we see ~110 fps out the door.

## Mitigations, ranked by effort × impact

1. **Live with it (easiest).** Document the asymmetry in benchmark reports and move on. Pro: zero changes. Con: WPE always looks "worse" on CPU.

2. **Client-side throttle in `shader::prepare`** (~10 lines, in this repo). Track `last_frame_uploaded_at`; if a new frame arrives <16.667 ms after the previous, skip the wgpu import + bind-group rebuild but *still* call `Cmd::Release` so WPE's buffer pool doesn't stall. Saves iced + wgpu CPU but not WPE-WebProcess CPU. Halves our portion of the cost.

3. **Backpressure via delayed release** (~20 lines, in this repo). Queue the `Cmd::Release` with a 16 ms timer instead of sending it on every new frame. WPE's buffer pool fills, `render_buffer` blocks waiting for a free slot, WebProcess throttles itself. Saves WPE-WebProcess CPU too — but depends on pool size (WPE Platform headless uses 3 buffers by default, so this should work).

4. **Patch `WPEViewHeadless.cpp`** (one-line fix, upstream WebKit). Move `priv->lastFrameTime = now;` inside the `if (next <= now)` branch (or update it to `next` rather than `now`). This is the proper fix — it makes the 60 fps cap actually work. Worth filing as a WebKit bug at <https://bugs.webkit.org/>; same code path serves any other consumer of `WPEViewHeadless` so they'd want this too.

5. **Substitute our own toplevel/view** — would require subclassing or `wrap_` macros that the WPE Platform API doesn't currently expose for the headless backend. Bigger lift than #4 and downstream of fixing upstream.

Recommendation: do **#3 (delayed release)** now if you want a fair benchmark today, **#4 (upstream patch)** if this becomes a sola-shipped thing. Avoid #2 alone because it leaves the WebProcess CPU cost on the table.

## Memory delta — what's CEF spending it on

CEF runs ~9 processes; WPE runs ~8. CEF's helper processes each carry V8 isolates, Skia per-process state, and sandbox overhead. Estimated split of CEF's ~1.65 GB:

- Main browser process: ~250 MiB
- GPU process: ~150 MiB (CPU OSR path on this hardware — would be smaller on Mesa)
- Renderer (Blink): ~700 MiB on slate.auto (page-content-dependent)
- Network, utility, zygotes: ~550 MiB combined

The WPE comparison's 1.23 GB is mostly UIProcess + WPEWebProcess + WPENetworkProcess, fewer process boundaries.

For multi-tab/multi-window workloads CEF's per-window memory cost rises linearly; WPE's doesn't grow as fast because more state is shared in the UIProcess.

## Visual quality

Side-by-side at the same window size on slate.auto: WPE text reads slightly softer, CEF crisper. Same Skia raster backend in both, different glyph hinting / subpixel positioning / CSS-pixel rounding. Not a defect either way.

## Caveats baked into all numbers

- CEF is on **CPU OSR** (`on_paint` → `queue.write_texture`) because NVIDIA proprietary can't drive `on_accelerated_paint`. On Mesa stacks (Intel / AMD / NVK), CEF's CPU% should drop further because the GPU process renders directly into a dma-buf.
- WPE is on **GPU dma-buf** (zero-copy import via modifier-aware Vulkan) — best path it has on NVIDIA.
- `ps -o %cpu` is averaged since process start, not interval. Good for "2× delta" calls, not for "5% delta" calls.
- Single 30-second run per (engine, URL). Variance not measured; should add an N-iteration wrapper.

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
  bench/run-bench.sh wpe "$url" 30 "docs/notes/data/2026-05-21_wpe-${site}"
  bench/run-bench.sh cef "$url" 30 "docs/notes/data/2026-05-21_cef-${site}"
done
```

## Next experiments worth running

- **Mitigation #3 (delayed release)** applied to sola-browser-wpe, re-run the same matrix, compare to current WPE numbers. Expected: CPU drops from ~55 % toward ~30 %.
- **Speedometer 3** on both — pure JS perf (V8 vs JSC) with no rendering bias. The harness as written would work; just point it at `https://browserbench.org/Speedometer3.0/` and a 120 s duration.
- **N=3 iterations** per URL with cold-cache resets between runs, to filter out first-launch noise.
- **A real WebGL workload at known FPS** — the aquarium page in CEF only delivered ~8 paints/s in our run, which suggests CEF's `windowless_frame_rate` (default 30) or some other cap is gating it. If we want a true GPU-saturation comparison we may need to raise that.
