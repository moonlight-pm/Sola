# Browser Engine Unification — Design

> Status: **draft for review** · 2026-06-19
> Supersedes the two-parallel-crates arrangement described in
> `docs/specs/2026-05-21-sola-browser-cef-port-and-benchmark.md`.

## Goal

Stop maintaining two near-identical browser codebases. Today `sola-browser-wpe`
and `sola-browser-cef` duplicate ~1,000 lines of chrome (window, tabs, nav,
theme, bus, input) on top of ~1,000 lines of genuinely engine-specific code
each. We want **one shared chrome codebase**, with the engine bodies (which are
inherently different — WPEWebKit C API vs the `cef` crate) kept separate, plus a
single front-door command that picks an engine.

This is **Option B + an `exec`-based dispatcher**, chosen over a single
fat binary (Option A) because `libcef.so` is 1.34 GB and DT_NEEDED at load:
a unified binary would force every WPE launch — the primary path on this
NVIDIA host — to map all of Chromium and run its 42 load-time constructors.
Keeping the engines in separate process images avoids that entirely.

### Measured facts that justify the shape (for the record)

- `libWPEWebKit-2.0.so.1` = 165 MB; `libcef.so` = **1.34 GB**. Both DT_NEEDED.
- **Zero** colliding exported symbols between the two engine libs
  (`webkit_*`/`wpe_*`: 1330 vs `cef_*`: 240). So engine isolation is a footprint
  decision, not a correctness one — the old interposition crash class does not
  recur either way.
- `libcef.so` has 42 `.init_array` constructors that fire on load (incl. a
  Chromium allocator shim). Reason enough not to map it in WPE mode.
- `solactl open` is bus-mediated (`Topic::OpenUrl`) — already engine-agnostic.
- `crates/sola-browser/` was deleted with the retired GTK browser; references to
  `crates/sola-browser/dist/applications/sola-browser.desktop` in
  `solactl/open.rs` and `sola-make/install.rs` are **stale** and get fixed here.

## Decisions (settled in discussion)

1. **Three crates:** a `sola-browser-core` library + two thin engine binaries +
   a `sola-browser` dispatcher binary.
2. **Dispatcher uses `execv`, not fork+supervise.** The engine process *becomes*
   `sola-browser`; no lingering parent. `sola` (the process manager) already
   supervises components, so a second supervisor is redundant.
3. ~~**Unify the window identity to `app_id = "sola-browser"`** for both engines.~~
   **Reversed 2026-06-20:** each engine reports an engine-specific app_id
   (`"sola-browser-wpe"` / `"sola-browser-cef"`) instead. The shell then tracks
   them as distinct apps (MRU/focus/zone/menu), the launcher shows two labelled
   entries — **"Browser (WPE)"** and **"Browser (CEF)"** — and both engines can
   run at the same time. The unify-vs-split toggle is exactly the constant passed
   into `run()` plus the `builtins.rs` shape, as flagged below; we took the split.

## Architecture

```
crates/
  sola-browser-core/        # NEW lib — all shared chrome, generic over Engine
    src/lib.rs              #   pub: Engine trait, run(), re-exports
    src/engine.rs          #   Engine trait + shared Cmd/NavCmd/InputEvent/TabId/TabInfo
    src/app.rs             #   App<E>, Msg, update, view, consts (was main.rs body)
    src/integration.rs     #   bus receive side (was the identical integration.rs)
    src/input.rs           #   iced event -> InputEvent (was the ~identical input.rs)
    src/shader.rs          #   FrameSlot, shared shader::Program scaffolding + WGSL
    src/run.rs             #   run::<E>(app_id) entry point

  sola-browser-wpe/         # THIN bin — WPE engine impl only
    src/main.rs            #   fn main() { sola_browser_core::run::<WpeEngine>("sola-browser") }
    src/engine.rs          #   WpeEngine + impl Engine (was wpe.rs)
    src/frame.rs           #   WpeFrame + wgpu dma-buf import (was wgpu_import.rs + WpePrimitive)
    src/sola_wpe.{c,h}, wpe_wrapper.h, wpe_sys.rs   # FFI, unchanged
    src/bin/*.rs           #   probes, unchanged

  sola-browser-cef/         # THIN bin — CEF engine impl only
    src/main.rs            #   fn main() { sola_browser_core::run::<CefEngine>("sola-browser") }
    src/engine.rs          #   CefEngine + impl Engine (was cef.rs)
    src/frame.rs           #   CefFrame + import (was cpu_import.rs + CefPrimitive)

  sola-browser/             # NEW dispatcher bin — NO engine deps
    src/main.rs            #   parse engine, resolve sibling binary, execv
    dist/applications/sola-browser.desktop          # restores the .desktop handler
```

