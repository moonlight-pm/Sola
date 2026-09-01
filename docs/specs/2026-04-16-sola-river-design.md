# sola-river — Design

**Date:** 2026-04-16
**Status:** Design, awaiting implementation plan.
**Worktree:** `.worktrees/sola-river`

## 1. Summary

Replace `sola-compositor` (Rust/Smithay, ~3,600 LOC) with `sola-river`, a small Rust crate (~600–900 LOC) that wraps the River Wayland compositor (0.4.2, Arch package) and translates between Sola's shell-authority bus topics and River's `river-window-management-v1` Wayland protocol.

The shell authority model, the `sola-bus` wire format, and every app process remain unchanged on the outside. What changes is who implements the compositor: instead of maintaining our own, we drive a well-tested one over a stable protocol.

## 2. Motivation

Maintaining a compositor is not on Sola's critical path. Recent weeks have been dominated by compositor-level debugging — XWayland dmabuf forwarding, damage optimization, multi-GPU plumbing, X11 positioning, window-id allocation. None of that is Sola's value proposition. River (via wlroots) solves those problems for us; we pay a small translation layer in return.

Concrete gains:
- XWayland works — wlroots handles it; we delete `crates/sola-compositor/src/xwayland/`.
- IME, tablet, gestures, pointer-constraints, multi-output, HiDPI, fractional scaling — all available when we want them, via standard protocols.
- Every dmabuf / DRM / damage edge case is hit by someone else first.

## 3. Non-goals

- Exposing River's custom protocols (`river-layout-v3`, `river-status`, `river-control`, `river-layer-shell-v1`) to external tools. Sola has no use for external tiling engines, status bars, or control CLIs — the bus covers all of that.
- Shipping decorations drawn by River. We turn them off.
- Click-to-focus tuning, raise-on-click policy refinement, multi-monitor zoning, layer-shell-based panels, IME, libinput declarative config. All future work, noted in §12.
- Running River's default init; River is spawned with no init command, and `sola-river` connects to it directly as a WM client.

## 4. Architecture

```
sola (process manager)
  ├── sola-bus               (unchanged)
  ├── sola-river             (NEW — replaces sola-compositor in MANAGED)
  │    └── /usr/bin/river    (spawned as child; inherits WAYLAND_DISPLAY)
  ├── sola-shell             (minor refactors — see §7.3)
  ├── sola-terminal          (minor — drop SetWindowPolicy emission)
  └── sola-monitor           (minor — drop SetWindowPolicy emission)
```

`sola-river` is spawned by `sola` in the standard `MANAGED` list with `PR_SET_PDEATHSIG=SIGTERM`. It in turn forks+execs `/usr/bin/river` as its own child (also with `PR_SET_PDEATHSIG=SIGTERM`). `sola-river` owns River's lifecycle: if River exits, `sola-river` exits; `sola` then respawns `sola-river` under the usual backoff rules.

Only one Wayland process runs at a time: River. Its socket is `wayland-0`. All Sola apps connect to it directly by inheriting `WAYLAND_DISPLAY=wayland-0` from `sola` — the same mechanism used today. `DISPLAY=:0` continues to be exported for X11 apps via XWayland (which River starts lazily on demand).

### 4.1 `sola-river` internal components

| Component | File | Responsibility |
|---|---|---|
| `RiverSupervisor` | `supervisor.rs` | fork+exec `/usr/bin/river`; wait for socket; propagate SIGTERM; exit on river exit. |
| `RiverClient` | `client/mod.rs` + submodules | Wayland client: bind `river_window_management_v1`, `river_xkb_bindings_v1`, `wl_seat`. Issue requests; receive events. |
| `BusClient` | `bus.rs` | Thin wrapper over `sola_bus::BusClient`; subscribes to consumed topics, publishes emitted ones. |
| `Translator` | `translator.rs` | Pure bus↔river mapping. Holds all cross-side state. |
| `WindowRegistry` | `registry.rs` | `u32 ↔ river_window_v1` bidirectional map. Mints `u32` IDs. |
| `NodeRegistry` | `registry.rs` | Caches `river_node_v1` per window and shell surface. |
| `ChordRegistry` | `registry.rs` | `Chord ↔ river_xkb_binding_v1` map for registered chords. |
| `PendingUpdate` | `client/sequence.rs` | Accumulates manage/render changes; flushes per calloop tick. |

Event loop: `calloop` (same choice as `sola-compositor` today). Sources: River's Wayland fd, bus socket fd, child process fd (for River exit detection), calloop timer (for chord-registration debounce if needed).

### 4.2 Boundaries

- `supervisor.rs` imports only `std::process`, `libc`, and the tracing crate. Knows nothing about Wayland or the bus.
- `client/*` imports only Wayland-client crates and our internal registry types. Knows nothing about sola-bus.
- `bus.rs` imports `sola_bus`. Knows nothing about River.
- `translator.rs` is the only file that imports both sides.

