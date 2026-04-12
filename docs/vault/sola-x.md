# sola-x

**Crate:** `crates/sola-x/` (in progress, `.worktrees/sola-x`)
**Binary:** `sola-x`
**Role:** XWayland host — manages the XWayland process independently of the compositor, bridging X11 windows as proxy Wayland surfaces.

## Why Separate

X11 apps (like Steam) should survive compositor restarts. By hosting XWayland in its own process, X11 apps maintain their connection to sola-x while the compositor restarts. sola-x then reconnects its proxy surfaces to the new compositor.

## Architecture

```
X11 app (Steam)
  ↓ X11 protocol
XWayland
  ↓ Wayland protocol (connects as client)
sola-x (minimal Wayland compositor)
  ↓ Wayland protocol (connects as client)
sola-compositor (main display)
```

sola-x is both a Wayland server (for XWayland) and a Wayland client (of sola-compositor). It bridges X11 windows as proxy surfaces.

## Status

- **Phase 1 (complete):** Server-side XWayland hosting. Wayland socket, XWayland spawn, X11 window tracking.
- **Phase 2 (stubbed):** Client connection to sola-compositor. Proxy surface creation, input forwarding.
- **Phase 3 (not started):** State serialization for seamless restart.

## Bus Integration (planned)

Not yet implemented. Will need:
- Connect as a [[Sola Bus]] client
- Publish X11 window lifecycle events
- Receive input forwarding from compositor
- Listen for compositor restart signals

## Source Files

| File | Purpose |
|---|---|
| `src/main.rs` | Wayland socket setup, XWayland spawn, event loop |
| `src/state.rs` | `SolaX` state struct, WindowBridge tracking |
| `src/error.rs` | Error types |
| `src/server/mod.rs` | Wayland protocol delegates |
| `src/server/compositor.rs` | Surface commits for XWayland |
| `src/server/seat.rs` | Keyboard/pointer (stub) |
| `src/server/shm.rs` | Shared memory buffers |
| `src/server/xwayland.rs` | XWayland lifecycle, X11 window handling |
| `src/client/mod.rs` | Client connection to sola (stub) |