`sola-browser-core` depends on `iced`, `wgpu`, `wgpu-hal`, `ash`, `sola-bus`,
`sola-core`, `sola-kit` — the deps both browsers already share. It depends on
**neither** engine lib. Each engine bin depends on `sola-browser-core` plus its
own engine lib. The dispatcher depends on neither engine bin nor any engine lib
(std + `sola-core` for env/config only) — that is what keeps Chromium out of the
WPE launch path.

## The `Engine` trait (the core boundary)

Both engine handles already expose an identical 7-method surface; the trait just
names it. `Cmd`, `NavCmd`, `InputEvent`, `TabId`, `TabInfo` move to
`sola-browser-core` verbatim (they are shared-shape today). `TaggedFrame`
becomes generic over the engine's frame.

```rust
// sola-browser-core/src/engine.rs  (design sketch; exact code in the plan)

pub trait Engine: Sized + 'static {
    /// Engine-specific raw frame (WpeFrame: dma-buf fd; CefFrame: dma-buf or CPU buffer).
    type Frame: Send + 'static;

    /// The iced shader Program that imports `Self::Frame` into wgpu and samples it.
    /// Construct it from a `FrameSlot<Self::Frame>`. This is the one piece of the
    /// render path that stays engine-specific.
    type Program: iced::widget::shader::Program<Msg> + 'static;

    /// CEF subprocess gate. Runs first in `run()`, before logging/Wayland init.
    /// WPE returns `None` (no typed subprocesses); CEF dispatches workers when
    /// argv carries `--type=` and returns `Some(exit_code)`.
    fn dispatch_subprocess(_app_id: &'static str) -> Option<std::process::ExitCode> { None }

    /// Bring the engine up. Encapsulates ALL engine-specific startup quirks —
    /// e.g. WPE's WEBKIT_EXEC_PATH + WAYLAND_DISPLAY hide/restore dance moves
    /// inside this method, so `run()` calls it uniformly.
    fn spawn(app_id: &'static str, url: &str, w: u32, h: u32) -> Self;

    fn cmd_sender(&self) -> std::sync::mpsc::Sender<Cmd>;
    fn tabs_handle(&self) -> TabsHandle;       // Arc<Mutex<Vec<TabInfo>>> alias
    fn active_tab_handle(&self) -> ActiveHandle; // Arc<AtomicU32> alias
    fn cursor_handle(&self) -> CursorHandle;
    fn frames(&self) -> FrameReceiver<Self::Frame>; // Arc<Mutex<Receiver<TaggedFrame<Self::Frame>>>>
    fn make_program(slot: std::sync::Arc<FrameSlot<Self::Frame>>) -> Self::Program;
    fn shutdown(&self);
}
```

### What is shared vs engine-specific

