# Dev Mode — Design Spec

**Date:** 2026-04-12
**Scope:** Live development workflow for sola-app frontends. Two independent pieces: self-restart in `sola-app`, and a `--watch` flag on `cargo make install`.

## Goal

Make the frontend iteration loop: save file → see change on the TTY in seconds. No changes to the asset serving architecture (`app:///`, `include_str!`, on-demand TS stripping). The production binary is the dev artifact.

## Piece 1: Self-Restart in `sola-app`

Every app built with `SolaApp` watches its own binary and restarts itself when the binary is replaced on disk.

### Behavior

- On startup, spawn a background thread that watches the app's own binary via inotify
- Resolve the binary path from `/proc/self/exe`
- Watch the binary's parent directory (not the file itself — inotify doesn't survive file replacement)
- Filter events to the app's own filename
- 500ms debounce to handle rsync-style deploys (write temp file, then atomic rename)
- On confirmed change: call `execv` with the resolved binary path and original `argv`
- Handle the Linux `" (deleted)"` suffix on `/proc/self/exe` when the binary has been replaced but the process still holds the old inode

### Implementation

Copy the relevant logic from `crates/sola/src/watcher.rs`:
- `exec_self()` — resolve path, handle `" (deleted)"`, execv
- Binary watching with debounce — inotify via `notify` crate, 500ms debounce window

Adapt for single-binary watching (sola watches multiple binaries; sola-app watches one). The code is ~80 lines; copying is preferable to a shared crate because the two usages have different shapes (watch-many + restart-children vs. watch-self + exec-self).

### Integration Point

Start the watcher thread inside `SolaApp::run()`, after GTK initialization but before entering the main loop. Every app gets self-restart automatically — no opt-in, no configuration.

### Dependencies

`notify` and `nix` are already workspace dependencies (used by `crates/sola/`). Add them to `sola-app/Cargo.toml`.

## Piece 2: `cargo make install --watch`

### Command Interface

```
cargo make install <app> --watch
```

Added as a flag on the existing `Install` subcommand via clap derive.

### Behavior

1. **Initial build+install:** Run immediately so `/opt/sola/bin` is current before watching
2. **Watch:** Monitor `apps/<app>/` and `crates/sola-app/` for file changes
3. **Debounce:** 500ms after last change before triggering a rebuild
4. **Rebuild+install:** Run `build <app>` then install locally to `/opt/sola/bin/`
5. **Coalesce:** If changes arrive during an active build, queue one pending rebuild — not a pile-up
6. **Error resilience:** Compile failures and install failures are printed but don't kill the watcher. It continues watching for the next change.

### Output

```
[watch] watching apps/terminal/, crates/sola-app/
[watch] initial build + install...
[install] sola-terminal ✓
[watch] changed: apps/terminal/web/src/app.ts
[watch] building sola-terminal...
[watch] installing...
[install] sola-terminal ✓
```

On error:
```
[watch] changed: apps/terminal/web/src/app.ts
[watch] building sola-terminal...
[build] FAILED (exit 1)
  error[E0308]: mismatched types ...
[watch] waiting for changes...
```

### Implementation

Lives entirely in `crates/sola-make/src/main.rs` (or a new `watch.rs` module if it's cleaner). Uses the `notify` crate for file watching. The build and install steps call the same functions as the existing `build` and `install` subcommands.

### `--watch` requires an app name

`--watch` only makes sense with a single app. If `--watch` is passed without an app name, print an error.

## End-to-End Dev Loop

1. Developer runs `cargo make install terminal --watch`
2. Initial build + install completes
3. Developer edits `apps/terminal/web/src/app.ts`
4. Watcher detects the change, rebuilds `sola-terminal`, copies it to `/opt/sola/bin/`
5. `sola-terminal` detects its binary was replaced, execs itself
6. Fresh app appears with the new frontend code

## What This Doesn't Do

- No hot module replacement or live reload — full process restart
- No state preservation across restarts — that's the app's responsibility
- No changes to the asset serving architecture
- No dev server or alternative asset loading path
- No changes to `crates/sola/` (process manager)
