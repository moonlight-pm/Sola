# sola-river Handoff

**Date:** 2026-04-16
**Worktree:** `.worktrees/sola-river`
**Branch:** `sola-river`
**Status:** Builds clean, 39 tests pass workspace-wide. Unverified on a TTY.

## What landed

- New crate `crates/sola-river/` (~800 LOC) that:
  - Supervises `/usr/bin/river` (spawn, socket-wait, SIGTERM-then-KILL shutdown,
    child-death watcher thread that exits us on River failure).
  - Connects to River as a Wayland client. Binds
    `river_window_manager_v1` (v4), `river_xkb_bindings_v1` (v2), `wl_seat`.
  - Translates bus → River on `Composition`, `Frame`, `Focus`,
    `RegisteredChords` via a per-tick `PendingUpdate` flush through
    manage/render sequences.
  - Translates River → bus on window lifecycle (`Apps` sticky),
    pointer events (`MouseEntered`/`MouseLeft`/`MouseClicked`), and
    chord presses (`Chord`).
- `sola-compositor` crate deleted (~3600 LOC).
- Bus contract updated: `Windows`→`Apps`, `SetWindowPolicy` and
  `ShellKeyBindings` removed, `RegisteredChords`/`Chord`/`MouseClicked`/
  `MouseLeft` added.
- `sola-shell` reworked:
  - Emits `RegisteredChords` sticky (shell shortcut table + Meta+Tab +
    Meta+Space + Meta+Numpad + Enter/Escape).
  - Consumes `Topic::Chord` via `keys::handle_chord` — same action
    routing as before, now driven from the bus instead of GTK input.
  - Tracks `mru_window_by_app` for switcher restore.
  - Focus-follows-pointer (`MouseEntered`) and focus-on-click
    (`MouseClicked`) both call a shared `focus_window_from_pointer`.
  - Switcher behavior preserved: `Topic::ChordReleased` surfaces the
    `river_xkb_binding_v1.released` event, and Meta+Tab release confirms
    the selection like the old GTK path did.
- `sola-app` framework no longer emits `SetWindowPolicy`; `WindowConfig`
  flags stay on the public struct for app-local behavior.
- `sola-terminal` and `sola-monitor` clean (no `SetWindowPolicy`).
- `sola` process manager's `MANAGED` list swaps `sola-compositor` for
  `sola-river`.

## Verified locally

- `cargo check --workspace` ✅
- `cargo test --workspace` ✅ (10 new tests in `sola-river::{registry,pending}`)
- `cargo make build --release` ✅ — all 8 binaries built, including `sola-river`.

## NOT verified (requires TTY install)

- River actually starts under `sola-river` spawn path (should — standard
  fork/exec with stdio captured to `/opt/sola/log/river.log`).
- Wayland global advertisement for `river_window_manager_v1` arrives
  within the two-roundtrip handshake in `client::connect`.
- First-window lifecycle: `window` → `app_id` → `title` → `Apps` emit.
- Manage/render sequence: `manage_start` → `propose_dimensions` +
  `set_borders` → `manage_finish`; `render_start` → `place_top` +
  `set_position` + `focus_window` → `render_finish`.
- Border disable actually takes effect (`set_borders(Edges::empty(), 0, 0, 0, 0, 0)`).
- `wayland-client` 0.31 dispatch model plays nicely with River's manage/
  render batching under the 20ms bus-tick cadence.
- Chord registration via `get_xkb_binding` + `enable`. Modifier bit
  mapping (shift=1, ctrl=4, mod1=8, mod4=64) matches the xkb-bindings
  protocol's `modifiers` enum.
- Focus-follows-pointer actually lands on the correct window (edge case:
  rapid pointer traversal).
- XWayland windows surface as regular `river_window_v1`s with
  `WM_CLASS` in `app_id`.

## Known gaps / punted to future work

- `OutputGeometry` topic is no longer emitted by `sola-river`. The shell
  falls through to its default layout until output handling is added
  (see TODO in `client/window.rs` for `Event::Output`).
- River's `parent` event (child/transient relationships) not surfaced.
- River's `dimensions` event is acked but not propagated — windows end
  up with whatever River picks, which matches the design doc
  ("propose_dimensions(0, 0) initially") but may need refinement.

## Install procedure

```bash
cd .worktrees/sola-river
cargo make install
# On a TTY:
RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
```

Logs: `/opt/sola/log/sola-river.log` (our tracing output),
`/opt/sola/log/river.log` (River's own output).

## Rollback

```bash
cd /home/joshua/Workspace/Sola
git worktree remove .worktrees/sola-river
git branch -D sola-river
```