Each unit can be read, tested, and changed in isolation.

## 5. Bus contract

### 5.1 Topics consumed by `sola-river`

| Topic | Shape (sketch) | Mapping |
|---|---|---|
| `Composition` | `Vec<CompositionEntry { window_id, .. }>` | For each entry in order, look up `river_window_v1` and its cached `river_node_v1`, issue `place_top`. Last entry ends on top. |
| `Frame` | `FrameUpdate { window_id, x, y, w, h }` | `propose_dimensions(w, h)` in manage sequence; `set_position(x, y)` in render sequence. |
| `Focus` | `FocusTarget { window_id }` or variant for shell-surface / none | `river_seat_v1.focus_window` / `focus_shell_surface` / `clear_focus`. |
| `RegisteredChords` (new, sticky) | `Vec<Chord { keysym: u32, modifiers: u32 }>` | Diff against previous; for added chords call `get_xkb_binding(seat, keysym, modifiers).enable()`; for removed chords destroy the binding. |

### 5.2 Topics emitted by `sola-river`

| Topic | Shape | When |
|---|---|---|
| `Apps` (sticky) | `Vec<App { window_id, app_id, title }>` | On any `window`/`app_id`/`title`/`closed` event from River. Entire list re-emitted (sticky). |
| `Chord` (new) | `Chord { keysym: u32, modifiers: u32 }` | On `river_xkb_binding_v1.pressed`. Released events are ignored for v1. |
| `MouseEntered` (new) | `MouseEntered { window_id }` | On `river_seat_v1.pointer_enter(window)`. |
| `MouseLeft` (new) | `MouseLeft` (no args) | On `river_seat_v1.pointer_leave`. |
| `MouseClicked` (new) | `MouseClicked { window_id }` | On `river_seat_v1.window_interaction(window)`. |

### 5.3 Topics removed

- `SetWindowPolicy` — the Rust type, the bus definition, and all emission sites. Authority over zoning, size, position, auto-focus, and keyboard targeting is now purely the shell's concern.

### 5.4 Notably absent: no `FocusChanged` echo

`sola-river` does not emit a confirmation topic when focus changes. The shell emits `Focus(X)` and treats it as effective immediately; it tracks its own focus state without a round-trip echo. This keeps the shell simpler and avoids a chatty confirmation loop. If focus moves due to mouse movement, the shell learns via `MouseEntered` and emits a new `Focus` itself — still self-originating.

## 6. River protocol usage

### 6.1 Globals bound

- `river_window_manager_v1`
- `river_xkb_bindings_v1` (+ per-seat `river_xkb_bindings_seat_v1`)
- `wl_seat`
- `wl_output` (read-only; for logical coordinate space)

Only the WM holds `river_window_manager_v1` and the xkb-bindings manager. No other client can; attempting produces a protocol error. This is fine — `sola-river` is the only WM-role client.

### 6.2 Window lifecycle

```
river.window(new_id)             →  mint u32; register; emit Apps
river.app_id / title / parent    →  update registry; re-emit Apps
manage_start ... manage_commit   →  propose_dimensions(0, 0) initially; set_borders(0)
render_start ... render_commit   →  node.set_position; node.place_top per Composition
river.closed                     →  destroy; drop from registry; re-emit Apps
```

### 6.3 Decorations off

On every window in its first `manage_start`/`manage_commit` sequence, `sola-river` calls `river_window_v1.set_borders(0)`. This disables River's drawn borders for that window for its lifetime. Clients are responsible for their own decorations via client-side drawing (Sola apps already draw their own WebKit-rendered chrome; third-party xdg-shell clients negotiate CSD/SSD via `xdg_decoration`, which River honors when we ignore borders).

### 6.4 Manage/render sequencing

River's protocol is double-buffered around two sequence types:
- **Manage**: window-management state (dimensions, fullscreen, borders, close). `manage_start` → requests → `manage_commit` → River replies with `dimensions` events per affected window → done.
- **Render**: rendering state (node positions, z-order, focus). `render_start` → requests → `render_commit` → done.

`sola-river` accumulates all pending changes into a `PendingUpdate` struct during bus handling. On each calloop tick (or immediately after all pending bus messages are drained), if `manage_dirty` or `render_dirty`, it flushes:

```
if manage_dirty { manage_start; issue_manage_requests; manage_commit; await dimensions events }
if render_dirty { render_start; issue_render_requests; render_commit }
clear PendingUpdate
```

Sola's bus emits burst-y updates (a keypress often produces Composition + Frame + Focus in immediate succession), so batching per-tick aligns naturally.

### 6.5 XWayland

