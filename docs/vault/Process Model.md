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

| Process | Binary | Always running? |
|---|---|---|
| [[sola]] | `sola` | Yes (top-level supervisor) |
| [[sola-bus]] | `sola-bus` | Yes |
| [[sola-compositor]] | `sola-compositor` | Yes |
| [[sola-x]] | `sola-x` | Yes (when merged) |
| [[sola-switcher]] | `sola-switcher` | Yes (hot WebView, hidden when inactive) |

## No Launch Ordering

All Sola apps handle missing bus or compositor connections gracefully and reconnect when available. Processes can start in any order.

## Shutdown

Two paths:
1. **Kill chord** (`Super+Shift+Backspace`) — sola hears it on the bus, kills all children, exits
2. **`Topic::Shutdown`** on the bus — sola hears it, kills all children, exits
3. **`kill sola`** — `PR_SET_PDEATHSIG` ensures children die with it
