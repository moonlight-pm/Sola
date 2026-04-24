# sola-app

Shared crate providing the WebView application framework for every
Sola shell app.

## What it provides

**Rust side:**
- GTK4 / WebKit6 window + WebView lifecycle
- `app:///` custom URI scheme (no HTTP server, no network)
- On-demand TypeScript stripping via `swc_ts_fast_strip`
- WebKit `UserContentManager` message handlers (JS ↔ Rust IPC)
- glib↔tokio bridge for async command dispatch
- [[sola-bus]] connection, subscription derived automatically from
  the handlers registered in `register_bus`
- Logging (stderr + `/opt/sola/log/{app_id}.log`)
- Wayland socket wait

**TypeScript side (served automatically at `/lib/` and `/vendor/`):**
- `ipc.ts` — `invoke(cmd, args)` and `on(event, cb)` over WebKit
  `postMessage`
- `store.ts` — `createStore()` (Arrow.js `reactive()` wrapper),
  `persist()`, `save()`
- `theme.ts` — CSS custom property application
- Arrow.js (vendored at `/vendor/arrow/`)

## App author API

```rust
use sola_app::{AppCtx, AssetBundle, BusRegistry, SolaApp, asset_bundle};
use sola_bus::topics::{Topic, TopicKind};

pub static WEB: &AssetBundle = &asset_bundle! {
    "/index.html"     => (include_str!("../web/index.html"), Html),
    "/src/main.ts"    => (include_str!("../web/src/main.ts"), TypeScript),
    /* ... */
};

pub struct MyApp { /* ... */ }

impl SolaApp for MyApp {
    const APP_ID: &'static str = "sola-myapp";

    fn new(ctx: &mut AppCtx) -> Self {
        ctx.add_window(/* WindowConfig ... */, WEB);
        MyApp { /* ... */ }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        // Handlers registered here become the app's bus subscription set.
        // Default CloseApp is inherited from the trait; don't re-register.
        bus.on(TopicKind::Windows, Self::on_windows);
        // ...
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &serde_json::Value,
        id: Option<u64>,
        source: &sola_app::WindowHandle,
        ctx: &mut AppCtx,
    ) {
        // handle invokes from JS
    }
}

fn main() {
    sola_app::run::<MyApp>();
}
```

Only `APP_ID` and `new` are required; every other trait method has
a default no-op so apps opt in to what they need (`on_windows`,
`on_js_command`, `on_shutdown`, …). See
`crates/sola-app/src/lib.rs` for the full trait.

## Asset resolution

`app:///` requests check in order:

1. App assets (from `asset_bundle!`)
2. Platform assets (`/lib/`, `/vendor/arrow/` from sola-app)

Apps own `/src/`, `/index.html`, `/vendor/` (app-specific deps).
Platform owns `/lib/`, `/vendor/arrow/`.

## IPC flow

```
JS: invoke("cmd", {args})
  → postMessage → UserContentManager callback (glib thread)
  → tokio mpsc → SolaApp::on_js_command (tokio thread)
  → result via std::sync::mpsc → glib poll (2ms)
  → evaluate_javascript("window.__solaRecv(...)") → JS Promise resolves
```

Rust→JS events use the same return path: send JSON through the
mpsc channel and it flows back into the WebView.

## Relation to `sola-core::config`

Historically the `JsonConfig` / `JsonConfigIn` traits lived here;
they now live in `sola-core` and are re-exported as
`sola_app::config` for backward compatibility with existing app
code. Apps that want persistence should migrate to a typed
persistent bus topic rather than continuing to use `JsonConfig` —
see [[Topics#Behavior]] and [[sola-bus#Persistence]].

## See also

- [[sola-bus]] — IPC bus that sola-app connects to
- [[Topics]] — message catalog
- Spec: `docs/specs/2026-04-12-sola-app-crate-design.md`
