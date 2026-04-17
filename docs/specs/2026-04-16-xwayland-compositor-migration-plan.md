# XWayland Compositor Migration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move XWayland from the separate sola-x process into sola-compositor so X11 apps use direct dmabuf import (single hop) instead of the forwarding bridge that corrupts NVIDIA EGL.

**Architecture:** XWayland spawns as a child of the compositor. X11 windows become `Window::new_x11_window()` elements in the existing `Space<Window>`. The shell sees them via `Apps`/`Composition` exactly as before. The entire sola-x crate, bridge layer, and client-side Wayland connection are deleted.

**Tech Stack:** Smithay 0.7 (xwayland feature), X11Wm, XwmHandler, XWaylandShellHandler

**Key insight:** Smithay's `Window` type already wraps both Wayland toplevels and X11 surfaces when the `xwayland` feature is enabled. `Window::new_x11_window(surface)` produces a Window that works in `Space<Window>`, renders via the same pipeline, and provides `wl_surface()`. No custom enum needed.

---

### Task 1: Add xwayland feature to sola-compositor

**Files:**
- Modify: `crates/sola-compositor/Cargo.toml`

- [ ] **Step 1: Add the xwayland Smithay feature**

In `crates/sola-compositor/Cargo.toml`, add `"xwayland"` to the smithay features list:

```toml
smithay = { version = "0.7.0", default-features = false, features = [
    "backend_drm",
    "backend_gbm",
    "backend_egl",
    "backend_libinput",
    "backend_udev",
    "backend_session_libseat",
    "renderer_gl",
    "renderer_multi",
    "wayland_frontend",
    "desktop",
    "xwayland",
] }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p sola-compositor`
Expected: compiles with warnings about unused xwayland types (that's fine, we'll use them next)

- [ ] **Step 3: Commit**

```
git add crates/sola-compositor/Cargo.toml
git commit -m "feat(compositor): enable smithay xwayland feature"
```

---

### Task 2: Update window_app_id and window_title for X11 surfaces

**Files:**
- Modify: `crates/sola-compositor/src/state.rs`

X11 windows don't have XdgToplevelSurfaceData. The current `window_app_id` and `window_title` return `None` for them. They need to check for X11 surfaces and use `class()` / `title()`.

- [ ] **Step 1: Update window_app_id**

Replace the `window_app_id` function in `crates/sola-compositor/src/state.rs`:

```rust
fn window_app_id(window: &Window) -> Option<String> {
    // X11 windows use class as app_id.
    if let Some(x11) = window.x11_surface() {
        let class = x11.class();
        return if class.is_empty() { None } else { Some(class) };
    }

    // Wayland toplevels use xdg_toplevel app_id.
    use smithay::wayland::compositor::with_states;
    use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;

    window.toplevel().and_then(|toplevel| {
        with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|data| data.lock().ok())
                .and_then(|attrs| attrs.app_id.clone())
        })
    })
}
```

- [ ] **Step 2: Update window_title**

Replace the `window_title` function:

```rust
pub fn window_title(window: &Window) -> Option<String> {
    // X11 windows use the X11 title property.
    if let Some(x11) = window.x11_surface() {
        let title = x11.title();
        return if title.is_empty() { None } else { Some(title) };
    }

    // Wayland toplevels use xdg_toplevel title.
    use smithay::wayland::compositor::with_states;
    use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;

    window.toplevel().and_then(|toplevel| {
        with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|data| data.lock().ok())
                .and_then(|attrs| attrs.title.clone())
        })
    })
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p sola-compositor`

- [ ] **Step 4: Commit**

```
git add crates/sola-compositor/src/state.rs
git commit -m "feat(compositor): support X11 window app_id and title"
```

---

### Task 3: Add XWayland state fields and xwayland module

**Files:**
- Modify: `crates/sola-compositor/src/state.rs`
- Modify: `crates/sola-compositor/src/lib.rs`
- Create: `crates/sola-compositor/src/xwayland/mod.rs`

- [ ] **Step 1: Add XWayland fields to State**

In `crates/sola-compositor/src/state.rs`, add imports and fields:

```rust
// Add to imports at top:
use smithay::wayland::xwayland_shell::XWaylandShellState;
use smithay::xwayland::X11Wm;

// Add to State struct:
    // -- XWayland --
    /// X11 window manager. Set when XWayland connects and sends Ready.
    pub xwm: Option<X11Wm>,
    /// XWayland shell protocol state.
    pub xwayland_shell_state: Option<XWaylandShellState>,
```

Initialize both as `None` in `State::new()`.

- [ ] **Step 2: Create the xwayland module directory**

Create `crates/sola-compositor/src/xwayland/mod.rs`:

```rust
//! XWayland integration — spawns XWayland and handles X11 windows.
//!
//! X11 windows are wrapped as `Window::new_x11_window()` and enter
//! the compositor's standard `pending_surfaces` → `Space` pipeline.

pub mod xwm;

use smithay::reexports::calloop::EventLoop;
use smithay::wayland::xwayland_shell::XWaylandShellState;
use smithay::xwayland::XWayland;

use crate::state::State;

/// Spawn XWayland and register its event source with the event loop.
pub fn setup(
    state: &mut State,
    event_loop: &EventLoop<'static, State>,
) {
    state.xwayland_shell_state = Some(XWaylandShellState::new::<State>(&state.display_handle));

    let (xwayland, xwayland_client) = match XWayland::spawn(
        &state.display_handle,
        None,
        std::iter::empty::<(String, String)>(),
        true,
        std::process::Stdio::null(),
        std::process::Stdio::null(),
        |_| {},
    ) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!("failed to spawn XWayland: {e}");
            return;
        }
    };

    if let Err(e) = event_loop
        .handle()
        .insert_source(xwayland, move |event, _, state| match event {
            smithay::xwayland::XWaylandEvent::Ready {
                x11_socket,
                display_number,
            } => {
                tracing::info!(display_number, "XWayland ready");
                unsafe { std::env::set_var("DISPLAY", format!(":{display_number}")) };

                match smithay::xwayland::X11Wm::start_wm(
                    state.loop_handle.clone(),
                    x11_socket,
                    xwayland_client.clone(),
                ) {
                    Ok(wm) => {
                        state.xwm = Some(wm);
                        tracing::info!("X11 window manager started");
                    }
                    Err(err) => {
                        tracing::error!(?err, "failed to start X11 window manager");
                    }
                }
            }
            smithay::xwayland::XWaylandEvent::Error => {
                tracing::error!("XWayland failed to start");
            }
        })
    {
        tracing::error!("failed to register XWayland event source: {e}");
    }
}
```

- [ ] **Step 3: Register the module in lib.rs**

Add to `crates/sola-compositor/src/lib.rs`:

```rust
pub mod xwayland;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p sola-compositor`

- [ ] **Step 5: Commit**

```
git add crates/sola-compositor/src/state.rs crates/sola-compositor/src/lib.rs crates/sola-compositor/src/xwayland/
git commit -m "feat(compositor): add XWayland state and spawn logic"
```

---

### Task 4: Implement XwmHandler and XWaylandShellHandler

**Files:**
- Create: `crates/sola-compositor/src/xwayland/xwm.rs`

This is the core of the migration. X11 windows become `Window::new_x11_window()` elements, enter `pending_surfaces`, and flow through the existing composition pipeline.

- [ ] **Step 1: Create xwm.rs with XwmHandler**

Create `crates/sola-compositor/src/xwayland/xwm.rs`:

