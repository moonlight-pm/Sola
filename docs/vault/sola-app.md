# sola-app

Shared crate providing the WebView application framework for all Sola shell apps.

## What It Provides

**Rust side:**
- GTK4/WebKit6 window + WebView lifecycle
- `app:///` custom URI scheme (no HTTP server, no network)
- On-demand TypeScript stripping via `swc_ts_fast_strip`
- WebKit `UserContentManager` message handlers (JS↔Rust IPC)
- glib↔tokio bridge for async command dispatch
- [[Sola Bus]] connection + polling
- Logging (stderr + `/opt/sola/log/{app_id}.log`)
- Wayland socket wait

**TypeScript side (served automatically at `/lib/` and `/vendor/`):**
- `ipc.ts` — `invoke(cmd, args)` and `on(event, cb)` over WebKit `postMessage`
- `store.ts` — `createStore()` (Arrow.js `reactive()` wrapper), `persist()`, `save()`
- `theme.ts` — CSS custom property application
- Arrow.js (vendored, served at `/vendor/arrow/`)

## App Author API

```rust
use sola_app::{SolaApp, AppHandler, embed_web};

struct MyApp { ... }

#[async_trait]
impl AppHandler for MyApp {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value {
        match cmd {
            "my_command" => { ... }
            _ => json!({"error": "unknown"})
        }
    }
}

fn main() {
    SolaApp::builder()
        .app_id("sola-myapp")
        .window_size(1920, 1080)
        .web_assets(embed_web!("web/"))
        .handler(|event_tx| MyApp::new(event_tx))
        .run();
}
```

## Asset Resolution

`app:///` requests check in order:
1. App assets (from `embed_web!("web/")`)
2. Platform assets (lib/, vendor/ from sola-app crate)

Apps own `/src/`, `/index.html`, `/vendor/` (app-specific deps).
Platform owns `/lib/`, `/vendor/arrow/`.

## IPC Flow

```
JS: invoke("cmd", {args})
  → postMessage → UserContentManager callback (glib thread)
  → tokio mpsc → AppHandler::dispatch() (tokio thread)
  → result via std::sync::mpsc → glib poll (2ms)
  → evaluate_javascript("window.__solaRecv(...)") → JS Promise resolves
```

Events (Rust→JS) use the same return path: send JSON through the mpsc channel.

## See Also

- [[Sola Bus]] — IPC bus that sola-app connects to
- [[sola-compositor]] — Wayland compositor that hosts the WebView windows
- Spec: `docs/specs/2026-04-12-sola-app-crate-design.md`
