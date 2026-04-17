# Move XWayland into the Compositor

## Problem

sola-x acts as a separate mini-compositor for XWayland, forwarding buffers to sola-compositor by dup'ing dmabuf file descriptors and re-creating them on a second Wayland connection. This double-hop causes intermittent `eglCreateImageKHR` failures on NVIDIA GPUs that corrupt the EGL context's fence synchronization state, freezing the entire desktop. No other Wayland compositor uses this architecture — they all host XWayland directly.

## Solution

Delete the `sola-x` crate. Move XWayland hosting into `sola-compositor`. X11 windows become first-class surfaces in the compositor's Space, rendered directly — no buffer forwarding, no fd duplication, no separate EGL context.

## Architecture

### New module: `src/xwayland/`

All XWayland code lives in a dedicated module directory within the compositor.

**`mod.rs`** — Spawns XWayland on the compositor's display, registers the calloop event source, sets `DISPLAY` env var when ready. Delegates to Smithay's `XWayland::spawn()` with the compositor's `DisplayHandle`.

**`xwm.rs`** — `XwmHandler` implementation for the compositor's `State`. Core responsibilities:

- `map_window_request`: Set window mapped, push to `pending_surfaces` via the existing flow. Emit `WindowPolicy` on the bus so the shell can compose it.
- `unmapped_window` / `destroyed_window`: Remove from tracking, emit updated `Apps` list.
- `configure_request`: Apply geometry from the X11 client.
- `mapped_override_redirect_window`: Handle popups/menus (tracked separately from managed windows).

X11 windows enter the compositor's `pending_surfaces` → `unmapped_surfaces` → `Space` pipeline, exactly like native Wayland toplevels. The shell sees them via `Apps` and controls them via `Composition`/`Frame`/`Focus`.

**`xwayland_shell.rs`** — `XWaylandShellHandler` implementation. Handles `surface_associated` — maps the X11 window ID to its wl_surface so the XwmHandler can find the right Window.

### State changes

New fields on the compositor's `State`:

```rust
// -- XWayland --
xwm: Option<X11Wm>,
xwayland_shell_state: Option<XWaylandShellState>,
```

No bridge state, no client connection, no proxy surfaces, no frame callback stashing.

### Dmabuf handling

XWayland's dmabufs are imported directly into the compositor's EGL context — single hop, standard path. The `dmabuf_imported` handler validates via `renderer.import_dmabuf()` as originally intended. Failed imports correctly send `Failed` back to XWayland, which falls back to a different format.

### Process manager changes

Remove `"sola-x"` from the `MANAGED` list in `crates/sola/src/main.rs`. XWayland is a child of the compositor process, managed by Smithay's `XWayland` event source.

### Cargo workspace changes

- Remove `sola-x` from `Cargo.toml` workspace members.
- Add XWayland-related Smithay features to `sola-compositor`'s `Cargo.toml` (the `xwayland` feature, `x11rb` dependency).
- Delete `crates/sola-x/` directory.

## What does NOT change

- **Shell code** — X11 apps appear via `Apps`/`Composition`/`Focus` bus topics, same as before.
- **Bus topics** — No new topics. `SetWindowPolicy`, `Apps`, `Composition`, `Frame`, `Focus` all work as-is.
- **Input handling** — The compositor's existing seat handles keyboard/pointer for X11 surfaces directly. No input forwarding needed.
- **Rendering** — X11 surfaces are rendered by Smithay's Space renderer like any other Window.
- **Build system** — `cargo make build` / `cargo make deploy` work as before, minus the sola-x target.

## Migration mapping

| sola-x file | Destination | Notes |
|---|---|---|
| `server/xwayland.rs` | `xwayland/mod.rs` + `xwayland/xwm.rs` | XWayland spawn + XwmHandler |
| `server/compositor.rs` | Deleted | Buffer forwarding gone |
| `server/seat.rs` | Deleted | Compositor's seat handles X11 directly |
| `server/shm.rs` | Deleted | Compositor's SHM handles X11 directly |
| `server/dmabuf.rs` | Deleted | Compositor's dmabuf handles X11 directly |
| `server/mod.rs` | Deleted | Protocol glue gone |
| `client/mod.rs` | Deleted | No second Wayland connection |
| `bridge.rs` | Deleted | No buffer forwarding |
| `state.rs` | Deleted | Fields absorbed into compositor State |
| `main.rs` | Deleted | No separate process |

## Success criteria

- Steam runs without EGL corruption or desktop freezes.
- Brave runs as before.
- X11 window menus/popups (override-redirect) are visible.
- Shell composes X11 windows via Composition (same as today).
- `cargo make deploy` no longer builds or deploys sola-x.
