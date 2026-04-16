# sola-shell Design

**Date:** 2026-04-14

## Overview

sola-shell is the system shell for Sola — a macOS-style menubar, app switcher, key router, and zone manager in a single sola-app process. It replaces `sola-switcher` and absorbs zone snapping from the process manager.

## Architecture

### Process Model

sola-shell is a managed sola-app process launched by the process manager. It owns two Wayland surfaces from a single GTK application:

- **Menubar surface** — 28px tall, full output width, always visible at the top of the screen. Left side shows focused app name and menu labels. Right side shows a clock.
- **Overlay surface** — full output size, transparent, hidden by default. Shown on demand for dropdown menus and the app switcher. Raised to top of z-order when active.

Both surfaces are WebView windows using the sola-app framework (extended to support multiple surfaces from one process).

### Key Routing

The compositor's existing rule is unchanged: if Super is held and no input grab is active, send the key to the bus. The shell is the primary consumer of these `Topic::Key` events.

**Routing flow:**

1. Compositor sends `Key(code, pressed, super_held, shift_held)` to the bus
2. Shell receives it and checks, in order:
   a. Built-in actions (Super+Tab = app switcher, Super+Shift+Backspace = shutdown, Super+Numpad = zone snap)
   b. Focused app's menu shortcut definitions (Super+T mapped to "new_tab" action in terminal's menu)
   c. If no match, ignored
3. For menu-matched keys, shell emits `MenuAction { app_id, action_id }` on the bus
4. The target app listens for `MenuAction` events and handles the named action

Apps stop interpreting raw `Key` events for shortcut handling. They listen for `MenuAction` instead. This eliminates the problem of multiple apps responding to the same key.

### Zone Snapping

Zone snapping logic moves from `sola/src/zoning.rs` into the shell. The shell already knows:
- The focused app (via `FocusChanged` sticky topic)
- The output geometry (via `OutputGeometry` sticky topic)
- The zone key mappings (built-in: Super+Numpad)

When a zone key is pressed, the shell computes the geometry and emits `SetWindowGeometry`. Zone assignments persist to `~/.config/sola/session.json` as before.

**Menubar offset:** All zone `rect()` values are adjusted to account for the 28px menubar. The y-origin shifts down by 28px and the available height is reduced by 28px. This happens in the shell's geometry computation, not in the `Zone::rect()` definitions themselves — the bus-level zone definitions remain fractional (0.0–1.0) and the shell applies the menubar offset when converting to pixels.

### App Switcher

The app switcher is absorbed into the shell's overlay surface. When Super+Tab fires:

1. Shell activates the overlay with the switcher UI
2. Shell emits `GrabInput("sola-shell")` so all keys route to the shell's surfaces
3. Shell emits `ListApps` and renders the app list in the overlay
4. Tab/Arrow navigation handled by the overlay's key controller
5. On Super release: shell emits `RaiseApp(selected)`, `ReleaseInput`, hides the overlay

This is the same flow as the current `sola-switcher` but rendered in the shell's overlay surface.

### Menu System

#### Menu Advertisement

Apps advertise their menus by emitting a sticky `SetAppMenu` bus topic:

```
SetAppMenu {
    app_id: String,
    menus: Vec<MenuDefinition>,
}
```

The first menu in the array is conventionally the app menu (labeled with the app name, contains About/Quit). Subsequent entries are standard menus (File, Edit, View, etc.).

```
MenuDefinition {
    label: String,
    items: Vec<MenuItem>,
}

MenuItem::Action {
    id: String,
    label: String,
    shortcut: Option<String>,  // display hint, e.g. "Super+T"
    disabled: bool,
    checked: bool,
}

MenuItem::Divider
```

The shell caches all app menus. When `FocusChanged` fires, the menubar re-renders with the focused app's menu labels.

#### Shortcut Extraction

When the shell receives a `SetAppMenu`, it scans all items for `shortcut` fields and builds a reverse lookup: `(key_combo) → (app_id, action_id)`. This lookup is consulted during key routing. If the focused app has a menu item with `shortcut: "Super+T"` and `id: "new_tab"`, then Super+T while that app is focused emits `MenuAction { app_id: "sola-terminal", action_id: "new_tab" }`.

Shortcut strings use a simple format: `"Super+T"`, `"Super+Shift+N"`. The shell parses these to match against incoming `KeyEvent` fields.

#### Dropdown Rendering

When a menu label is clicked in the menubar:

1. Menubar JS sends a command to Rust: `{ cmd: "menu_click", args: { menu_index: 1 } }`
2. Rust looks up the focused app's menu at that index
3. Rust sends the menu items to the overlay JS: `{ event: "show_dropdown", items: [...], anchor_x: 120 }`
4. Overlay renders the dropdown positioned below the menubar at the anchor position
5. Clicking an item sends `{ cmd: "menu_action", args: { action_id: "file.new" } }`
6. Rust emits `MenuAction { app_id, action_id }` on the bus and hides the overlay
7. Clicking outside or pressing Escape dismisses the dropdown

## Window Policy

Apps are the authority on how their windows should be managed. Each app declares its window roster via a sticky `SetWindowPolicy` bus topic before mapping its surfaces.

### Policy Declaration

```
SetWindowPolicy {
    app_id: String,
    windows: Vec<WindowPolicy>,
}

WindowPolicy {
    title: String,       // matches xdg_toplevel title for surface identification
    zoned: bool,         // true = shell manages position/size via zones
    auto_focus: bool,    // true = compositor gives focus on map
    size: Option<(i32, i32)>,     // fixed size for unzoned windows
    position: Option<(i32, i32)>, // fixed position for unzoned windows
}
```

