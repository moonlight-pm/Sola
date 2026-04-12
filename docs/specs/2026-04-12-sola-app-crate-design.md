# sola-app Crate — Design Spec

**Date:** 2026-04-12
**Scope:** Shared crate providing the WebView application framework for all Sola shell apps. Bundles Rust host (GTK4/WebKit6, URI scheme, IPC bridge) and TypeScript platform lib (Arrow.js, reactive state, IPC).

## Goal

Eliminate per-app boilerplate for WebView setup, IPC plumbing, and frontend runtime. Apps depend on `sola-app` and get:
- `app:///` URI scheme with on-demand TypeScript stripping
- WebKit `UserContentManager` message handlers (JS↔Rust, no network)
- glib↔tokio bridge for async command dispatch
- Arrow.js + reactive state + IPC lib served automatically
- Bus connection + polling
- Logging, Wayland socket wait, window creation

## Crate Structure

```
crates/sola-app/
  src/
    lib.rs          # Public API: SolaApp builder, AppHandler trait
    webview.rs      # WebContext, URI scheme, UserContentManager setup
    bridge.rs       # glib↔tokio channel plumbing
    assets.rs       # embed_web! macro + built-in lib/vendor serving
    strip.rs        # swc_ts_fast_strip wrapper
  web/
    lib/            # Platform JS (served at /lib/)
      ipc.ts        # WebKit message handler bridge
      store.ts      # createStore(), persist(), save()
      theme.ts      # CSS custom property application
    vendor/         # Platform vendor deps (served at /vendor/)
      arrow/
        index.mjs
        index.d.ts
        chunks/internal-DchK7S7v.mjs
        internal.d.ts
  Cargo.toml
```

## Rust API

### AppHandler Trait

```rust
#[async_trait]
pub trait AppHandler: Send + Sync + 'static {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value;
}
```

Single method. Apps match on command names and return JSON results. The framework calls this from the tokio thread when JS sends a command via `postMessage`.

### SolaApp Builder

```rust
SolaApp::builder()
    .app_id("sola-terminal")
    .window_size(1920, 1080)
    .decorated(false)
    .web_assets(embed_web!("web/"))
    .initial_state(&restored_json)
    .handler(|event_tx| Terminal::new(event_tx))
    .on_bus_event(|topic, send_to_js| {
        if let Topic::Key(KeyEvent { code: KEY_T, pressed: true, super_held: true, .. }) = topic {
            send_to_js(json!({"event": "new_tab"}));
        }
    })
    .run();
```

### What `.run()` Does

1. Logging setup: stderr + file at `/opt/sola/log/{app_id}.log`
2. Wayland socket wait (up to 10s)
3. GTK Application creation with `app_id` as prgname
4. WebContext with `app:///` URI scheme:
   - Platform assets (lib/, vendor/) served from crate's embedded files
   - App assets served from `embed_web!` output
   - `.ts` files stripped via `swc_ts_fast_strip` on-demand
   - Import map for platform deps injected into `index.html`
   - `__RESTORED_STATE__` replaced with `.initial_state()` value
5. UserContentManager with `sola` message handler
6. glib↔tokio bridge (std::sync::mpsc, polled every 2ms)
7. Tokio thread with command dispatch loop using `AppHandler`
8. Window + WebView creation
9. Bus connection + 50ms polling with `on_bus_event` callback
10. GTK main loop

### Handler Event Channel

The `AppHandler` factory receives a `std::sync::mpsc::Sender<String>` for pushing events to JS outside of command responses (e.g., PTY data). This is the same channel the framework uses — messages go through the glib bridge to `evaluate_javascript("window.__solaRecv(...)")`.

## embed_web! Macro

Walks a directory at compile time and generates a static asset table:

```rust
embed_web!("web/")
// Generates:
// &[
//     ("/index.html", "...", ContentType::Html),
//     ("/src/app.ts", "...", ContentType::TypeScript),
//     ("/src/theme.css", "...", ContentType::Css),
//     ("/vendor/xterm.mjs", "...", ContentType::JavaScript),
//     ...
// ]
```

File types detected by extension:
- `.ts` → TypeScript (stripped on serve)
- `.js`, `.mjs` → JavaScript (served as-is)
- `.css` → CSS
- `.html` → HTML
- `.d.ts` → skipped (editor-only, not served)

## Asset Resolution Order

When the browser requests `app:///path`:
1. Check app assets (from `embed_web!`)
2. Check platform assets (lib/, vendor/ from sola-app crate)
3. 404

Apps own `/src/`, `/index.html`, and optionally `/vendor/` for app-specific deps.
Platform owns `/lib/` and `/vendor/arrow/`.

## Import Map Injection

The framework scans the app's `index.html` for an existing `<script type="importmap">`. If found, it merges platform entries into the `"imports"` object. If not found, it injects one before the first `<script>` tag.

Platform entries:
```json
{
  "imports": {
    "@arrow-js/core": "/vendor/arrow/index.mjs"
  }
}
```

App entries (e.g., xterm) stay in the app's HTML and are preserved.

## Bus Integration

Framework handles:
- `BusClient::connect()` with graceful fallback
- 50ms polling on glib thread
- Topic parsing

Apps provide an `on_bus_event` closure that receives parsed `Topic` and a `send_to_js` callback. Apps that don't need bus events omit `.on_bus_event()`.

## Editor Type Resolution

Apps point their `tsconfig.json` paths at the crate's source files:

```json
{
  "paths": {
    "@arrow-js/core": ["../../crates/sola-app/web/vendor/arrow/index.d.ts"],
    "/lib/ipc.js": ["../../crates/sola-app/web/lib/ipc.ts"],
    "/lib/store.js": ["../../crates/sola-app/web/lib/store.ts"],
    "/lib/theme.js": ["../../crates/sola-app/web/lib/theme.ts"]
  }
}
```

The `.ts` sources serve double duty: runtime code (stripped and served) and editor type source. No separate `.d.ts` generation needed for the lib modules.

## Terminal Refactor

After `sola-app` exists, `apps/terminal/` becomes:

**Keeps:** `web/src/` (app.ts, terminal-pane.ts, components/sidebar.ts, theme.css), `web/index.html`, `web/vendor/xterm*`, `src/main.rs` (~50 lines), `src/commands.rs` (dispatch logic), `src/pty.rs`, `src/tmux.rs`, `src/state.rs`

**Removes:** WebContext/URI scheme setup, UserContentManager setup, glib↔tokio bridge, TS stripping, logging setup, Wayland wait, bus boilerplate, `web/src/lib/`, `web/vendor/arrow/`

## Future: Component Library

`components/sidebar.ts` stays in terminal for now. Once 2-3 apps use similar sidebar patterns, it moves to `sola-app/web/components/` and gets served at `/components/`.
