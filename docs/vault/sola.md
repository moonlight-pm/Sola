# sola (process manager)

**Crate:** `crates/sola/`
**Binary:** `sola`
**Role:** Pure process supervisor. Launches and manages all other Sola components.

## Responsibilities

- Launches [[sola-bus]], [[sola-compositor]], and all shell apps as child processes
- Watches for crashes, restarts with backoff (2s delay if crashed within 5s of launch)
- Watches all managed binaries for changes via inotify, restarts on update
- Watches own binary, `execv`'s self on update
- Listens on the [[Sola Bus]] for `Topic::Shutdown`
- Handles the kill chord (`Super+Shift+Backspace`) from `Topic::Key` events
- Sets `PR_SET_PDEATHSIG(SIGTERM)` on children so they die if sola is killed

## Design Principle

Almost no logic. Almost never changes. The less it does, the less reason to restart it (which would restart everything).

## Source Files

| File | Purpose |
|---|---|
| `src/main.rs` | Process supervision loop, bus client, binary change handling |
| `src/watcher.rs` | inotify binary watcher, debouncing, `exec_self()` for self-restart |

## Managed Processes

Defined in `MANAGED` const:
```rust
const MANAGED: &[&str] = &["sola-bus", "sola-compositor"];
```
Binaries are discovered relative to the sola binary's own directory.
