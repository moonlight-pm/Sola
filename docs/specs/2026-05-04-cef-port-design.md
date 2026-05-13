# sola-kit CEF port — design

**Status:** Design approved by user 2026-05-04. Ready for implementation plan.
**Scope:** `crates/sola-kit/` only, worked on in `.worktrees/sola-kit-preact`. No other apps in the workspace are affected.

## Goal

Replace WebKitGTK + GTK4 with Chromium Embedded Framework (CEF) as the WebView engine inside `sola-kit`. Keep the framework's public API (`SolaApp` trait, `AppCtx::add_window`, `WindowHandle`, `BusRegistry`, `asset_bundle!` macro) intact. The change is invisible to any future `SolaApp` consumer; only the engine inside the box swaps.

## Motivation

WebKitGTK's docked Web Inspector deadlocks when the user drags the resize splitter, due to a `gtk_widget_size_allocate()` call on the inspector pane that violates GTK4's measure-before-allocate invariant (verified in `Source/WebKit/UIProcess/API/gtk/WebKitWebViewBase.cpp::webkitWebViewBaseSizeAllocate`). Workarounds at our layer are limited: detach loses "same window" UX, patching WebKitGTK locally is a maintenance treadmill via the Nix overlay. More broadly, planned future use cases — sola-browser with stacked WebViews + chrome inside one window, framework-drawn desktop chrome (titlebars) — are awkward-to-impossible with WebKitGTK in attached/native-windowed mode and natural with CEF in offscreen rendering (OSR) mode.

## Decisions made during brainstorm

| Decision | Choice |
|---|---|
| CEF binary location | `~/.cache/sola/cef-<version>/`, downloaded by `sola-make` on demand |
| Scope | `crates/sola-kit/` only; sola-kit is both framework lib and storybook consumer |
| Rendering mode | **Offscreen rendering (OSR)** — we own the Wayland surface; CEF gives us frame buffers |
| Subprocess handling | **Single-binary** — `main()` short-circuits to `CefExecuteProcess()` for renderer/GPU/utility |
| Wayland client library | `smithay-client-toolkit` (sctk) |
| Buffer transport | **DMA-BUF via `OnAcceleratedPaint`** — zero-copy, no CPU paint fallback in MVP |
| Surface ownership | We own xdg_toplevel, decoration, frame callbacks, input dispatch |
| Sandbox | `no_sandbox = true` for dev; revisit before any "production" use |
| Library-path resolution | `patchelf --set-rpath` at install; dev-mode wrapper sets `LD_LIBRARY_PATH` |

## Architecture

### Process model

A sola-kit app is one process, one main thread:

1. **`main()` entry** — `sola_kit::cef::init::short_circuit_if_subprocess()` checks `argv` for `--type=...`. If present, hands control to `CefExecuteProcess()` which never returns (it runs the renderer/GPU/utility worker). Only the main browser process falls through.
2. **`main()` (browser process)** — initializes CEF (`CefInitialize` with the cache paths from `cef::distribution`), connects the bus client, brings up the `wayland::WaylandClient`, then runs `CefRunMessageLoop` on the main thread.
3. **Background thread** — polls the bus epoll fd; on every delivery, calls `CefPostTask(TID_UI, …)` to run handlers on the main thread. This integrates the bus into CEF's main loop without contention.

### Module layout

```
crates/sola-kit/src/
├── lib.rs              run<A>(), bus loop, theme_css, import map injector  [API kept; impl edits]
├── ctx.rs              AppCtx::add_window — pairs one Surface with one Browser  [edits]
├── window.rs           WindowHandle::{eval_js, send_to_js, …}  [impl over CEF IPC; API unchanged]
├── strip.rs            swc TS+JSX transform  [UNCHANGED]
├── assets.rs           AssetBundle, ContentType, asset_bundle!  [UNCHANGED]
│
├── cef/
│   ├── mod.rs          re-exports
│   ├── init.rs         subprocess detection + CefExecuteProcess + CefInitialize
│   ├── distribution.rs ~/.cache/sola/cef-<ver>/ probe + download (called once at startup)
│   ├── browser.rs      Browser wrapper: lifecycle, ShowDevTools, ExecuteJavaScript
│   ├── handlers.rs     CEF callback impls (RenderHandler::OnAcceleratedPaint, LoadHandler, …)
│   ├── ipc.rs          JS↔Rust bridge (cefQuery on JS side ↔ CefMessageRouter on Rust side)
│   └── scheme.rs       app:// scheme handler factory wrapping AssetBundle
│
├── wayland/
│   ├── mod.rs          re-exports
│   ├── client.rs       WaylandClient singleton: connection, globals, dispatch
│   ├── surface.rs      per-window xdg_toplevel + frame callbacks + dma-buf import
│   └── input.rs        wl_pointer/wl_keyboard/wl_touch → CEF input event translation
│
└── app/                KitApp (storybook) — does not know CEF or Wayland exist
    ├── main.rs         main() = sola_kit::cef::init::short_circuit_if_subprocess() + run::<KitApp>()
    ├── kit_app.rs      [unchanged at API level; impl that calls send_to_js still works]
    ├── catalog.rs      [unchanged]
    └── fonts.rs        [unchanged]
```