| Concern | Lands in | Notes |
|---|---|---|
| `App` struct, `Msg`, `update`, `view` | core (`app.rs`, generic `App<E>`) | identical today modulo engine spawn call |
| nav bar, tab sidebar, resize-drag overlay | core | already built on `sola-kit` widgets |
| bus integration (`integration.rs`) | core | byte-identical after engine-name normalization |
| input mapping (`input.rs`) | core | near-identical; reconcile to one during port |
| `Cmd`/`NavCmd`/`InputEvent`/`TabId`/`TabInfo` | core (`engine.rs`) | shared-shape today |
| `FrameSlot`, resize-feedback in `prepare`, render WGSL | core (`shader.rs`) | the size loop + sampling pass are engine-agnostic |
| `run::<E>()` boot flow + iced builder | core (`run.rs`) | one copy of the `iced::application(...)` setup |
| `WpeEngine`/`CefEngine` + worker thread | engine bin | the irreducible ~950 lines each |
| `WpeFrame`/`CefFrame` + wgpu import (Primitive/Pipeline) | engine bin | dma-buf (WPE) vs dma-buf/CPU (CEF) |
| CEF subprocess dispatch | engine bin (`Engine::dispatch_subprocess`) | `browser_subprocess_path = current_exe()` unchanged |

### The shared `run()` entry point

```rust
// sola-browser-core/src/run.rs  (design sketch)
pub fn run<E: Engine>(app_id: &'static str) -> std::process::ExitCode {
    if let Some(code) = E::dispatch_subprocess(app_id) { return code; } // CEF worker gate, first
    sola_core::log::init(app_id);
    let _ = sola_core::env::activate_wayland_session(10_000);
    let url = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.into());
    let engine = E::spawn(app_id, &url, VIEW_W, VIEW_H);   // WPE env dance lives inside spawn
    // ... build FrameSlot<E::Frame>, set process statics, BusSetup::new(app_id) ...
    // ... iced::application(App::<E>::new, App::update, App::view)
    //         .window(application_id = app_id) ... .run()
}
```

Each engine `main.rs` is then literally:

```rust
fn main() -> std::process::ExitCode {
    sola_browser_core::run::<WpeEngine>("sola-browser")   // or ::<CefEngine>
}
```

## The `sola-browser` dispatcher

A dependency-light bin that selects an engine and `execv`s the sibling binary.

```rust
// crates/sola-browser/src/main.rs  (design sketch)
fn main() -> ExitCode {
    let engine = pick_engine();                  // --engine flag | $SOLA_BROWSER_ENGINE | default "wpe"
    let dir = std::env::current_exe()?.parent()?; // install-prefix independent
    let target = dir.join(format!("sola-browser-{engine}"));
    let target = if target.exists() { target } else { fallback_to_other(dir, engine)? };
    let rest: Vec<_> = std::env::args_os().skip(1).filter(|a| !is_engine_flag(a)).collect();
    // execv replaces this process; current_exe() in the child = the engine binary,
    // so CEF's browser_subprocess_path / --type= worker re-exec resolves correctly.
    Err(std::process::Command::new(&target).args(rest).exec()) // std::os::unix::process::CommandExt
}
```

- **Selection precedence:** `--engine wpe|cef` > `$SOLA_BROWSER_ENGINE` > default
  (`wpe`). Leaves room for future host-based defaulting (WPE on NVIDIA, CEF on
  Mesa — see `project_gpu_nvidia`).
- **Passthrough:** the URL and WPE's `--app <url>` / `--profile <name>` modes
  pass through untouched; only `--engine` is consumed.
- **Fallback:** if the chosen engine binary is missing (e.g. CEF's 1.34 GB
  distribution not installed), warn and `exec` the other engine instead of
  failing.
- **`.desktop`:** `dist/applications/sola-browser.desktop` ships with this crate
  and registers `sola-browser %u` as the http/https handler, fixing the stale
  references in `solactl/open.rs` and `sola-make/install.rs`.

## app_id (per-engine) + launcher

**Updated 2026-06-20** — superseded the unify-to-`"sola-browser"` plan. Each
engine reports its own app_id: `sola-browser-wpe/main.rs` passes
`"sola-browser-wpe"` into `run()`, `sola-browser-cef/main.rs` passes
`"sola-browser-cef"`.