Transparent. X11 clients appear as regular `river_window_v1`s with their `WM_CLASS` surfacing through the `app_id` event. `sola-river` does not distinguish them. River starts XWayland lazily; we do nothing.

### 6.6 Shell surfaces

`river_shell_surface_v1` promotion is a WM-only operation — only `sola-river` can call `get_shell_surface`. But `sola-shell`'s menubar and overlay are owned by the `sola-shell` process, not `sola-river`. Cross-client wl_surface references are not possible in Wayland.

Consequence: the menubar and overlay remain regular `xdg_toplevel` surfaces, and `sola-river` treats them like any other window. The shell pins them to the top via its `Composition` list (menubar always last in the ordering; overlay also near the top when shown). This matches current behavior — the shell already emits z-order via `Composition` today.

We use `river_shell_surface_v1` for zero surfaces in v1. If we later want a compositor-rendered splash or lock surface owned by `sola-river` itself, it becomes an option.

## 7. Process lifecycle

### 7.1 Startup

1. `sola` spawns `sola-river` (with `WAYLAND_DISPLAY=wayland-0` and `DISPLAY=:0` already set).
2. `sola-river` fork+execs `/usr/bin/river`:
   - env: `WAYLAND_DISPLAY=wayland-0`, `DISPLAY=:0`
   - args: `-log-level info`
   - pre_exec: `PR_SET_PDEATHSIG=SIGTERM`, `setsid` so River has its own process group.
3. `sola-river` polls for `$XDG_RUNTIME_DIR/wayland-0` with exponential backoff (10ms → 1s, cap 5s, total cap 30s). If the socket does not appear, `sola-river` exits with an error; `sola` respawns.
4. `sola-river` connects to River as a Wayland client, binds globals, retrieves `wl_seat`, per-seat binds.
5. `sola-river` connects to `sola-bus`, subscribes to topics.
6. Initial state: after binding `river_window_manager_v1`, any already-mapped windows are expected to arrive as `window` events before (or during) the first manage sequence — standard Wayland global-binding behavior. `sola-river` mints IDs for each and publishes an initial `Apps`. Verify exact replay semantics against River behavior during implementation; if replay does not happen at bind time, issue a no-op `manage_start` / `manage_commit` to trigger it.

### 7.2 Shutdown

- SIGTERM to `sola-river`: send SIGTERM to River, wait up to 2s, SIGKILL if needed, exit 0.
- River exits first (crash / unexpected): `sola-river` detects via child fd, logs error, exits non-zero. `sola` respawns under standard backoff.
- `sola` dies: kernel sends SIGTERM to `sola-river` via `PR_SET_PDEATHSIG`. `sola-river` in turn propagates to River.

### 7.3 Changes required elsewhere

**`sola-bus/topics.rs`:**
- Remove `SetWindowPolicy`.
- Add `RegisteredChords`, `Chord`, `MouseEntered`, `MouseLeft`, `MouseClicked`.

**`sola-shell`:**
- Publish `RegisteredChords` as sticky whenever its chord list changes. Chord list = chords extracted from `SetAppMenu` entries (menu shortcuts) + hardcoded shell chords (Super+Tab, Super+Space, Super+arrows).
- Listen for `Chord`; match against shortcut table; emit `MenuAction` as today.
- Listen for `MouseEntered` → emit `Focus(window_id)` (focus-follows-mouse policy). `MouseLeft` → no-op (leave focus where it is). `MouseClicked` → emit `Focus(window_id)` + re-emit `Composition` with clicked window on top.
- Track `HashMap<AppId, WindowId>` MRU from own `Focus` emissions (not from a compositor echo). Use when activating an app (e.g., Super+Tab result): look up the app's MRU window, emit `Focus` for it.
- Stop emitting `SetWindowPolicy`.
- On-map focus decision: observe `Apps`; if the newly-appeared window belongs to the currently-active app, emit `Focus` for it.

**`sola-terminal`, `sola-monitor`:** remove `SetWindowPolicy` emission.

**`crates/sola/src/main.rs`:** change `MANAGED` from `["sola-bus", "sola-compositor", ...]` to `["sola-bus", "sola-river", ...]`.

**`crates/sola-make`:** add `sola-river` as a build/deploy target; remove `sola-compositor`.

**Deletion:** remove `crates/sola-compositor/` entirely.

## 8. Registries

### 8.1 `WindowRegistry`

```rust
pub struct WindowRegistry {
    next_id: u32,
    by_id: HashMap<u32, RiverWindow>,
    by_object: HashMap<ObjectId, u32>,  // river_window_v1 object id → our u32
}

struct RiverWindow {
    window: river_window_v1,
    node: Option<river_node_v1>,
    app_id: Option<String>,
    title: Option<String>,
    state: WindowState,
}

enum WindowState {
    PendingFirstDimensions,   // seen, no size yet
    Sized,                    // first dimensions proposed
    Placed,                   // position set at least once
    Visible,                  // render_commit landed
}
```

