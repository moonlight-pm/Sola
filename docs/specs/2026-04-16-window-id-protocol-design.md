# Window ID Protocol

## Problem

The bus protocol identifies windows by `(app_id, title)`. X11 apps change their title dynamically (Brave changes it on every tab switch), causing the shell's Composition, Focus, and Frame messages to target stale identities. The compositor can't find the window, it gets unmapped, and the UI flickers.

## Solution

The compositor assigns a stable `u32` window ID when each window becomes ready. All bus messages that reference a specific window use this ID instead of `(app_id, title)`.

## Bus Topic Changes

### New: `WindowInfo` (replaces `App`)

```rust
struct WindowInfo {
    window_id: u32,
    app_id: String,
    title: String,
}
```

`Apps(Vec<App>)` becomes `Windows(Vec<WindowInfo>)`. Emitted as sticky by the compositor whenever windows appear or disappear. The shell groups by `app_id` for display purposes.

### Updated: `CompositionEntry`

```rust
struct CompositionEntry {
    pub window_id: u32,
}
```

### Updated: `FrameUpdate`

```rust
struct FrameUpdate {
    pub window_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
```

### Updated: `FocusTarget`

```rust
struct FocusTarget {
    pub window_id: u32,
}
```

### Updated: `MouseEnteredPayload`

```rust
struct MouseEnteredPayload {
    pub window_id: u32,
}
```

### Unchanged

- `WindowPolicyPayload` — still uses `app_id` + window `title`. Apps emit policies before the compositor assigns IDs. The compositor matches policies to windows by `(app_id, title)` internally.
- `AppMenuPayload`, `MenuActionPayload` — keyed by `app_id`, not individual windows.
- `ShellKeyBindingsPayload` — keyed by `app_id`.

## Compositor Changes

### ID assignment

- New field: `next_window_id: u32` on State, starts at 1.
- In `apply_pending_surfaces`, when a window becomes ready (has app_id, and wl_surface for X11), assign `next_window_id` and increment.
- Store mapping: `window_ids: HashMap<u32, Window>` on State (window_id to Window).
- Reverse mapping: store the window_id on each Window via Smithay's `UserDataMap`.

### Window lookup

- New helper: `find_window_by_id(id: u32) -> Option<Window>` replaces `find_surface`.
- `handle_composition`, `handle_frame`, `handle_focus` all resolve by window_id.
- `forward_pointer_motion` emits `MouseEntered` with window_id.
- `emit_apps_list` becomes `emit_windows_list`, includes window_id, app_id, and title for each window.

### Frame geometries

Currently keyed by `(app_id, Option<String>)`. Change to keyed by `window_id: u32`.

## Shell Changes

### Receiving window list

- Shell receives `Windows(Vec<WindowInfo>)` instead of `Apps(Vec<App>)`.
- Builds internal data structures:
  - `windows: HashMap<u32, WindowInfo>` — all known windows by ID.
  - Groups by `app_id` for switcher display and MRU tracking.

### Emitting messages

- `emit_composition` builds `CompositionEntry { window_id }` using stored IDs.
- Focus messages use `FocusTarget { window_id }`.
- Frame/zone messages use `FrameUpdate { window_id, ... }`.
- The shell looks up its own window IDs (menubar, switcher, launcher) from the Windows list by matching `app_id + title`.

### MRU and focus tracking

Currently tracks `focused_app_id: Option<String>`. Changes to `focused_window_id: Option<u32>`. The shell can derive app_id from the window list when needed.

## Migration

The `App` struct and `Apps` topic are removed. The `Windows` topic replaces it. All consumers (shell, switcher, monitor) update simultaneously in the same commit set.

## Success Criteria

- Brave tab switches don't cause flickering or focus loss.
- Steam with multiple windows (main + Friends List) can be independently composed.
- Shell's own multi-window setup (menubar, switcher, launcher, menu) works as before.
- Monitor app shows window IDs in its output.
