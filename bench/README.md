# Browser engine bench harness

Two small tools that drive `sola-browser-{wpe,cef}` runs and produce
a markdown comparison. Not a binary, not in cargo — just shell + python.

## Quick start

```sh
# from the workspace root
cargo make build sola-browser-wpe
cargo make build sola-browser-cef

# 30-second runs, same URL on both engines
bench/run-bench.sh wpe https://slate.auto 30 docs/notes/data/wpe-slate
bench/run-bench.sh cef https://slate.auto 30 docs/notes/data/cef-slate

# Render the report
bench/summarize.py docs/notes/data/wpe-slate docs/notes/data/cef-slate \
    -o docs/notes/2026-05-21-wpe-vs-cef-bench.md
```

## What it measures

Each run produces four files in its output directory:

| file        | content                                                                              |
| ----------- | ------------------------------------------------------------------------------------ |
| `procs.csv` | per-second sample of every process in the tree (`t_s,pid,ppid,cpu_pct,rss_kib,comm`) |
| `gpu.csv`   | per-second `nvidia-smi` utilization + memory (whole-GPU, not per-process)            |
| `fps.csv`   | shader-thread FPS counter scraped from `/opt/sola/log/sola.log`                      |
| `meta.json` | engine, URL, duration, host info                                                     |
| `app.log`   | the binary's own stderr/stdout for post-mortem                                       |

`summarize.py` rolls those into min / median / max per metric and
emits a markdown table per run.

## Caveats

- **CPU%** comes from `ps -o %cpu`, which is *averaged since the
  process started*. It's a useful "is one engine roughly 2x the
  other" signal but is not interval CPU. For interval CPU you'd
  read `/proc/<pid>/stat` `utime+stime` deltas — a future
  refinement.
- **GPU%** is whole-GPU utilization, not per-process. NVIDIA
  doesn't expose per-process GPU% via `nvidia-smi` on consumer
  cards. Close the browser, baseline GPU%, then run; the delta
  is roughly the engine's contribution.
- **FPS** is the shader-thread import rate. Chromium only
  re-paints on actual change (CSS animation, video, JS draw),
  so a static page can show 1–2 fps on CEF while WPE shows
  ~60 fps because WebKit re-emits frames more eagerly. Pick
  test URLs accordingly.
- **First run on a fresh boot is slower** for both engines
  (cold caches, JIT warmup). Throw away the first run if you
  care about steady-state.
- The harness `pkill`'s any leftover sola-browser-* before
  launching so previous sessions don't contaminate samples.
  Don't run this while you're using one of the browsers for
  something else.

## What's missing

- **Initial-paint latency** — would need to instrument
  `WpeEngine::spawn → first frame` and
  `CefEngine::spawn → first on_paint`. The shape would be
  one number per cold-launch run.
- **JS perf** — Speedometer / JetStream / Octane scores
  inside the rendered page, scraped via DevTools or page-side
  JS reporting through cefQuery / WebKit JSC bindings.
- **Per-process GPU** — would need `nvidia-smi pmon -c <N>
  -i 0 -s mu` and parsing.
- **Multiple iterations + variance** — currently one run per
  invocation. A wrapper that runs N iterations and reports
  variance across them is a small follow-up.

See `docs/specs/2026-05-21-sola-browser-cef-port-and-benchmark.md`
for the plan this implements.
