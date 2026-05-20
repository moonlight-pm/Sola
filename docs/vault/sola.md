# sola (process manager)

**Crate:** `crates/sola/`
**Binary:** `sola`
**Role:** Pure process supervisor. Launches and manages every other
Sola component.

## Responsibilities

- Spawns River first, waits for its wayland socket and XWayland
  display
- Launches managed processes: sola-bus, sola-river, sola-shell,
  sola-session
- Restarts crashed processes with a 2-second backoff if the crash
  happened within 5s of launch
- Watches all managed binaries via inotify and restarts on change
- Watches its own binary, `execv`'s self on update (which relaunches
  children under the new instance)
- Listens on the bus for `Topic::Shutdown`
- Sets `PR_SET_PDEATHSIG(SIGTERM)` on every child so they die if
  sola is killed
- Rotates `sola.log` at startup (100KB max, 10 rotated files)

## River Supervision

River is the only non-managed child — it's spawned directly by sola
with its own supervisor (`src/river.rs`). Every Wayland client
depends on it, so if River dies the whole session exits.

- `RiverSupervisor::spawn()` — kills orphan rivers, cleans stale
  sockets, spawns river via `$PATH`
- `wait_for_socket()` — polls until River opens a live `wayland-N`
  socket (30s timeout)
- `wait_for_xwayland()` — polls for an XWayland display (3s,
  optional)
- Publishes socket/display names to `$XDG_RUNTIME_DIR/sola-wayland`
  and `sola-display`

## Binary Resolution

All external binaries (`river`, managed children, etc.) are
resolved via `sola_core::process::resolve_binary()` which does
`$PATH` lookup. Sola targets NixOS only; that's the only
distribution we test on and the only one we plan to support.

## Design Principle

Almost no logic. Almost never changes. The less it does, the less
reason to restart it (which would restart everything).

## Source Files

| File          | Purpose                                                            |
|---------------|--------------------------------------------------------------------|
| `src/main.rs` | Process supervision loop, bus client, binary-change handling       |
| `src/river.rs`| River lifecycle — spawn, socket discovery, orphan cleanup, shutdown|

## Managed Processes

Defined in the `MANAGED` const in `src/main.rs`:

```rust
const MANAGED: &[&str] = &[
    "sola-bus",
    "sola-river",
    "sola-shell",
    "sola-session",
];
```

Binaries are discovered relative to the sola binary's own directory
(`/opt/sola/bin/` in the installed layout).

## See also

- [[Sola]] — the system overview
- [[Process Model]] — the supervision model in detail