### Boundary discipline

- `cef/` and `wayland/` never reference each other directly. They are glued in `ctx.rs::add_window`: the function creates a `wayland::Surface`, then `cef::Browser::new(&surface, …)` consumes the surface as an opaque paint target.
- `app/` (KitApp) sees neither.
- `cef/` is the future engine-swap point. If a future requirement justifies replacing CEF (Servo, Ladybird, etc.), the contract lives at `cef::Browser` and its handler traits.
- `webview.rs` is deleted; its responsibilities split into `cef/scheme.rs`, `cef/ipc.rs`, and `cef/browser.rs`.

## Window creation + frame loop

### Window creation sequence

```
1. ctx.add_window(WindowConfig { … })
        │
        ▼
2. wayland::Surface::new(&wl_client, cfg)
   ├─ Acquire xdg_toplevel from xdg_wm_base
   ├─ Set title, app_id (= APP_ID), initial size, decoration
   ├─ Subscribe to wl_pointer / wl_keyboard / wl_touch / wl_seat
   ├─ Negotiate zwp_linux_dmabuf_v1 capabilities
   └─ Return Surface handle

3. cef::Browser::new(surface_handle, asset_bundles, app_id)
   ├─ Build CefBrowserSettings (background_color, dev_tools_disabled=false, …)
   ├─ Build CefWindowInfo with windowless_rendering_enabled=true,
   │    external_begin_frame_enabled=true (so we drive vsync via wl frame callbacks)
   ├─ CefBrowserHost::CreateBrowserSync(window_info, RenderHandler { surface_handle },
   │    LoadHandler, IpcHandler, …, settings, "app:///index.html")
   └─ Browser handle stored in WindowInner alongside the Surface

4. WindowHandle returned to KitApp::new caller. Same Rc<WindowInner> shape as today.
```

### Frame loop (steady state)

- `wl_surface.frame` callback fires (sola-river is ready for a new frame).
- Our handler calls `host.SendExternalBeginFrame()` on the CEF browser.
- ~1 frame later CEF's GPU process produces a dma-buf and calls `OnAcceleratedPaint(info)`.
- We import the dma-buf via `zwp_linux_dmabuf_v1::create_params` → `wl_buffer`.
- `wl_surface.attach`, `wl_surface.damage_buffer` (with the damage rects from CEF), `wl_surface.commit`.
- Sola-river composites and signals the next frame callback.

This couples CEF's framerate to sola-river's vsync; no tearing, no wasted frames, no copies.

### Resize

`wl_surface.configure` → `host.WasResized()`. CEF renders at the new size; we present, then ack the configure on the first new-size frame to avoid flashing the old buffer.

### Suspend / occlusion

When sola-river marks the toplevel suspended (via xdg_toplevel state, or inferred from unmapped), we call `host.WasHidden(true)`. CEF stops driving frames. `false` on resume.

## IPC + input + DevTools

### Rust → JS

Same wire format as today — `frame.ExecuteJavaScript("window.__solaRecv('{...}')", url, line)`. `web/lib/ipc.ts::recv` is unchanged. The early `__solaRecvQueue` stub in `index.html` stays as protection against early messages.

### JS → Rust

Native CEF idiom: `window.cefQuery({ request: JSON.stringify({cmd, args}), onSuccess, onFailure })`. Rust side implements `CefMessageRouterBrowserSide::Handler::OnQuery`, dispatches to `KitApp::on_js_command(cmd, args, source, ctx)`, then `callback.Success(json_result)` or `callback.Failure(code, msg)`.

Ergonomic upgrade: the manual correlation-id `pending` Map in `web/lib/ipc.ts::invoke` goes away — `cefQuery` is natively request/response. `invoke()` becomes a thin wrapper around it. Public function signature unchanged.

