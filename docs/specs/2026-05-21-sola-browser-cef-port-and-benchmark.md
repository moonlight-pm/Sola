# sola-browser-cef port + WPE/CEF performance comparison

> **Status (2026-05-21): COMPLETE — outcome: stay on WPE.**
>
> The port shipped, the benchmark ran, and the comparison settled the
> question. WPE wins per-frame efficiency by ~70 % on animated workloads
> on NVIDIA proprietary, driven by CEF's inability to use
> `on_accelerated_paint` and the resulting ~720 MiB/s host-memory
> readback per paint. Memory was 400–700 MiB cheaper for WPE too.
> Full numbers + analysis: `docs/notes/2026-05-21-wpe-vs-cef-bench.md`.
>
> The `sola-browser-cef` crate is kept workspace-excluded as a reference
> implementation. Revisit if the host moves to Mesa (NVIDIA Open + NVK,
> Intel, AMD) where CEF can use the GPU transport.
>
> ---

> **Original goal:** Stand up a CEF-backed sibling of `sola-browser-wpe`
> so we can measure CPU / GPU / RAM head-to-head against the WPE path
> on the same hardware, same iced chrome, same dma-buf import pipeline.

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

## Phase B — Minimal CEF browser → CPU pixel frames

This is the actual port work. Modeled on `sola-kit/src/cef/browser.rs`
+ `cef/handlers.rs`, but using the CPU OSR transport rather than
dma-buf (see "Pre-implementation discovery" above).

- CEF process lifecycle (`cef.rs::CefEngine::dispatch_subprocess` +
  `spawn`):
  - `cef::sys::cef_api_hash(CEF_API_VERSION, 0)` at process top.
    Required by CEF 133+ before any other CEF call.
  - `cef::execute_process(args, app, null)` — returns `>= 0` for
    subprocesses (renderer / GPU / utility / zygote), `-1` for the
    browser process. The wrapper exits subprocesses with the
    returned code.
  - `cef::initialize(args, settings, app, null)` in the browser
    process with `Settings { no_sandbox: 1,
    windowless_rendering_enabled: 1, external_message_pump: 0,
    multi_threaded_message_loop: 0 }`. `framework_dir_path`,
    `resources_dir_path`, `locales_dir_path`,
    `browser_subprocess_path`, `root_cache_path` populated from
    the build-time `SOLA_BROWSER_CEF_DIR` env (mirror
    `sola-kit/src/cef/distribution.rs`).
  - `cef::run_message_loop()` blocks until quit; we run it on the
    engine worker thread so `iced::application().run()` keeps the
    main thread.
  - `cef::shutdown()` on quit.
- Off-screen browser:
  - `WindowInfo { windowless_rendering_enabled: 1,
    shared_texture_enabled: 0, external_begin_frame_enabled: 0 }`.
    `shared_texture_enabled = 0` is the CPU path — `on_paint`
    fires with a `*const u8` BGRA buffer per frame.
  - `BrowserSettings { background_color: 0xFFFFFFFF }` (opaque
    white default; web content paints on top).
  - `browser_host_create_browser_sync(window_info, client, url,
    settings, None, None)`.
- `RenderHandler` (`wrap_client!`-generated `CefClient`):
  - `view_rect(browser, rect)` — out-param the current size from
    a `Mutex<(u32,u32)>` shared with the resize command.
  - `on_paint(browser, paint_element_type, dirty_rects, buffer,
    width, height)` — `paint_element_type == PET_VIEW` only
    (ignore popup). Copy the buffer into a `Vec<u8>`
    (`width * height * 4` bytes, BGRA), wrap it in a `CefFrame`,
    send through the frame channel.
  - `on_accelerated_paint` — implemented as a panic'ing stub. If
    NVIDIA ever starts producing accelerated frames, we want to
    notice loudly rather than silently mis-render.
- Resize (`Cmd::Resize`):
  - Update the shared `Mutex<(u32,u32)>` so `view_rect` sees the
    new size on its next invocation.
  - `browser.host().was_resized()` from the CEF UI thread (post a
    task if we're not already on it).
- Frame upload path (`cpu_import.rs`, replacing `wgpu_import.rs`):
  - Maintain a single `wgpu::Texture` sized to the current frame
    dimensions. On size change, recreate; otherwise reuse.
  - `queue.write_texture` with the BGRA bytes each frame.
  - Format: `Bgra8UnormSrgb` (CEF emits sRGB-encoded BGRA, same
    as WPE; same washed-out trap if we use `Bgra8Unorm`).

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

## Pre-implementation discovery — CEF OSR on NVIDIA

**Answered before Phase B:** CEF's dma-buf OSR transport
(`on_accelerated_paint`) doesn't work on this NVIDIA proprietary box.
sola-kit's `cef::browser::Browser::new` documents this in detail
(lines 41-47) and intentionally uses `shared_texture_enabled = 0`
to fall back to CPU OSR (`on_paint`) with wl_shm memcpy. Root cause:
NVIDIA's libEGL doesn't expose the `EGL_MESA_*` extensions that
CEF's GPU process needs to allocate exportable dma-buf textures.
Re-enable conditions are in `docs/vault/Distribution.md` ("When to
revisit dma-buf"); the short list is "Mesa NVK or Intel/AMD".

**Impact on this port:**
- `sola-browser-cef` uses `on_paint` (CPU pixel buffer) instead of
  `on_accelerated_paint`. No dma-buf, no modifier handling.
- The shared `wgpu_import.rs` from sola-browser-wpe is *not*
  copied — we replace it with a `cpu_import.rs` that uploads
  pixel bytes to a `wgpu::Texture` via `queue.write_texture` each
  frame.
- The benchmark must report this asymmetry honestly. We are
  comparing the *best path each engine has on this hardware*:
  WPE = zero-copy GPU dma-buf with modifier-aware sampling,
  CEF = CPU memcpy upload. The expected CPU / bandwidth delta
  in favor of WPE is real and actionable — it tells us CEF needs
  FHS-shimmed Mesa libEGL (or Mesa NVK) to be GPU-competitive
  on this stack. If the user later switches the host to
  NVIDIA Open + Mesa NVK, flipping `shared_texture_enabled = 1`
  re-runs the comparison on equal footing.

## Open questions
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