```rust
//! X11 window manager handler — maps X11 windows into the compositor's Space.
use smithay::desktop::Window;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{Reorder, ResizeEdge, X11Window, XwmHandler, XwmId};
use smithay::xwayland::X11Surface;

use crate::lifecycle::emit_apps_list;
use crate::state::State;

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut smithay::xwayland::X11Wm {
        self.xwm.as_mut().expect("xwm not initialized")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(
            title = %window.title(),
            class = %window.class(),
            "X11 window map request"
        );

        if let Err(err) = window.set_mapped(true) {
            tracing::error!(?err, "failed to set X11 window mapped");
            return;
        }

        // X11 window enters the standard pending_surfaces flow.
        // It will be picked up by apply_pending_surfaces once the
        // wl_surface is associated (surface_associated callback).
        let win = Window::new_x11_window(window);
        self.pending_surfaces.push(win);
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(
            title = %window.title(),
            class = %window.class(),
            "X11 override-redirect window mapped"
        );

        // OR windows (menus, popups, tooltips) are mapped directly
        // into the space at their X11-requested position, above
        // everything else.
        let geo = window.geometry();
        let win = Window::new_x11_window(window);
        self.space.map_element(win, (geo.loc.x, geo.loc.y), true);
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(title = %window.title(), "X11 window unmapped");
        remove_x11_window(self, &window);
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        remove_x11_window(self, &window);
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let geo = window.geometry();
        let new_geo = Rectangle::new(
            (x.unwrap_or(geo.loc.x), y.unwrap_or(geo.loc.y)).into(),
            (
                w.map(|v| v as i32).unwrap_or(geo.size.w),
                h.map(|v| v as i32).unwrap_or(geo.size.h),
            )
                .into(),
        );

        if let Err(err) = window.configure(Some(new_geo)) {
            tracing::error!(?err, "failed to configure X11 window");
        }
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _geometry: Rectangle<i32, Logical>,
        _above: Option<X11Window>,
    ) {
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _button: u32,
        _resize_edge: ResizeEdge,
    ) {
    }

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {}
}

impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        self.xwayland_shell_state
            .as_mut()
            .expect("xwayland_shell_state not initialized")
    }

    fn surface_associated(
        &mut self,
        _xwm: XwmId,
        _wl_surface: smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        surface: X11Surface,
    ) {
        tracing::info!(
            title = %surface.title(),
            class = %surface.class(),
            "X11 surface associated"
        );
    }
}

/// Remove an X11 window from all tracked collections.
fn remove_x11_window(state: &mut State, surface: &X11Surface) {
    // Remove from space (mapped windows).
    let found = state.space.elements().find(|w| {
        w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id())
    }).cloned();

    if let Some(window) = found {
        state.space.unmap_elem(&window);
    }

    // Remove from pending/unmapped.
    state.pending_surfaces.retain(|w| {
        !w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id())
    });
    state.unmapped_surfaces.retain(|w| {
        !w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id())
    });

    emit_apps_list(state);
}

smithay::delegate_xwayland_shell!(State);
```

- [ ] **Step 2: Add xwm module to mod.rs**

The `pub mod xwm;` was already added in Task 3's mod.rs.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p sola-compositor`

- [ ] **Step 4: Commit**

```
git add crates/sola-compositor/src/xwayland/xwm.rs
git commit -m "feat(compositor): implement XwmHandler for direct X11 window management"
```

---

### Task 5: Wire XWayland into the compositor's main.rs and lifecycle

**Files:**
- Modify: `crates/sola-compositor/src/main.rs`

- [ ] **Step 1: Call xwayland::setup in the compositor's startup**

In `crates/sola-compositor/src/main.rs`, add the XWayland setup call after the state is created and the event loop is ready, but before entering the main loop. Find where `lifecycle::run_loop` is called and add before it:

```rust
crate::xwayland::setup(&mut state, &event_loop);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p sola-compositor`

- [ ] **Step 3: Commit**

```
git add crates/sola-compositor/src/main.rs
git commit -m "feat(compositor): spawn XWayland on compositor startup"
```

---

### Task 6: Restore dmabuf validation in the compositor

**Files:**
- Modify: `crates/sola-compositor/src/wayland/dmabuf.rs`

Now that XWayland connects directly, dmabuf imports are single-hop. Restore the proper validation.

- [ ] **Step 1: Restore import_dmabuf validation**

Replace the `dmabuf_imported` handler in `crates/sola-compositor/src/wayland/dmabuf.rs`:

```rust
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::renderer::ImportDma;
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};

use crate::state::State;

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        self.dmabuf_state
            .as_mut()
            .expect("dmabuf_state not initialized")
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        let render_node = self.primary_render_node;

        match self.gpu_manager.single_renderer(&render_node) {
            Ok(mut renderer) => match renderer.import_dmabuf(&dmabuf, None) {
                Ok(_texture) => {
                    dmabuf.set_node(render_node);
                    let _ = notifier.successful::<State>();
                }
                Err(err) => {
                    tracing::debug!(?err, "dmabuf import failed");
                    notifier.failed();
                }
            },
            Err(err) => {
                tracing::error!(?err, "failed to get renderer for dmabuf import");
                notifier.failed();
            }
        }
    }
}

smithay::delegate_dmabuf!(State);
```