### Compositor Behavior

1. New surface maps → compositor holds it invisible (does not render, does not focus)
2. Once app_id and title are known, compositor matches against declared policies
3. **Zoned window**: compositor suggests full output size, auto-focuses, participates in zone management
4. **Unzoned window**: compositor applies declared size/position, skips auto-focus, does not participate in MRU tracking
5. **No matching policy** (legacy/X11 apps): falls back to current behavior — suggest full size, auto-focus. This keeps backward compatibility.

### Surface Identification

Apps set the xdg_toplevel `title` to their declared role name before mapping the surface. The compositor matches `(app_id, title)` pairs against the policy registry.

### Examples

**Terminal** (single zoned window):
```
SetWindowPolicy { app_id: "sola-terminal", windows: [
    { title: "main", zoned: true, auto_focus: true },
]}
```

**Shell** (two unzoned windows):
```
SetWindowPolicy { app_id: "sola-shell", windows: [
    { title: "menubar", zoned: false, auto_focus: false, size: (1920, 28), position: (0, 0) },
    { title: "overlay", zoned: false, auto_focus: false },
]}
```

## Bus Topics

### New Topics

- `SetWindowPolicy(WindowPolicyPayload)` — sticky, emitted by apps at startup. Declares how each window should be managed by the compositor.
- `SetAppMenu(AppMenuPayload)` — sticky, emitted by apps at startup. Payload: `{ app_id, menus: [{ label, items }] }`
- `MenuAction(MenuActionPayload)` — emitted by shell when a shortcut or menu click maps to an action. Payload: `{ app_id, action_id }`

### Existing Topics Used

- `Key(KeyEvent)` — shell is primary consumer
- `FocusChanged(String)` — shell uses to update menubar display and shortcut context
- `OutputGeometry(OutputGeometry)` — shell uses for zone calculations and surface sizing
- `SetWindowGeometry(WindowGeometry)` — shell emits for zone snapping
- `GrabInput(String)` / `ReleaseInput` — shell uses for switcher and dropdown focus
- `ListApps` / `Apps(Vec<App>)` — shell uses for app switcher
- `RaiseApp(String)` — shell emits from switcher
- `Shutdown` — shell can emit from system menu

### Topics Removed from Other Consumers

- Terminal and other apps stop listening for `Key` events for shortcut handling
- Process manager stops handling `Key` events for zone snapping (still handles Shutdown for the supervisor loop)

## sola-app Framework Changes

The sola-app framework needs to support two surfaces from one process. Options:

1. **Two gtk4::ApplicationWindow instances** in a single GTK Application — each with its own WebView, WebContext, and asset bundle. The `on_activate` callback already provides window access; the shell creates and manages the second window itself.

2. The simpler approach: sola-app creates the menubar as the primary window. The shell creates the overlay window manually in `on_activate`, using the same GTK application context. No framework changes needed beyond what already exists.

Approach (2) is preferred — it keeps sola-app simple and the overlay is shell-specific.

## Menubar Surface

- **Size:** full output width x 28px
- **Position:** top of screen, zoned via `SetWindowGeometry` at (0, 0, output_width, 28)
- **Content:** left-aligned app name + menu labels, right-aligned clock
- **Background:** solid dark, consistent with shell theme
- **Always visible:** compositor renders it above all zone-managed windows. The zone offset ensures no window overlaps it.

## Overlay Surface

- **Size:** full output dimensions
- **Position:** (0, 0, output_width, output_height)
- **Background:** transparent
- **Hidden by default:** only shown when dropdown menu or app switcher is active
- **Content:** positioned dropdown menu or centered app switcher UI
- **Dismiss:** click outside, Escape key, or Super release (for switcher)

## Clock

Simple clock display in the right side of the menubar. Updated every minute via JS `setInterval`. No bus involvement — purely local to the shell's menubar WebView.

## Process Manager Changes

- Remove `sola-switcher` from managed processes
- Add `sola-shell` to managed processes
- Remove `zoning.rs` module and all zone snapping logic
- Remove `Key` event handling (except `Shutdown` detection for supervisor shutdown via Super+Shift+Backspace — this stays in the process manager as a safety fallback)
- The process manager becomes pure lifecycle: launch, supervise, restart

## Migration Path for Apps

Apps adopting the menu system:

1. Define menu structure and emit `SetAppMenu` sticky topic at startup
2. Replace `on_bus_event` Key handlers with `MenuAction` handlers
3. Shortcuts declared in menu items are automatically routed by the shell

Apps that don't adopt menus continue to work — they just don't appear in the menubar and don't get shortcut routing. The shell shows a bare menubar (just the app name from `FocusChanged`) for apps without registered menus.

## File Structure

```
apps/
  shell/
    Cargo.toml
    src/
      main.rs         # SolaApp builder, on_activate for overlay + key controller
      menus.rs        # Menu cache, shortcut extraction and lookup
      zoning.rs       # Zone snapping (moved from sola/)
      switcher.rs     # App switcher state and logic
    web/
      index.html      # Menubar HTML/CSS
      src/
        menubar.ts    # Menubar rendering, clock, menu label clicks
        overlay.ts    # Dropdown + switcher rendering
      overlay.html    # Overlay HTML (separate WebView)
```
