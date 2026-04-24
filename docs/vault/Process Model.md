# Process Model

Every Sola component runs as a separate process, supervised by [[sola]].

## Why Separate Processes

- **Independent restartability** — rebuild and restart one component
  without touching the rest
- **Crash isolation** — a bug in the shell doesn't take down the bus
- **Development velocity** — deploy changes without a full desktop
  restart

## Supervision

[[sola]] manages all processes:

- **Crash restart** — if a managed process exits with non-zero, sola
  relaunches it
- **Backoff** — if it crashed within 5s of launch, sola waits 2s
  before restarting (no tight loops)
- **Binary watching** — inotify watches every managed binary; on
  change, the updated process is killed and relaunched
- **Self-restart** — if `sola`'s own binary changes, it `execv`'s
  itself (children are relaunched by the new instance)
- **Death signal** — `PR_SET_PDEATHSIG(SIGTERM)` on every child, so
  they die if sola is killed

## Process List

| Process         | Binary         | Role                                            | Always running?               |
|-----------------|----------------|-------------------------------------------------|-------------------------------|
| [[sola]]        | `sola`         | Process manager                                 | Yes (top-level supervisor)    |
| River           | `river`        | Wayland compositor (wlroots)                    | Yes (spawned by sola directly) |
| [[sola-bus]]    | `sola-bus`     | IPC bus host + state.toml persistence           | Yes                           |
| sola-river      | `sola-river`   | Compositor bridge (bus ↔ River wayland)         | Yes                           |
| sola-shell      | `sola-shell`   | Desktop shell — launcher, switcher, menubar     | Yes                           |
| sola-session    | `sola-session` | User-app session manager (spawn, close, reap)   | Yes                           |
| Apps            | `sola-*`       | User-facing apps                                | Launched on demand            |

## Startup Order

River must be running before any Wayland client can start:

1. `sola` spawns River, waits for its wayland socket (30s timeout)
2. `sola` optionally waits for the XWayland display (3s, optional)
3. `sola` launches managed processes: sola-bus, sola-river, sola-shell,
   sola-session
4. Apps are launched later via `Topic::LaunchApp`, handled by
   sola-session

## Shared Primitives (sola-core)

Common helpers used across every managed process:

- `sola_core::log::init(name)` — unified tracing setup (stderr +
  `/opt/sola/log/sola.log`, with process tag and module label)
- `sola_core::log::rotate()` — startup log rotation (100KB, 10 files)
- `sola_core::env` — XDG runtime dir, wayland socket discovery,
  X display probing
- `sola_core::process` — `resolve_binary()` (PATH lookup),
  `set_pdeathsig_sigterm()`, graceful shutdown helpers
- `sola_core::keys` — `KeyCode`, `KeyChord` primitives
- `sola_core::watcher` — inotify-based binary change watchers,
  `exec_self()`
- `sola_core::config` — remaining JSON helpers (`JsonConfig` /
  `JsonConfigIn`) used by `ApplicationsConfig`; being retired as
  that last holdout migrates to a persistent topic
- `sola_core::Encrypted<T>` — serde newtype encrypting only on
  human-readable serializers; see [[sola-bus#Encrypted payloads]]

## State

There is no central config process. Persistent state lives on the
bus and is written by the bus to `~/.config/sola/state.toml`. See
[[Sola#Configuration]] and [[Topics#Behavior]].

## Shutdown

Two paths:

1. `Topic::Shutdown` on the bus — sola hears it, kills all children,
   shuts down River, exits
2. `kill sola` — `PR_SET_PDEATHSIG` ensures children die with it

If River exits unexpectedly, sola shuts down the entire session (all
Wayland clients depend on it).