### Input forwarding

`wayland::input.rs` translates wl_seat events to CEF events:

| Wayland event | CEF call |
|---|---|
| `wl_keyboard.enter` / `leave` | `host.SetFocus(true)` / `false` |
| `wl_keyboard.key` | `host.SendKeyEvent(CefKeyEvent { type, native_key_code, modifiers, … })` |
| `wl_keyboard.modifiers` | update modifier state |
| `wl_pointer.enter` / `leave` | `host.SendMouseMoveEvent(…, mouse_leave={false,true})` |
| `wl_pointer.motion` | `host.SendMouseMoveEvent(…)` |
| `wl_pointer.button` | `host.SendMouseClickEvent(…, MBT_*, mouse_up)` |
| `wl_pointer.axis` | `host.SendMouseWheelEvent(…)` |
| `wl_touch.{down,up,motion}` | `host.SendTouchEvent(CefTouchEvent { id, x, y, type })` |
| `zwp_text_input_v3.commit_string` | `host.ImeCommitText(text, …)` |
| `zwp_text_input_v3.preedit_string` | `host.ImeSetComposition(text, …)` |
| `wl_data_device.drop` | deferred (v2) |
| `xdg_activation_v1` | `host.SetFocus(true)` plus the existing focus topic on the bus |

Two gotchas:
- **Keysym translation.** Wayland delivers xkb keycodes; CEF expects "native" key codes. On Linux that's the X11 keysym = `evdev_keycode + 8`, which is exactly what `crates/sola-core/src/keys.rs` already produces. We reuse `KeyCode` directly. For `windows_key_code` in `CefKeyEvent`, we map xkb keysym → Windows VK code via a small table cribbed from Chromium.
- **Modifier consistency.** xkb modifier mask → CEF `EVENTFLAG_*` bitfield in one helper.

### DevTools

CEF's devtools are also OSR-able. `browser.host().ShowDevTools(window_info, render_handler, …)` with the same `windowless_rendering_enabled = true` and `external_begin_frame_enabled = true` flags spawns *another* `cef::Browser` rendering into *another* `wayland::Surface` we own. Same loop, same input forwarding, same lifecycle. F12 produces a second xdg_toplevel beside the inspected window, positioned by sola-river.

There is no attached-mode race because there is no GTK4 `gtk_widget_size_allocate` happening. Chromium's devtools talk over the standard inspector protocol and surface management is ours.

`KitApp::on_menu_action("open_devtools")` calls `Browser::open_devtools()` instead of WebKit's `inspector().show()`. F12 menu wiring is unchanged.

## Distribution + build + runtime

### CEF version pinning

Single source of truth: `crates/sola-make/src/cef.rs::CEF_VERSION`. The exact version is selected during Checkpoint A from the latest stable release on the Spotify CDN at the time the implementation lands; this spec uses `132.3.0` (Chromium 132) as a placeholder for examples below. Bump = one-character edit; the cache key is version-suffixed so multiple versions can coexist on disk and switching is just a recompile.

Source: official Spotify CDN (`https://cef-builds.spotifycdn.com/`), `_linux64_minimal.tar.bz2` variant (~150 MB compressed, ~400 MB extracted).

### Cache layout

```
~/.cache/sola/cef-132.3.0/
├── Release/
│   ├── libcef.so
│   ├── chrome-sandbox
│   ├── libEGL.so, libGLESv2.so, libvulkan.so.1
│   ├── snapshot_blob.bin
│   ├── v8_context_snapshot.bin
│   └── …
└── Resources/
    ├── icudtl.dat
    ├── resources.pak, chrome_100_percent.pak, chrome_200_percent.pak
    └── locales/{en-US,…}.pak
```

`CefSettings` at startup:
- `framework_dir_path = ~/.cache/sola/cef-<ver>/Release/`
- `resources_dir_path = ~/.cache/sola/cef-<ver>/Resources/`
- `locales_dir_path   = ~/.cache/sola/cef-<ver>/Resources/locales/`
- `browser_subprocess_path = current_exe()`
- `no_sandbox = true`
- `windowless_rendering_enabled = true`

### sola-make changes

New module `crates/sola-make/src/cef.rs`:
```rust
pub const CEF_VERSION: &str = "132.3.0";
pub fn ensure_cef() -> PathBuf { … }   // returns the cache path; downloads if missing
fn download_and_extract(dir: &Path) { … }
pub fn cef_path() -> PathBuf { … }
```

