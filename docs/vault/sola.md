# sola (process manager)

**Crate:** `crates/sola/`
**Binary:** `sola`
**Role:** Pure process supervisor. Launches and manages all other Sola components.

## Responsibilities

- Spawns River compositor first, waits for its wayland socket and XWayland display
- Launches managed processes: sola-bus, sola-river, sola-shell, sola-session
- Watches for crashes, restarts with backoff (2s delay if crashed within 5s of launch)
- Watches all managed binaries for changes via inotify, restarts on update
- Watches own binary, `execv`'s self on update
- Listens on the bus for `Topic::Shutdown`
- Sets `PR_SET_PDEATHSIG(SIGTERM)` on children so they die if sola is killed
- Rotates `sola.log` on startup (100KB max, keeps 10 rotated files)

## River Supervision

River is special — it's not a managed process but a direct child with its own supervisor (`src/river.rs`). River must be running before any wayland clients can start.

- `RiverSupervisor::spawn()` — kills orphan rivers, cleans stale sockets, spawns river via PATH lookup
- `wait_for_socket()` — polls until river opens a live `wayland-N` socket (30s timeout)
- `wait_for_xwayland()` — polls for XWayland display (3s timeout, optional)
- Publishes socket/display names to `$XDG_RUNTIME_DIR/sola-wayland` and `sola-display`
- If River dies, the entire session shuts down

## Binary Resolution

All external binaries (river, etc.) are resolved via `sola_core::process::resolve_binary()` which does `$PATH` lookup. No hardcoded paths — works on NixOS and traditional distros.

## Design Principle

Almost no logic. Almost never changes. The less it does, the less reason to restart it (which would restart everything).

## Source Files

| File | Purpose |
|---|---|
| `src/main.rs` | Process supervision loop, bus client, binary change handling |
| `src/river.rs` | River compositor lifecycle — spawn, socket discovery, orphan cleanup, shutdown |

## Managed Processes

Defined in `MANAGED` const:
```rust
const MANAGED: &[&str] = &[
    "sola-bus",
    "sola-river",
    "sola-shell",
    "sola-session",
];
```
Binaries are discovered relative to the sola binary's own directory.
