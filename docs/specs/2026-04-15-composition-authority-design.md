# Composition Authority Design

**Date:** 2026-04-15

## Overview

The shell becomes the single authority on what's visible, where, how big, and what has focus. The compositor is a pure rendering and input engine — it applies the shell's composition exactly as described.

## UpdateComposition

A single bus topic from the shell that describes the complete scene:

```rust
pub struct CompositionSurface {
    pub app_id: String,
    pub title: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub struct Composition {
    pub surfaces: Vec<CompositionSurface>,
    pub focus: Option<(String, Option<String>)>,
}
```

- `surfaces`: z-order, bottom to top. Each entry identifies a surface by (app_id, title). Title is optional — `None` matches any window from that app.
- `focus`: which surface receives keyboard focus, identified by (app_id, title).
- Surfaces not in the list are unmapped (hidden, not rendered, no input).

## Compositor Behavior

When `UpdateComposition` arrives:

1. For each entry in the surfaces list, find the matching window via `window_by_app_id_title`.
2. Configure each surface with the given width/height (send xdg_toplevel configure).
3. Unmap all surfaces currently in the Space that are not in the list.
4. Map surfaces in list order (bottom to top), preserving the declared positions.
5. Set keyboard focus to the surface identified by `focus`.

Surfaces not yet included in any composition stay unmapped. No fallback. If the shell is not running, no surfaces are visible — failures are immediately obvious.

### What the Compositor Still Does

- Accepts Wayland connections, tracks surfaces through the protocol lifecycle.
- Buffers new surfaces in `pending_surfaces` until app_id is known.
- Emits `Topic::Apps` whenever the surface list changes (window map/unmap).
- Emits `Topic::FocusChanged` when keyboard focus changes from compositor-initiated actions (future: click-to-focus). Does NOT emit FocusChanged when applying a composition — the shell already knows the focus it set.
- Routes Super+key events to the shell's `keyboard_target` surface via `wl_keyboard.key`.
- Emits `Topic::OutputGeometry` on output hotplug/mode change.

### What Gets Removed from Compositor

- `auto_focus` logic in `apply_pending_surfaces` — shell controls focus.
- `raise_element` / `raise_shell_surfaces` — shell controls z-order.
- `handle_raise_app` — replaced by shell recomputing composition.
- `handle_set_window_geometry` — merged into composition.
- `apply_pending_geometries` / `pending_geometries` — no longer needed.
- `sync_mru` — shell tracks MRU via FocusChanged.

## Shell Behavior

The shell maintains a complete picture of the scene from:

- **WindowPolicy** from apps — their preferences (size, position, zoned, keyboard_target).
- **Apps** from compositor — which surfaces exist.
- **FocusChanged** from compositor — for MRU tracking.
- **OutputGeometry** — screen dimensions.
- **Shell's own state** — zone assignments, session persistence, active switcher, open menus.

On any state change, the shell recomputes and emits `UpdateComposition`.

### Composition Computation

The shell builds the surface list bottom to top:

1. **Shell menubar** — always present, at its declared position/size.
2. **App windows** — ordered by MRU (most recent on top), using each app's policy for size/position. Zoned apps get their zone geometry. Non-zoned apps get their declared size/position. Apps without a policy get the full output size.
3. **Shell panels** (switcher, menu dropdowns) — on top when active, absent when inactive.

Focus is set to the MRU-front app's keyboard_target surface, unless the switcher or a menu is active.

### When to Recompute

- `Apps` received — surface added or removed.
- `FocusChanged` received — MRU order changed.
- `OutputGeometry` received — screen size changed, recompute all positions.
- Switcher activated/deactivated.
- Menu panel opened/closed.
- Zone assignment changed.

## Surface Lifecycle

1. App creates a Wayland surface, sets app_id and title.
2. Compositor detects the surface in `apply_pending_surfaces`, emits updated `Apps` list. Surface is NOT mapped — it stays in the pending/known list.
3. Shell receives `Apps`, sees the new surface, computes composition including it (using the app's WindowPolicy for size/position preferences).
4. Shell emits `UpdateComposition`.
5. Compositor applies: configures the surface with size, maps at position, sets z-order and focus.

Between steps 2 and 5, the surface is invisible. The bus round-trip is ~1-2 frames.

## Non-Sola Apps

Regular Wayland clients and X11 apps (via sola-x) appear in the `Apps` list with their app_id. They have no WindowPolicy, so the shell uses defaults:

- Full output size, position (0, 0).
- Zoning available via Super+Numpad like any other app.
- Included in MRU ordering.
- Referenced in composition by `(app_id, None)` which matches any window from that app.

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

- `auto_focus` is removed — the shell controls focus via composition.
- `keyboard_target` stays — the compositor needs it for Super+key routing.
- `zoned`, `size`, `position` stay — the shell uses them to compute geometry.

## Bus Topic Changes

### Added

- `UpdateComposition(Composition)` — full scene descriptor.

### Removed

- `SetWindowGeometry` — merged into composition.
- `RaiseApp` — replaced by composition recomputation.

### Unchanged

- `Apps`, `FocusChanged`, `OutputGeometry` — compositor still emits these.
- `SetWindowPolicy` — apps still declare preferences.
- `SetAppMenu`, `MenuAction` — menu system unchanged.
- `Shutdown`, `LaunchApp` — lifecycle unchanged.

## Shell Surface Model

The full-screen overlay surface is eliminated. The shell creates purpose-specific surfaces:

- **Menubar** — persistent, declared size (output_width × 28), at bottom of z-order (coverable by fullscreen apps).
- **Switcher panel** — created at startup, unmapped until Super+Tab activates it. Centered, sized to content. Mapped on top of everything when active.
- **Menu panel** — created at startup, unmapped until a menu is clicked. Positioned below the menubar, sized to content. Mapped on top of everything when active.

Each surface has its own WindowPolicy with `keyboard_target: false` (only the menubar has `keyboard_target: true`).

## sola-x Impact

sola-x currently emits and handles `SetWindowGeometry` for X11 window resizing. With SetWindowGeometry removed, X11 geometry flows through the composition: the compositor configures sola-x's proxy surfaces based on the composition, and sola-x applies those configures to the X11 windows (this path already works). For X11-initiated resizes, sola-x reports the new size to the shell (mechanism TBD — may need a new topic or reuse of Apps metadata).