u32 allocation is monotonic. IDs are never reused during `sola-river`'s lifetime. A restart produces fresh IDs — apps and shell reconcile from the next `Apps` emission.

### 8.2 `ChordRegistry`

```rust
pub struct ChordRegistry {
    by_chord: HashMap<Chord, river_xkb_binding_v1>,
}
```

On `RegisteredChords` bus update, diff old/new sets; create bindings for additions, destroy for removals.

## 9. Error handling

- **Bus disconnect:** `sola-river` keeps running. `BusClient` auto-reconnects. River and its clients are unaffected. Sticky topics (`Apps`, `RegisteredChords`) replay on reconnect per standard bus behavior.
- **Wayland protocol error from River:** logged as error including the object/request in question. If recoverable (e.g., single-window state desync), drop that window from the registry and wait for River's `closed` event. If unrecoverable (e.g., connection-level), exit; `sola` respawns.
- **Bus topic referencing an unknown window_id** (e.g., shell `Frame` for a window `sola-river` hasn't registered yet): queue the update in `PendingUpdate` with a 100ms expiry. If the window appears within 100ms, apply the update in its first manage/render pair. Otherwise drop and log a warning. Covers the race where the shell reacts to its own stale `Apps` faster than `sola-river`'s side has settled.
- **River child dies:** detected on child fd. Log, exit non-zero, let `sola` respawn.
- **Socket timeout on startup:** 30s cap; log, exit non-zero, let `sola` respawn. A persistent failure indicates River itself is broken; `sola`'s backoff prevents a respawn storm.

## 10. Logging

Standard Sola pattern: `tracing` with structured fields, stderr + `/opt/sola/log/sola-river.log`. River's stdout/stderr is captured by `sola-river` and redirected to `/opt/sola/log/river.log` (separate file — River logs at its own level, independent of ours).

Key log events:
- `info`: startup, socket found, globals bound, river exit, shutdown.
- `warn`: bus topic for unknown window_id (+ whether queued), protocol error on a single window.
- `error`: river crash, connection-level protocol error, bus topic of invalid shape.
- `debug`: per-topic traffic, manage/render sequence summaries.

## 11. Rollout

One shot in this worktree:

1. Scaffold `crates/sola-river/` with full module layout.
2. Implement all modules (supervisor, client/*, bus, translator, registry).
3. Delete `crates/sola-compositor/`.
4. Update `sola-bus` topics (delete `SetWindowPolicy`, add the new five).
5. Update `crates/sola/src/main.rs` (`MANAGED` list).
6. Update `crates/sola-make` build/install targets.
7. Refactor `sola-shell`: chords via bus, MRU tracking, mouse handling, no SetWindowPolicy.
8. Strip `SetWindowPolicy` emission from `sola-terminal` and `sola-monitor`.
9. `cargo make install`; launch `sola` from a TTY; exercise the stack.

If anything fundamental doesn't work, `git worktree remove .worktrees/sola-river` and we're back on master with today's compositor intact.

## 12. Future work (explicitly out of scope)

- Multi-output zoning (requires shell changes to become output-aware).
- Layer-shell integration — when launcher / lock / panels are built. Using standard `wlr-layer-shell-unstable-v1` if River exposes it; otherwise River's `river-layer-shell-v1` with a shim.
- Click-to-focus tuning: whether `MouseClicked` should change focus if focus is already "somewhere," behavior when clicking decorations, etc.
- Raise-on-click policy: currently shell re-emits `Composition`; may want finer-grained z-order rules.
- Declarative libinput config via `river-libinput-config-v1` / `river-xkb-config-v1` (if we want a settings surface for input).
- Decoration negotiation for foreign xdg-shell clients (currently borders off; CSD/SSD handshake via xdg_decoration is unchanged and River honors it).
- IME / text input / tablet / gestures — all available via standard protocols when needed.
- Output hotplug and HiDPI handling in the shell's zone model.
- Session restore: restoring window geometry / app placement across a full sola restart.

## 13. Appendix: protocol references

- `river-window-management-v1` v4 (stable, MIT): https://raw.githubusercontent.com/riverwm/river/master/protocol/river-window-management-v1.xml
- `river-xkb-bindings-v1` v2: https://raw.githubusercontent.com/riverwm/river/master/protocol/river-xkb-bindings-v1.xml
- River 0.4.2 on Arch extra: https://archlinux.org/packages/extra/x86_64/river/
- Isaac Freund, "Separating the Wayland Compositor and Window Manager" (Mar 2026): https://isaacfreund.com/blog/river-window-management/
