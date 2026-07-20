# sola-app

> **Status (2026-07-20):** moved to **`apocrypha/sola-app/`**. Not a
> workspace member. Not installed. Superseded by [[sola-kit]] (iced).
>
> Last consumers were the WebView prototypes under `apocrypha/apps/`
> (retired agent; mail kept as rewrite reference for a future
> `crates/sola-mail`). Do not add new dependents.

Historical GTK4 + WebKit6 WebView application framework. Sources and
the vendored TS helpers (`ipc.ts`, `store.ts`, `theme.ts`, Arrow.js)
live under `apocrypha/sola-app/`.

## What it provided

**Rust side:**
- GTK4 / WebKit6 window + WebView lifecycle
- `app:///` custom URI scheme (no HTTP server, no network)
- On-demand TypeScript stripping via `swc_ts_fast_strip`
- WebKit `UserContentManager` message handlers (JS ↔ Rust IPC)
- glib↔tokio bridge for async command dispatch
- [[sola-bus]] connection, subscription from registered handlers
- Logging + Wayland socket wait

**TypeScript side (served at `/lib/` and `/vendor/`):**
- `ipc.ts` — `invoke` / `on` over WebKit `postMessage`
- `store.ts` — Arrow.js store helpers
- `theme.ts` — CSS custom properties

## Migration

All production UI uses [[sola-kit]]. There is no trait-compatible
drop-in migration path anymore — new apps implement iced
`application`/`daemon` against the kit scaffolding (`startup`,
`BusSetup`, theme, components).

Mail rewrite: lift protocol logic from `apocrypha/apps/mail/src/`,
rebuild UI on iced. See `apocrypha/README.md`.

## Specs (historical)

- `docs/specs/2026-04-12-sola-app-crate-design.md`
- `docs/specs/2026-04-15-sola-app-trait-api-design.md`
