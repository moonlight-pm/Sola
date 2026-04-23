# Process Model

Every Sola component runs as a separate process, supervised by [[sola]].

## Why Separate Processes

- **Independent restartability** — change one component, rebuild and restart just that one
- **Crash isolation** — a bug in the switcher doesn't take down the compositor
- **Development velocity** — deploy changes without full desktop restart

## Supervision

[[sola]] manages all processes:

- **Crash restart** — if a managed process exits with a non-zero code, sola restarts it
- **Backoff** — if a process crashes within 5 seconds of launch, sola waits 2 seconds before restarting (prevents tight restart loops)
- **Binary watching** — inotify watches all managed binaries; on change, the updated process is killed and relaunched
- **Self-restart** — if sola's own binary changes, it `execv`'s itself (children are relaunched by the new instance)
- **Death signal** — `PR_SET_PDEATHSIG(SIGTERM)` on all children, so they die if sola is killed

## Process List

| Process | Binary | Role | Always running? |
|---|---|---|---|
| [[sola]] | `sola` | Process manager | Yes (top-level supervisor) |
| River | `river` | Wayland compositor (wlroots) | Yes (spawned by sola directly) |
| [[sola-bus]] | `sola-bus` | IPC bus host | Yes |
| sola-river | `sola-river` | Compositor bridge (bus ↔ River wayland) | Yes |
| sola-shell | `sola-shell` | Desktop shell (switcher, launcher, menubar, zoning) | Yes |
| sola-session | `sola-session` | User app session manager + config store | Yes |
| Apps | `sola-*` | User-facing apps (browser, mail, terminal, etc.) | Launched on demand |

## Startup Order

River must be running before any wayland client can start:

1. `sola` spawns River, waits for its wayland socket
2. `sola` optionally waits for XWayland display
3. `sola` launches managed processes (bus, river bridge, shell, session)
4. Apps are launched later via `Topic::LaunchApp` through sola-session

## Shared Primitives (sola-core)

Common helpers used across all managed processes:

- `sola_core::log::init(name)` — unified tracing setup (stderr + `/opt/sola/log/sola.log`)
- `sola_core::log::rotate()` — startup log rotation (100KB, 10 files)
- `sola_core::env` — XDG runtime dir, wayland socket discovery, X display probing
- `sola_core::process` — `resolve_binary()` (PATH lookup), `set_pdeathsig_sigterm()`, `graceful_shutdown()`
- `sola_core::watcher` — binary change watchers, self-restart via `exec_self()`
- `sola_core::config` — JSON config persistence, centralized config store, bus config helpers

## Shutdown

Two paths:
1. **`Topic::Shutdown`** on the bus — sola hears it, kills all children, shuts down River, exits
2. **`kill sola`** — `PR_SET_PDEATHSIG` ensures children die with it

If River exits unexpectedly, sola shuts down the entire session (all wayland clients depend on it).
