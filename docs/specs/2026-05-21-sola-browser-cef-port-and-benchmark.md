# sola-browser-cef port + WPE/CEF performance comparison

> **Goal:** Stand up a CEF-backed sibling of `sola-browser-wpe` so we
> can measure CPU / GPU / RAM head-to-head against the WPE path on
> the same hardware, same iced chrome, same dma-buf import pipeline.

## Why

`sola-browser-wpe` is the phase-0 spike. We picked WPE because it
seemed lightest and is what `sola-kit` was already integrating for
the future Sola app stack. With phase-0c green, the cost of porting
to CEF is small (most of the crate is engine-agnostic) and the data
we'd get is decisive: we'll be measuring Chromium / Blink / V8 vs
WebKit / JSC under identical conditions, with the same iced chrome
and the same wgpu+modifier sampling path on both sides.

Maintainability, binary size, and license footprint matter too, but
only as tiebreakers if perf is close.

## Architecture

```
+---------------------+        +---------------------+
| sola-browser-wpe    |        | sola-browser-cef    |
| ─────────────────── |        | ─────────────────── |
| iced chrome   <─────┼────────┼─────> iced chrome   |  shared by copy
| shader::Program     |        | shader::Program     |  (no abstraction
| wgpu_import (modif) |        | wgpu_import (modif) |   yet — measure
|                     |        |                     |   first)
| wpe.rs              |        | cef.rs              |  ← only divergence
|   WPEDisplay        |        |   CefBrowser (OSR)  |
|   on_buffer_render  |        |   on_accelerated_   |
|                     |        |     paint           |
+─────────┬───────────+        +──────────┬──────────+
          │                               │
          │       same WpeFrame-shaped    │
          │       (fd, w, h, fmt, mod,    │
          │        stride, offset)        │
          ▼                               ▼
   +─────────────────────────────────────────────+
   |  wgpu-hal-patched (VK_EXT_image_drm_format_  |
   |  modifier on Vulkan device)                  |
   +─────────────────────────────────────────────+
```

Both crates stay workspace-`exclude`d for the same iced /
`wayland-sys` dlopen reason that already excludes
`sola-browser-wpe` and `sola-monitor-iced`.

**Reuse strategy:** copy-by-value, not abstract-and-share. `shader.rs`,
`wgpu_import.rs`, and `main.rs` are nearly identical; the engine
boundary differs enough between WPE and CEF that pulling them into a
shared `sola-browser-core` crate now would force premature design
decisions. After the benchmark, if both stick around, refactor.

## Reference: where the bits live

- **CEF download + patchelf + Resources symlink:** `sola-make`'s
  `install-cef` subcommand. One-time per machine + per-version bump.
- **CEF OSR integration that already works in this repo:**
  `crates/sola-kit/src/cef/browser.rs` and `cef/handlers.rs` —
  `on_accelerated_paint`, dma-buf plane info, etc.
- **`cef` crate name-deltas vs the C API:** `crates/sola-kit/CLAUDE.md`
  (or the project CLAUDE.md's "Binding name deltas vs the design spec"
  block).

## Phase A — Scaffold