Wired into `sola-make build` and `sola-make install`. New explicit subcommand `cargo make install-cef` for refreshes.

### Library-path resolution

- **Build script** `crates/sola-kit/build.rs`: calls `sola_make::cef::ensure_cef()`, emits `cargo:rustc-link-search=$cache/Release` and `cargo:rustc-link-lib=cef`.
- **Install step** in `sola-make install`: runs `patchelf --set-rpath $cache/Release` on the installed `/opt/sola/bin/sola-kit` binary. NixOS-friendly; no `LD_LIBRARY_PATH` wrappers in production.
- **Dev step** (`cargo make run`, watch): the build script writes `target/cef-runpath`; a wrapper reads it and sets `LD_LIBRARY_PATH` for the launched binary. Dev-only.

### Cargo.toml diff (in `crates/sola-kit`)

**Remove:** `gtk4`, `gdk4`, `glib`, `gio`, `pango`, `pangocairo`, `webkit6`.

**Add:**
- `smithay-client-toolkit = "0.19"`
- `wayland-protocols = { version = "0.32", features = ["client", "unstable", "staging"] }`
- `wayland-client = "0.31"`
- A CEF binding crate. Initial pick: the `cef` crate (latest, OSR-friendly, tracks recent CEF) at a version compatible with `CEF_VERSION`. The crate choice is contained to `crates/sola-kit/src/cef/` — swappable for `cef-rs` or a hand-rolled bindgen against CEF's stable C API if it goes stale (decision gate at Checkpoint B; see "Risk profile" below).
- `xdg` or `directories` for cache-path resolution (if not already in tree via a sola-core helper).

### NixOS specifics

- `chrome-sandbox` SUID is unnecessary in dev with `no_sandbox = true`. We default off; a future "production" mode would wrap it via `configuration.nix`.
- CEF runtime depends on: `libGL`, `libgbm`, `libnss`, `libnspr`, plus a few minor surfaces. The full canonical list is documented in a comment block in `crates/sola-kit/build.rs`. User adds them to `configuration.nix` once.

## Migration plan

Six checkpoints, each a commit boundary. Stay on the `sola-kit-preact` branch. Do not merge to master at any point unless explicitly instructed.

### Checkpoint A — CEF distribution working *(no sola-kit changes; build stays green)*

- `crates/sola-make/src/cef.rs` with `CEF_VERSION` + `ensure_cef()` + `download_and_extract`.
- New CLI subcommand `cargo make install-cef`.
- **Verify:** `cargo make install-cef` from a clean machine produces `~/.cache/sola/cef-<ver>/Release/libcef.so` and exits 0.

### Checkpoint B — Empty CEF window *(big break, big payoff)*

Single commit: dependencies + scaffolding + main-loop swap.
- `crates/sola-kit/Cargo.toml`: drop gtk4/webkit6; add sctk/wayland-protocols/wayland-client/cef.
- `crates/sola-kit/build.rs`: new — calls `sola_make::cef::ensure_cef()`, emits link directives.
- `crates/sola-kit/src/cef/{init,distribution,browser,handlers}.rs`: scaffolded with the OSR rendering path.
- `crates/sola-kit/src/wayland/{client,surface}.rs`: scaffolded.
- `crates/sola-kit/src/webview.rs`: deleted.
- `crates/sola-kit/src/ctx.rs::add_window`: rewritten to pair Surface + Browser; loads `about:blank`.
- `crates/sola-kit/src/lib.rs::run<A>`: replaces `gtk_app.run()` with `CefInitialize` + `CefRunMessageLoop`. Bus polling moved to a background thread that posts via `CefPostTask`.
- `crates/sola-kit/src/app/main.rs`: prepended with the `CefExecuteProcess()` short-circuit.
- **Verify:** install + run from TTY → an empty white CEF window appears in sola-river; resize/move/close work.

### Checkpoint C — `app://` scheme + storybook page renders

- `crates/sola-kit/src/cef/scheme.rs`: custom scheme factory wrapping `AssetBundle` (port of old `webview.rs` scheme handler).
- `ctx.rs::add_window`: navigates to `app:///index.html`.
- **Verify:** Preact counter page renders; `+1` / `reset` buttons work. No theme push, no IPC yet.

### Checkpoint D — Bus + IPC wired *(feature parity)*

