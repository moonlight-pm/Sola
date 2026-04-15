# Composition Authority Design

**Date:** 2026-04-15

## Overview

The shell becomes the single authority on what's visible, where, how big, and what has focus. The compositor is a pure rendering and input engine. Three independent bus topics carry the shell's decisions to the compositor, each applied immediately on arrival.

## Bus Topics (Shell → Compositor)

### Composition

The z-ordered list of visible surfaces. Surfaces not in the list are unmapped (hidden, not rendered, no input).

```rust
pub struct CompositionEntry {
    pub app_id: String,
    pub title: Option<String>,
}

// Topic::Composition(Vec<CompositionEntry>)
```

Bottom to top. Title is optional — `None` matches any window from that app. When this arrives, the compositor unmaps surfaces not in the list and maps/reorders surfaces to match.

### Frame

Per-surface position and size. Applied immediately — can target mapped or unmapped surfaces (pre-configuring an unmapped surface so it's ready when Composition adds it).

```rust
pub struct FrameUpdate {
    pub app_id: String,
    pub title: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

// Topic::Frame(FrameUpdate)
```

The compositor configures the surface with the given size (sends xdg_toplevel configure) and maps it at the given position. The Wayland client receives the size through the standard configure event — no bus round-trip needed for the app to know its draw area.

### Focus

Which surface receives keyboard focus.

```rust
pub struct FocusTarget {
    pub app_id: String,
    pub title: Option<String>,
}

// Topic::Focus(FocusTarget)
```

The compositor calls `keyboard.set_focus()` for the matching surface.

## Independence

These three topics are independent axes. Each applies immediately as it arrives. Common operations:

- **Zone snap** → one `Frame` for the snapped window.
- **App switch** → `Focus` + `Composition` (reorder MRU).
- **Switcher opens** → `Composition` (add switcher panel on top).
- **New window appears** → `Frame` (geometry) + `Composition` (add to list) + `Focus`.
- **Output resize** → `Frame` for each affected surface.

## Compositor Behavior

### Applying Composition

1. Walk the list. For each entry, find the matching window via `window_by_app_id_title`.
2. Unmap all currently-mapped surfaces not in the list.
3. Map surfaces in list order (bottom to top) at their current positions.
4. Entries referencing surfaces that don't exist yet are silently skipped.

### Applying Frame

1. Find the matching window.
2. Configure with width/height (send xdg_toplevel configure).
3. Map at (x, y) position.

### Applying Focus

1. Find the matching window.
2. Call `keyboard.set_focus()`.
3. Do NOT emit FocusChanged — the shell already knows the focus it set.

### What the Compositor Still Does

- Accepts Wayland connections, tracks surfaces through the protocol lifecycle.
- Buffers new surfaces in `pending_surfaces` until app_id is known.
- Emits `Topic::Apps` whenever the surface list changes (window map/unmap).
- Emits `Topic::FocusChanged` only for compositor-initiated focus changes (future: click-to-focus). Never when applying Focus from the shell.
- Routes Super+key events to the shell's `keyboard_target` surface via `wl_keyboard.key`.
- Emits `Topic::OutputGeometry` on output hotplug/mode change.

### What Gets Removed from Compositor

- `auto_focus` logic in `apply_pending_surfaces` — shell controls focus.
- `raise_element` / `raise_shell_surfaces` — shell controls z-order.
- `handle_raise_app` — replaced by Composition.
- `handle_set_window_geometry` — replaced by Frame.
- `apply_pending_geometries` / `pending_geometries` — no longer needed.
- `sync_mru` — shell tracks MRU via FocusChanged.
- `emit_apps_list` from `focus_changed` — replaced by shell-driven composition.

## Shell Behavior

The shell maintains a complete picture of the scene from:

- **WindowPolicy** from apps — their preferences (size, position, zoned, keyboard_target).
- **Apps** from compositor — which surfaces exist.
- **FocusChanged** from compositor — for MRU tracking.
- **OutputGeometry** — screen dimensions.
- **Shell's own state** — zone assignments, session persistence, active switcher, open menus.

On state changes, the shell emits the appropriate topics:

- `Apps` received (new window) → `Frame` + `Composition` + `Focus`
- `Apps` received (window removed) → `Composition`
- `FocusChanged` received → update MRU, `Composition` + `Focus`
- `OutputGeometry` received → `Frame` for all surfaces, `Composition`
- Switcher activated → `Composition` (add panel)
- Switcher deactivated → `Composition` (remove panel) + `Focus`
- Zone snap → `Frame` for snapped window
- Menu opened → `Composition` (add panel)
- Menu closed → `Composition` (remove panel)

### Composition Computation

The shell builds the surface list bottom to top:

1. **Shell menubar** — always present, at its declared position/size.
2. **App windows** — ordered by MRU (most recent on top). Zoned apps get zone geometry. Non-zoned apps get their declared size/position. Apps without a policy get the full output size.
3. **Shell panels** (switcher, menu dropdowns) — on top when active, absent when inactive.

## Surface Lifecycle

1. App creates a Wayland surface, sets app_id and title.
2. Compositor detects the surface in `apply_pending_surfaces`, emits updated `Apps` list. Surface stays unmapped.
3. Shell receives `Apps`, sees the new surface, emits `Frame` (geometry from app's WindowPolicy or defaults) + `Composition` (including the new surface) + `Focus`.
4. Compositor applies each as it arrives: configures, maps, orders, focuses.

Between steps 2 and 4, the surface is invisible. No fallback — if the shell is not running, no surfaces appear.

## Non-Sola Apps

Regular Wayland clients and X11 apps (via sola-x) appear in the `Apps` list with their app_id. They have no WindowPolicy, so the shell uses defaults:

- Full output size, position (0, 0).
- Zoning available via Super+Numpad like any other app.
- Included in MRU ordering.
- Referenced by `(app_id, None)` which matches any window from that app.

## WindowPolicy Changes

WindowPolicy remains advisory — apps declare preferences, the shell reads them.

```rust
pub struct WindowPolicy {
    pub title: String,
    pub zoned: bool,
    pub keyboard_target: bool,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
}
```

- `auto_focus` is removed — the shell controls focus.
- `keyboard_target` stays — the compositor needs it for Super+key routing.
- `zoned`, `size`, `position` stay — the shell uses them to compute geometry.

## Bus Topic Changes

### Added

- `Composition(Vec<CompositionEntry>)` — z-order and visibility.
- `Frame(FrameUpdate)` — per-surface position and size.
- `Focus(FocusTarget)` — keyboard focus target.

### Removed

- `SetWindowGeometry` — replaced by Frame.
- `RaiseApp` — replaced by Composition + Focus.

### Unchanged

- `Apps`, `FocusChanged`, `OutputGeometry` — compositor still emits these.
- `SetWindowPolicy` — apps still declare preferences.
- `SetAppMenu`, `MenuAction` — menu system unchanged.
- `Shutdown`, `LaunchApp` — lifecycle unchanged.

## Shell Surface Model

The full-screen overlay surface is eliminated. The shell creates purpose-specific surfaces:

- **Menubar** — persistent, declared size (output_width × 28), at bottom of composition (coverable by fullscreen apps).
- **Switcher panel** — created at startup, absent from Composition until Super+Tab activates it. Sized to content, centered. Added on top when active.
- **Menu panel** — created at startup, absent from Composition until a menu is clicked. Positioned below the menubar, sized to content. Added on top when active.

Each surface has its own WindowPolicy with `keyboard_target: false` (only the menubar has `keyboard_target: true`).

## sola-x Impact

sola-x currently emits and handles `SetWindowGeometry` for X11 window resizing. With SetWindowGeometry removed, X11 geometry flows through Frame: the compositor configures sola-x's proxy surfaces based on Frame, and sola-x applies those configures to the X11 windows (this path already works). For X11-initiated resizes, sola-x reports the new size to the shell (mechanism TBD — may need a new topic or reuse of Apps metadata).