- [ ] **Step 2: Commit**

```
git add crates/sola-compositor/src/wayland/dmabuf.rs
git commit -m "fix(compositor): restore dmabuf validation now that XWayland is direct"
```

---

### Task 7: Update apply_pending_surfaces for X11 windows

**Files:**
- Modify: `crates/sola-compositor/src/lifecycle.rs`

The existing `apply_pending_surfaces` uses `window.toplevel()` for X11 shell keyboard target detection, and emits `WindowPolicy` based on xdg_toplevel data. X11 windows have no toplevel, so the function needs to handle them via `window.x11_surface()`.

- [ ] **Step 1: Update apply_pending_surfaces to handle X11 windows**

In `crates/sola-compositor/src/lifecycle.rs`, update `apply_pending_surfaces`. The function currently checks `State::app_id(&window)` to decide if a surface is "ready" — this now works for X11 thanks to Task 2. But the X11 wl_surface association can arrive after the Window is pushed to pending_surfaces. For X11, check `window.wl_surface().is_some()` as the readiness signal:

The existing logic moves surfaces from `pending_surfaces` to `unmapped_surfaces` when they have an app_id. For X11 windows, app_id (class) is known immediately from the XwmHandler, but the wl_surface may not be associated yet. Update the readiness check:

```rust
// In the loop over pending surfaces, replace the app_id check with:
let Some(app_id) = State::app_id(&window) else {
    still_pending.push(window);
    continue;
};

// X11 windows need their wl_surface associated before they're ready.
if window.x11_surface().is_some() {
    use smithay::wayland::seat::WaylandFocus;
    if window.wl_surface().is_none() {
        still_pending.push(window);
        continue;
    }
}
```

Also update the WindowPolicy emission for X11 windows — they don't have `window.toplevel()` for app_id detection. The existing code already reads app_id from `State::app_id()`, so just ensure the WindowPolicy path works when `toplevel()` returns None (skip the shell keyboard target check for X11 windows).

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p sola-compositor`

- [ ] **Step 3: Commit**

```
git add crates/sola-compositor/src/lifecycle.rs
git commit -m "feat(compositor): handle X11 windows in apply_pending_surfaces"
```

---

### Task 8: Remove sola-x from the process manager and workspace

**Files:**
- Modify: `crates/sola/src/main.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Remove sola-x from MANAGED list**

In `crates/sola/src/main.rs`, remove `"sola-x"` from the `MANAGED` array:

```rust
const MANAGED: &[&str] = &[
    "sola-bus",
    "sola-compositor",
    "sola-shell",
    "sola-terminal",
];
```

- [ ] **Step 2: Remove sola-x from workspace members**

Check `Cargo.toml` root — if it uses `members = ["crates/*", "apps/*"]` (glob), sola-x will be auto-excluded when deleted. If it lists members explicitly, remove the sola-x entry.

- [ ] **Step 3: Delete the sola-x crate**

```bash
rm -rf crates/sola-x
```

- [ ] **Step 4: Verify the full workspace builds**

Run: `cargo check`
Expected: all crates compile, no references to sola-x

- [ ] **Step 5: Commit**

```
git add -A
git commit -m "chore: remove sola-x crate — XWayland now lives in compositor"
```

---

### Task 9: Verify and clean up

- [ ] **Step 1: Full workspace build**

Run: `cargo check` from the worktree root.

- [ ] **Step 2: Grep for stale sola-x references**

```bash
grep -r "sola-x\|sola_x" crates/ apps/ --include="*.rs" --include="*.toml"
```

Remove any stale references (comments, imports, etc.).

- [ ] **Step 3: Verify deploy target list**

Run: `cargo make build` to confirm only the expected binaries are built (sola, sola-bus, sola-compositor, sola-shell, sola-terminal, plus apps).

- [ ] **Step 4: Final commit if needed**

```
git commit -m "chore: clean up stale sola-x references"
```