- `crates/sola-kit/src/cef/ipc.rs`: `CefMessageRouterBrowserSide::Handler::OnQuery` → `KitApp::on_js_command`.
- Bus loop in `lib.rs` pushes theme via `frame.ExecuteJavaScript("window.__solaRecv(...)")`.
- `crates/sola-kit/web/lib/ipc.ts::invoke`: thin `cefQuery` wrapper; `recv()` queue stub stays.
- `crates/sola-kit/src/wayland/input.rs`: keyboard + pointer event forwarding.
- **Verify:** theme push works (constructable stylesheet `replaceSync` on the JS side); counter still works (proves keyboard input forwarding doesn't regress JS).

### Checkpoint E — DevTools menu wired

- `crates/sola-kit/src/cef/browser.rs::open_devtools()`: spawns a second `cef::Browser` paired with a second `wayland::Surface`.
- `app/kit_app.rs::on_menu_action("open_devtools")`: routes there.
- **Verify:** F12 opens a separate Chromium DevTools window beside sola-kit. Resize panel — no freeze, no warnings.

### Checkpoint F — Polish + cleanup

- `crates/sola-make/src/install.rs`: post-install patchelf to set rpath on `/opt/sola/bin/sola-kit`.
- Update worktree `CLAUDE.md`: replace the Web Frontends section with notes on CEF + sctk + OSR + dma-buf, the dev-mode `LD_LIBRARY_PATH`, and "no_sandbox=true is intentional for now."
- Update `~/CLAUDE.md` with the canonical NixOS package list CEF needs.
- Delete dead code (any straggling gtk4 imports, etc.).
- **Verify:** fresh clone + `cargo make install sola-kit` → app launches and works end to end.

### Risk profile

| Checkpoint | Risk | Worst case |
|---|---|---|
| A | Low — pure download script | Tarball URL changes; bump version constant |
| B | **High** — biggest single change | sctk/CEF binding integration issues; OSR painting doesn't work |
| C | Low — scheme handler is a port of existing code | Path/MIME edge cases |
| D | Med — IPC has many sharp edges | cefQuery latency; subprocess message routing edge cases |
| E | Low — DevTools is just another browser | Position/sizing of devtools window |
| F | Low — packaging | NixOS dep list incomplete |

If Checkpoint B blocks for more than ~3 days, that is the signal to step back and reconsider the binding crate choice (`cef-rs`, or hand-rolled bindgen against CEF's stable C API). Do not grind on it indefinitely.

## Non-goals (this spec)

- Porting other apps in the workspace (sola-browser, sola-terminal, sola-shell, sola-settings, sola-monitor). Those still use sola-app's WebKitGTK; they will be ported in follow-up worktrees if and when the user decides to proceed.
- Sandboxing in production mode. We default to `no_sandbox = true` and revisit later.
- DMA-BUF on multi-GPU systems with cross-GPU copies. The pin is single-GPU systems; cross-GPU is a future concern.
- Drag-and-drop, file upload dialogs, full clipboard integration. Stubbed; v2 work.
- CPU paint fallback for `OnPaint`. We commit to the `OnAcceleratedPaint` path; if it fails on a given system, that's a configuration issue (CEF GPU process disabled?) we surface as an error rather than silently degrade.
- Multi-WebView-per-surface composition. The architecture is ready for it (each WebView is a `cef::Browser` + dma-buf source) but the storybook MVP is one Surface = one Browser. Sola-browser's eventual port will exercise the multi-Browser path; that's a separate spec.

## Open questions / future work

- **CEF binding crate finality.** Initial pick is the `cef` crate. If at Checkpoint B the binding is missing OSR ergonomics or has unsafe-surface issues we don't want to live with, we evaluate alternatives. The contract lives at `cef::Browser` so the swap is local.
- **Multi-monitor + per-output scaling.** OSR + dma-buf needs explicit handling of output scale changes. Initial impl can target single-output; multi-output goes in the v2 list.
- **Touchpad gestures (pinch, swipe).** wl_pointer's gesture protocols (zwp_pointer_gestures_v1) → CEF gesture events. Not in MVP; v2.
- **Renderer crash recovery.** CEF can restart a crashed renderer via `host.IsRenderProcessReady()` polling and `OnRenderProcessTerminated` — we hook a basic "log and keep running" handler now and consider auto-restart later.
- **Production sandbox.** When sola is being launched by a non-dev user (or by a non-Joshua maintainer), `no_sandbox = false` plus the SUID `chrome-sandbox` setup needs to be packaged into `configuration.nix`. Out of scope here.
