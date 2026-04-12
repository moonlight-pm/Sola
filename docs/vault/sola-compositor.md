# sola-compositor

**Crate:** `crates/sola-compositor/`
**Binary:** `sola-compositor`
**Role:** Wayland compositor built on Smithay. Owns the display, input devices, and rendering pipeline.

## Design Principle

The compositor is "dumb" — it handles Wayland protocol, DRM/KMS, input routing, and rendering. It has no knowledge of what shell shortcuts mean, what a switcher is, or how apps should behave. All desktop personality lives in shell apps.

## What It Does

- **DRM/KMS** — direct display output via GPU
- **libinput** — keyboard, pointer, scroll input
- **Wayland server** — accepts client connections, manages surfaces
- **XWayland** — X11 app support (being moved to [[sola-x]])
- **[[Input Routing]]** — Super+key to bus, everything else to focused client
- **Bus message handling** — responds to [[Topics]] like `GrabInput`, `ReleaseInput`, `RaiseApp`
- **Rendering** — composites all surfaces via OpenGL ES (GlesRenderer)

## State

The central `State` struct holds all compositor state. Key fields:

- `bus: Option<BusClient>` — connection to [[Sola Bus]]
- `space: Space<Window>` — Smithay's window manager (z-order, positions)
- `input_grab: Option<String>` — app_id with exclusive input, if any
- `seat: Seat` — keyboard + pointer
- `devices: HashMap<DrmNode, Device>` — per-GPU state

## Bus Handlers

| Topic | Action |
|---|---|
| `GrabInput(app_id)` | Find window by app_id, raise, give keyboard focus |
| `ReleaseInput` | Clear grab, restore normal focus |
| `RaiseApp(app_id)` | Raise all windows of an app, focus topmost |
| `ListApps` | Respond with MRU app list (not yet implemented) |
| `FocusChanged` | Emit on focus change (not yet implemented) |

## Surface Identity

Windows are identified by `app_id` on their `xdg_toplevel` surface. Helpers:
- `state.window_by_app_id(target)` — find first window matching app_id
- `state.windows_by_app_id(target)` — find all windows matching app_id

For XWayland apps, `WM_CLASS` serves the same purpose.

## Source Files

| File | Purpose |
|---|---|
| `src/main.rs` | Binary entry point, logging setup |
| `src/lib.rs` | `run()` — initialization sequence |
| `src/state.rs` | `State` struct, `window_by_app_id` helpers |
| `src/lifecycle.rs` | Event loop, bus message dispatch, shutdown |
| `src/error.rs` | Error types per subsystem |
| `src/types.rs` | Type aliases for Smithay generics |
| `src/cursor.rs` | xcursor theme loading |

### Backend

| File | Purpose |
|---|---|
| `backend/input.rs` | libinput setup, Super-key-to-bus routing |
| `backend/session.rs` | libseat session creation |
| `backend/gpu.rs` | GPU discovery, GpuManager |
| `backend/udev.rs` | Device enumeration, DRM setup |
| `backend/device.rs` | Per-GPU device state |
| `backend/socket.rs` | Wayland listening socket |

### Wayland Protocol

| File | Protocol |
|---|---|
| `wayland/compositor.rs` | `wl_compositor` — surface creation/commit |
| `wayland/shell.rs` | `xdg_wm_base` — toplevel windows |
| `wayland/decoration.rs` | `xdg_decoration` — CSD vs SSD |
| `wayland/seat.rs` | `wl_seat` — input devices |
| `wayland/shm.rs` | `wl_shm` — shared memory buffers |
| `wayland/dmabuf.rs` | `zwp_linux_dmabuf` — GPU buffer sharing |
| `wayland/data.rs` | `wl_data_device` — clipboard, drag-and-drop |
| `wayland/output.rs` | `wl_output` + `xdg_output` — display info |
| `wayland/xwayland.rs` | XWayland lifecycle + X11 window management |

### Output

| File | Purpose |
|---|---|
| `output/render.rs` | Frame rendering, VBlank handling |
| `output/scan.rs` | DRM connector/CRTC scanning |

### Input

| File | Purpose |
|---|---|
| `input/binding.rs` | Modifier state tracking |