- `crates/sola-shell/src/builtins.rs`: two labelled entries — **"Browser (WPE)"**
  → `/opt/sola/bin/sola-browser --engine wpe` (app_id `sola-browser-wpe`,
  `lucide/globe`) and **"Browser (CEF)"** → `/opt/sola/bin/sola-browser --engine
  cef` (app_id `sola-browser-cef`, `lucide/earth`). Both go through the
  dispatcher; the `--engine` flag is explicit on each so the entries are
  self-documenting.
- The shell tracks each engine's windows under its own identity for
  MRU/focus/zone/menu, so a WPE browser and a CEF browser coexist as distinct
  apps. (The `"sola-browser"` strings in `sola-bus`/`sola-shell` tests are
  arbitrary sample app_ids, unrelated to these live ids.)
- Trade-off: no single shared zoning default / synthesized-menu identity across
  engines — each engine carries its own. Worth it to run both at once and tell
  them apart in the shell.

## Build system (`sola-make`)

- `sola-make` discovers isolated/workspace-excluded crates and installs them
  alongside. Add `sola-browser` to the install set; `sola-browser-core` is a
  library (built transitively, not installed).
- Both engine bins keep their current build-deps: WPE keeps `bindgen`/`cc`/
  `pkg-config` + `sola_wpe.c`; CEF keeps the `cef` crate + `install-cef` fetch.
  `sola-browser-core` has no build script.
- Install layout under `/opt/sola/bin/`: `sola-browser`, `sola-browser-wpe`,
  `sola-browser-cef` (siblings — the dispatcher resolves them relative to itself).
- Workspace-exclusion: `sola-browser-core` inherits the same exclusion rationale
  (iced's `smithay-clipboard` flips `wayland-sys` to dlopen). The engine bins and
  dispatcher stay excluded as today.

## Migration strategy (keep it building at every step)

1. Create `sola-browser-core` as a library; move the shared, engine-agnostic
   types (`Cmd`/`NavCmd`/`InputEvent`/`TabId`/`TabInfo`, `FrameSlot`, WGSL,
   resize-feedback) into it. Define the `Engine` trait. Build the lib alone.
2. Port `sola-browser-wpe` onto the lib: implement `Engine` for `WpeEngine`,
   move the WPE-specific frame import behind `Engine::Program`/`type Frame`,
   reduce `main.rs` to the `run::<WpeEngine>` one-liner. Build + smoke WPE.
3. Port `sola-browser-cef` the same way. Build + smoke CEF.
4. Add the `sola-browser` dispatcher + `.desktop`; wire `sola-make`.
5. Flip `app_id` to `"sola-browser"` in `run()`; update `builtins.rs`; fix the
   stale `crates/sola-browser/...` references.

Each step leaves the tree compiling and each browser runnable, so regressions
are bisectable to one engine.

## Risks & non-goals

- **Generic `App<E>` ergonomics.** The shared `view`/`update` become generic over
  `E: Engine`; the iced `shader::Program` is an associated type. This is the main
  implementation friction. Mitigation: keep the trait narrow — the only
  engine-specific render seam is `make_program` + `type Frame`; everything else
  is concrete.
- **`input.rs` reconciliation.** The two copies differ slightly (293 vs 300
  lines). The port must converge them to one; verify no engine relied on a local
  quirk.
- **Non-goal:** changing the render pipeline, the dma-buf import strategy, or CEF
  vs WPE behaviour. This is a refactor — engine swap only, byte-for-byte where
  possible.
- **Non-goal:** the resize-feedback loop investigated earlier. Left as-is unless
  it resurfaces.

## Open question for review — RESOLVED 2026-06-20

- **app_id unification** (Decision 3): ~~unify to `"sola-browser"`~~ → **resolved
  to keep per-engine `sola-browser-wpe`/`-cef`**, preserving shell-level engine
  distinction and two launcher entries. The implementation initially shipped the
  unified form; this was reversed so both engines can run side by side. See the
  "app_id (per-engine) + launcher" section above.