- Create `crates/sola-browser-cef/` with:
  - `Cargo.toml` — copy of `sola-browser-wpe/Cargo.toml`, swap WPE
    pkg-config deps for `cef = "147.1.0"` (no `accelerated_osr`
    feature — we go through `wgpu-hal-patched`, not CEF's helper).
    Keep the `[patch.crates-io] wgpu-hal = { path = "../wgpu-hal-patched" }`.
  - `build.rs` — copy of `sola-kit/build.rs` (CEF cache discovery +
    link directives). No bindgen, no pkg-config WPE modules.
  - `shell.nix` — point at the workspace dev shell that has the
    NVIDIA / Wayland deps. `sola-kit` builds without nix-shell so
    we likely don't need WPE's shell; just inherit the default.
  - `src/main.rs` — copy from `sola-browser-wpe`, replace
    `WpeEngine` with `CefEngine` and drop the WPE-specific env
    var comments.
  - `src/shader.rs` — copy verbatim; `Cmd` / `FrameSlot` /
    `WpePrimitive` re-skinned as `CefPrimitive` for clarity but
    structurally identical.
  - `src/wgpu_import.rs` — copy verbatim. Same `DmabufMetadata`,
    same modifier-aware vkCreateImage path, same `B8G8R8A8_SRGB`.
  - `src/cef.rs` — new. Mirrors `wpe.rs`'s public surface:
    `CefEngine::spawn(url, w, h) → Self`, `cmd_sender()`,
    `frames()`, `shutdown()`. Internals talk to CEF via the OSR
    pattern from `sola-kit::cef::browser`.
  - `src/lib.rs` — re-export `shader`, `wgpu_import`, `cef`.
- Add `crates/sola-browser-cef` to the workspace `exclude` list.
- `cargo make build sola-browser-cef` succeeds (binary may
  `todo!()` inside `cef.rs`; that's fine for scaffold).

## Phase B — Minimal CEF browser → DMA-BUF frames

This is the actual port work. Modeled directly on
`sola-kit/src/cef/browser.rs`.

- CEF process lifecycle:
  - `cef::execute_process(args, None, None)` early in `main` — if
    return is `>= 0`, we're a CEF subprocess (renderer / GPU /
    network), exit immediately.
  - `cef::initialize` with `Settings` configured for OSR (no
    sandbox on NixOS, multi-threaded message loop disabled —
    we drive the loop ourselves on the engine thread).
  - `cef::shutdown` on engine drop.
- Off-screen browser:
  - `WindowInfo` with `windowless_rendering_enabled = true` and
    `shared_texture_enabled = true` (this is what enables
    `on_accelerated_paint`).
  - `BrowserSettings` with default values.
  - `browser_host_create_browser_sync(window_info, client, url,
    settings, None, None)`.
- `RenderHandler` (via `wrap_client!`):
  - `view_rect(browser, rect)` — out-param the current size from
    `CefEngine::current_size` (atomic / mutex).
  - `on_accelerated_paint(browser, paint_element_type, dirty_rects,
    info)` — pull `info.shared_texture_handle` (Linux: a dma-buf
    fd + plane layout array), dup the fd, build a `CefFrame` with
    the same shape as `WpeFrame`, send into the frame channel.
  - `on_paint` (software path) — ignore. If
    `on_accelerated_paint` ever silently falls back to it, log
    loudly and abort — the comparison is invalid without GPU
    rendering on both sides.
- Resize:
  - `Cmd::Resize { width, height }` updates `current_size`, then
    calls `browser.host().was_resized()` so CEF re-renders at the
    new size.
- Same `FrameSlot::last_size` debounce on the iced side as WPE.

## Phase C — Run both with the same URL and confirm visual parity

- Both binaries load `https://slate.auto`, render in a borderless
  iced window, resize cleanly when the wm zones change them.
- Visual diff: side-by-side screenshots at the same window size.
  Acceptable differences: subpixel font rendering, scroll
  positions. Unacceptable: blown-out colors, missing assets,
  blank regions.
- This is the gate before benchmarking. If CEF colors look
  desaturated, suspect the same sRGB/UNORM mismatch we hit in
  WPE — fix in `wgpu_import.rs` (or, if CEF's `info.format`
  reports `BGRA_8888` vs `RGBA_8888`, branch the vk_format).

## Phase D — Perf comparison harness

A small `bench/` directory with scripts that orchestrate runs and
collect numbers. Not a binary, not in the cargo workspace — just
shell + python that runs from a checkout.

### Measurements

- **Per-process CPU & RSS** — `ps -p <pid> -o %cpu,rss` polled at
  1 Hz, summed across the main process and CEF / WPE child
  processes (renderer, GPU, network helpers). Use process tree
  walking from the parent pid.
- **GPU compute / memory** — `nvidia-smi --query-gpu=utilization.gpu,memory.used --format=csv,noheader -lms 1000`
  during the run. We don't have per-process GPU breakdown without
  `nvidia-smi pmon`, which on Linux gives us
  `nvidia-smi pmon -c <N> -i 0 -s mu` (gpu mem + util per pid).
- **Frame rate / wall-clock latency** — add a counter to
  `shader::Primitive::prepare` that logs every 60 frames with
  a wall-clock delta. Same in both crates.
- **Steady-state vs cold start** — record (a) startup → first
  frame and (b) a 30s steady-state window after.

### Test pages

In order of complexity (script will run all, recording each):
1. `about:blank` — measure pure engine overhead.
2. `https://example.com` — static, minimal CSS.
3. `https://slate.auto` — moderate, real-world.
4. A WebGL demo (e.g. `https://webglsamples.org/aquarium/aquarium.html`) — exercises GPU.
5. JS-heavy SPA (TBD — pick one that doesn't change between runs).

### Output

- A single markdown report `docs/notes/2026-05-21-wpe-vs-cef-bench.md`
  with one table per test page: rows = engine, columns =
  startup ms / steady CPU% / steady RSS MB / GPU util % / GPU mem MB / fps.
- Raw CSVs alongside in `docs/notes/data/`.

## Open questions

- **CEF OSR on NVIDIA proprietary:** does
  `on_accelerated_paint` actually get called, or does CEF fall
  back to software rendering when it can't get GBM? sola-kit
  has it working in some configuration — confirm we hit the
  accelerated path on this box before declaring the port done.
  If CEF software-renders while WPE GPU-renders, the comparison
  is meaningless.
- **Modifier delta:** CEF on Linux historically used LINEAR
  modifiers via Mesa; on NVIDIA we'll likely get the same
  block-linear we get from WPE (or worse — software fallback).
  Plan B if CEF can't emit GPU dma-bufs on NVIDIA: defer the
  comparison until we have a Mesa machine to test both on.
- **CEF process model:** CEF spawns 3–5 helper processes
  (browser, renderer, GPU, network, optionally storage). The
  comparison must include them all in CPU / RSS totals.
- **Shared chrome later?** Out of scope for this plan. After
  the benchmark we decide: keep one, keep both, or extract a
  shared `sola-browser-core`. Don't do that work now.
