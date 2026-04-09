# Seamless Compositor Restart

**Date:** 2026-04-09
**Status:** Proposed

## Goal

Restart Sola (e.g. after a binary deploy) without killing running applications. Clients should survive the restart and reconnect to the new compositor instance.

## Background

Sola already has a binary watcher (`backend/watcher.rs`) that detects when the deployed binary changes and calls `execv()` to replace the running process. Currently this kills all clients because:

1. The Wayland listening socket is owned by the compositor and closed on exit
2. XWayland is a child process that dies with the compositor
3. No compositor state (window positions, focus) is preserved

## Approach: Three Phases

### Phase 1: Wayland Socket Handoff

**Goal:** Native Wayland clients survive restart.

**Mechanism:** File descriptors survive `execv()` unless marked `FD_CLOEXEC`. If the Wayland listening socket FD is kept open across the exec, existing client connections remain valid. The new compositor instance accepts the inherited FD instead of creating a fresh socket.

**Prior art:** Sway (PR #8259), Hyprland, KWin, and the `wl-restart` tool by Ferdi265 all use this pattern.

**Implementation:**

1. **Accept a socket FD via CLI.** Add `--wayland-fd <N>` to Sola's CLI. When provided, use this FD as the Wayland listening socket instead of creating one.

2. **Clear FD_CLOEXEC before exec.** In the binary watcher's restart path, before calling `execv()`:
   - Identify the listening socket FD
   - Clear `FD_CLOEXEC` on it: `fcntl(fd, F_SETFD, 0)`
   - Pass it to the new process via `--wayland-fd <N>` in the exec args

3. **Smithay integration.** Currently we use `ListeningSocketSource::with_name("wayland-0")`. We need to check if Smithay supports `from_fd()` or similar. If not, use `wl_display_add_socket_fd()` from `wayland-sys` directly.

4. **Client reconnection.** Clients that support reconnection (Qt with `QT_WAYLAND_RECONNECT=1`, GTK 4.x) will automatically recover. Others may need a reconnect signal or will need to be restarted.

**What to test:** Run `sola-wtest`, trigger a restart via binary watcher, confirm the wtest window survives and continues receiving events.

### Phase 2: XWayland Decoupling

**Goal:** X11 clients (Steam, etc.) survive restart.

**Problem:** XWayland is currently spawned as a child process of the compositor. When the compositor exits (even via `execv()`), XWayland terminates and takes all X11 clients with it.

**Approach:** Decouple XWayland's lifecycle from the compositor.

Two options:

**Option A: External XWayland process.**
- Run XWayland as a separate process (systemd user service or launched by a session wrapper)
- Compositor connects to the existing XWayland instance on startup
- Compositor disconnecting doesn't kill XWayland
- KWin uses this pattern ("Survive Xwayland crashes")

**Option B: Reparent XWayland before exec.**
- Before `execv()`, reparent the XWayland child process (e.g. to PID 1) so it isn't killed
- New compositor reconnects to the existing XWayland
- Simpler than Option A but more fragile

**Recommendation:** Option A. It's the proven pattern and also makes Sola resilient to XWayland crashes (not just restarts).

**What to test:** Run `sola-xtest` via XWayland, trigger a restart, confirm the xtest window survives.

### Phase 3: State Serialization

**Goal:** Restore window layout, focus, and workspace state after restart.

**Mechanism:** Before shutdown, serialize compositor state to a file. On startup, read it back and restore.

**State to preserve:**
- Window positions and sizes (per-output)
- Focus state (which window had keyboard/pointer focus)
- Workspace assignments (once workspaces exist)
- Output configuration (which output is primary, layout)

**Storage:** JSON file at `$XDG_STATE_HOME/sola/session.json` (or `/opt/sola/state/session.json` on canto).

**Implementation:**
1. Before `execv()`, iterate the Space and serialize each window's geometry, title, app_id/class
2. On startup, if `--restore` flag is set (or always after an exec restart), read the state file
3. As clients reconnect and create surfaces, match them to saved state by app_id/class and restore position

**What to test:** Arrange windows, trigger restart, confirm windows reappear in the same positions.

## Considerations

### DRM/KMS
DRM master status reverts to the seat manager on exit. The new process re-acquires it via libseat. GPU buffers are freed and reallocated — clients re-render their first frame. No special handling needed.

### libseat
Session re-acquisition via `LibSeatSession::new()` works cleanly after `execv()`. The seat manager tracks DRM master ownership and grants it to the new process.

### Client compatibility
Not all Wayland clients support reconnection. This is a toolkit-level feature:
- **Qt 6:** Supports reconnection with `QT_WAYLAND_RECONNECT=1`
- **GTK 4:** Has some reconnection support
- **XWayland:** Must survive independently (Phase 2)
- **Others:** May crash on disconnect regardless

### Binary watcher interaction
The existing binary watcher calls `execv()` directly. Phase 1 modifies this path to preserve the socket FD and pass it via CLI args. The watcher doesn't need fundamental changes — just the pre-exec FD handling.

## Test Plan

- `sola-wtest` (native Wayland) — verify survives restart (Phase 1)
- `sola-xtest` (X11 via XWayland) — verify survives restart (Phase 2)
- Steam — verify survives restart (Phase 2)
- Window positions — verify restored after restart (Phase 3)
