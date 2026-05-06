# sola-kit

Successor to [[sola-app]]. Same `SolaApp` shape, different runtime: CEF
(Chromium Embedded Framework) instead of WebKitGTK, smithay-client-toolkit
(direct Wayland) instead of GTK4. The crate is *both* a library every
future Sola WebView app links against, *and* a binary — the storybook /
theme editor that exercises the kit's own components.

When sola-kit reaches feature parity with sola-app the rest of the apps
(sola-shell, sola-settings, sola-terminal, etc.) migrate to it; sola-app
goes away.

## What it provides

**Rust side:**
- xdg_toplevel via [smithay-client-toolkit](https://crates.io/crates/smithay-client-toolkit) — direct Wayland, no GTK
- CEF browser per window with offscreen rendering (OSR)
- Two OSR transports — wl_shm (CPU readback, current) and dma-buf
  (zero-copy GPU, deferred). See [[Distribution#CEF OSR transport]] for
  why we're on wl_shm right now and when to revisit.
- `app:///` custom CEF scheme (no HTTP server, no network)
- On-demand TypeScript + JSX transform via swc — Preact's automatic
  runtime is hardcoded as the JSX import source so app code never
  imports `h` or `Fragment`
- CEF MessageRouter (`window.cefQuery` ↔ Rust) for JS→Rust IPC
- Bus pump on the CEF UI thread (16 ms tick, mirrors the Wayland
  event-pump cadence) that drains [[sola-bus]] and dispatches via the
  app's `BusRegistry`
- xdg configure → `BrowserHost::was_resized` so compositor resizes
  reach Chromium without flicker
- wl_pointer + wl_keyboard → CEF input — pointer covers
  enter/leave/motion/press/release/wheel; keyboard covers KEYDOWN /
  KEYUP / CHAR with a focused keysym→Windows-VK table for editing keys
- Logging (stderr + `/opt/sola/log/{app_id}.log`); Chromium's own
  `LOG(ERROR)` output is suppressed via `LOGSEVERITY_DISABLE` so the
  TTY isn't drowned in dbus probe noise. `LOG(FATAL)` still surfaces.

**TypeScript side (served automatically at `/lib/`):**
- `ipc.ts` — `invoke(cmd, args)` over `window.cefQuery` and
  `on(event, cb)` listeners over the `__solaRecv` reply channel

That's it. The kit makes **no JS framework choice**. Preact, Lit,
Svelte, vanilla — pick whatever in the *app's* `asset_bundle!` and
declare the matching importmap in the app's `index.html`. The
storybook (sola-kit's own bin) chose Preact + signals; that's a
storybook decision, not a kit decision.

## App author API

Same trait shape as [[sola-app]] — `SolaApp::APP_ID` and
`SolaApp::new` are required, everything else has a no-op default:

```rust
use sola_kit::{AppCtx, AssetBundle, BusRegistry, SolaApp, asset_bundle};
use sola_bus::topics::{Topic, TopicKind};

pub static WEB: &AssetBundle = &asset_bundle! {
    "/index.html"        => (include_str!("../web/index.html"), Html),
    "/index.tsx"         => (include_str!("../web/index.tsx"), Tsx),
    "/components/Main.tsx" => (include_str!("../web/components/Main.tsx"), Tsx),
    // App owns /vendor/* if it wants a framework.
};

pub struct MyApp { /* ... */ }

impl SolaApp for MyApp {
    const APP_ID: &'static str = "sola-myapp";

    fn new(ctx: &mut AppCtx) -> Self {
        ctx.add_window(/* WindowConfig ... */);
        MyApp { /* ... */ }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        // Handlers registered here become this app's bus subscription.
        // The default CloseApp handler is inherited; don't re-register.
        bus.on(TopicKind::Theme, Self::on_theme);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &serde_json::Value,
        id: Option<u64>,
        source: &sola_kit::WindowHandle,
        ctx: &mut AppCtx,
    ) {
        // handle invokes from JS; reply via source.send_to_js({"id": id, "result": ...})
    }
}

fn main() -> std::process::ExitCode {
    // CEF re-execs this binary as the renderer/GPU/utility/zygote
    // workers. The subprocess gate runs the worker and exits before
    // the browser-process path kicks in.
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess() {
        return code;
    }
    sola_kit::run::<MyApp>();
    std::process::ExitCode::SUCCESS
}
```

`SolaApp::APP_ID` is reported to the compositor as
`xdg_toplevel.app_id` (sola-river's per-app focus / zoning rules see
the right id) and to [[sola-bus]] as `set_app_id`.

## Asset resolution

`app:///` requests check in order:

1. App assets (from the app's `asset_bundle!`)
2. Platform assets — currently only `/lib/ipc.ts`

`AssetBundle::find` adds two convenience fallbacks at lookup time:

- `.js` requests fall back to `.ts` / `.tsx` / `.jsx` of the same
  stem — browsers ask for `./foo.js` because that's what their
  module loader resolves; the source on disk is `.ts`.
- Extensionless paths (`./foo`) probe `.ts`, `.tsx`, `.jsx`, `.js`,
  `.mjs` in that order, matching what the editor sees with
  `tsconfig.moduleResolution: "bundler"`.

## IPC flow

```
JS:  invoke("cmd", { args })
  → window.cefQuery({ request: '{"id":N,"cmd":"cmd","args":{...}}', ... })
  → renderer-side router → ProcessMessage(IPC) → browser process
  → KitClient::on_process_message_received
    → BrowserSideRouter::on_process_message_received
      → KitBrowserHandler::on_query_str
        ↳ thread_local lookup by browser_id → per-window JsDispatcher
        ↳ SolaApp::on_js_command(cmd, args, id, source, ctx)   [CEF UI thread]
        ↳ callback.success_str("")                              [acks the cefQuery]
  → reply (when id is Some):
    source.send_to_js({"id": N, "result": ...})
      → CefFrame::execute_java_script("window.__solaRecv(...)")
      → ipc.ts: pending Map by id → Promise resolves
```

The cefQuery `onSuccess` callback is intentionally a no-op: replies
travel back through `__solaRecv` (matched by `id` in
`web/lib/ipc.ts`'s `pending` Map). cefQuery's `onFailure` rejects the
matching pending promise on transport errors (e.g. renderer process
crashed mid-query).

Rust→JS one-way events follow the same `__solaRecv` path with no
`id` field — `ipc.ts::on(event, cb)` listeners pick them up by
`msg.event`.

## Bus → CEF UI thread bridge

A recurring CEF UI-thread task (`BusPumpTask<A>`, re-posted every
16 ms) drains the `BusClient` and dispatches via the app's
`BusRegistry`:

```
drain bus
  → for each msg:
      app.on_raw_bus_message(&msg, ctx)
      parse Topic
      if Shutdown: app.on_shutdown(ctx); cef::quit_message_loop()
      else: registry.dispatch(&delivery, app, ctx)
```

Framework-level interception is currently minimal — only `Shutdown`.
sola-app's bus loop additionally intercepts `Windows` (cache snapshot),
`Copy`/`Paste` (clipboard via GdkClipboard), `Evaluate` (JS exec). Those
land on sola-kit when the matching consumer arrives — Copy/Paste in
particular needs a Wayland-native mechanism, not GdkClipboard.

## Process model

CEF's process model is multi-process: one browser process (us, the
sola-kit binary) plus a zygote, GPU process, renderer per top-level
browsing context, utility processes for storage / network. CEF
re-execs *our* binary with `--type=...` for each subprocess — the
subprocess gate at the top of `main()` calls `cef::execute_process`,
which runs the worker and exits.

`KitCefApp::render_process_handler()` returns a `KitRenderProcessHandler`
that lives in renderer subprocesses and wires the renderer-side
MessageRouter into V8 context lifecycle (`OnContextCreated` /
`OnContextReleased`) so `window.cefQuery` is installed on every
context.

## Build / install

`cargo make build sola-kit`, `cargo make install sola-kit`.

CEF binaries (libcef.so + Resources/* + locales/*) are downloaded
once by `cargo make install-cef` to
`~/.cache/sola/cef-<version>/`. The build wires sola-kit's RUNPATH
to `~/.cache/sola/cef-<version>/Release/` directly so the user's
running binary picks up the patched libcef.so without
`LD_LIBRARY_PATH` gymnastics.

`crates/download-cef-stub/` replaces the upstream
`tauri-apps/download-cef` build dependency (which would have
downloaded CEF a *second* time during cargo build, dragging
`ureq`/`rustls`/`bzip2`/`tar` into our build graph). The stub
expects `cargo make install-cef` to have populated the cache, then
mirrors `Release/` into the cef-dll-sys build output as a flat
symlink tree — Chromium loads `icudtl.dat` and friends from the
directory next to libcef.so at runtime, so they have to be there.

## Relation to sola-app

| Concern        | [[sola-app]]             | sola-kit                            |
|----------------|--------------------------|-------------------------------------|
| WebView engine | WebKitGTK 6.0            | CEF 147                             |
| Window backend | GTK4                     | smithay-client-toolkit (direct WL)  |
| TS compiler    | `swc_ts_fast_strip`      | full `swc_core` (TS + JSX)          |
| JS framework   | Arrow.js (vendored)      | app's choice — kit ships none       |
| JS↔Rust IPC    | WebKit messageHandlers   | CEF MessageRouter (`cefQuery`)      |
| Bus event loop | `glib::unix_fd_add_local`| CEF UI-thread re-posted task        |
| Composition    | GTK widget tree          | xdg_toplevel + `BrowserHost::was_resized` |

The `SolaApp` trait surface is intentionally identical between the
two so apps migrate by switching `use sola_app::*` → `use
sola_kit::*` and updating their `Cargo.toml`. The `register_bus` /
`on_js_command` / `on_shutdown` shape is the same.

## Frontend stack the storybook chose

Documented in the worktree's `CLAUDE.md` (preact + signals + JSX
with the swc automatic runtime). That choice is **not part of the
kit** — apps that don't want Preact can ignore it entirely and ship
their own framework + importmap.

## Pending work

- LoadHandler::OnLoadEnd + an `eval_js` pre-load queue in `WindowInner`
  — currently `eval_js` calls into CEF immediately; messages emitted
  before the page commits its document may be dropped. Bites the
  moment bus dispatch starts replaying sticky topics at startup.
- Multi-browser dispatch in `cef/scheme.rs` — single-window static
  registry today; needs per-browser routing once DevTools (a separate
  OSR Surface) lands.
- DevTools (`Browser::open_devtools` was deleted in the strip-down).
- IME and `wl_touch`.
- Surrogate-pair `CHAR` events (most emoji typed into `<input>`
  won't render correctly).
- `external_begin_frame_enabled = 1` driven from Wayland frame
  callbacks instead of CEF's self-drive.

## See also

- [[sola-app]] — the framework sola-kit is replacing
- [[Distribution]] — CEF runtime libraries on NixOS, OSR transport
  caveats, GPU plumbing
- [[sola-bus]] — IPC bus the kit subscribes to
- [[Topics]] — message catalog
