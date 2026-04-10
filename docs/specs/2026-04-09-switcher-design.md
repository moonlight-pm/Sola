# Sola Switcher Design

Depends on: [Sola Bus Design](2026-04-09-sola-bus-design.md)

## Overview

The app switcher is the first shell app for Sola. It lets the user cycle through running apps with Super+Tab (hold-to-browse, macOS Cmd+Tab style) and raise the selected app on release.

## Interaction Model

1. **Super+Tab** opens the switcher.
2. Switcher shows icons for all open **apps** (grouped by `app_id` / `WM_CLASS`, not individual windows).
3. Apps are ordered by most recent use. The current app is at position 0.
4. Position 1 (if it exists) is highlighted.
5. Continued Tab presses advance the selection, cycling at the end.
6. Mouse motion over an app sets it as the current selection.
7. Left/Right arrow keys move the selection.
8. **Releasing Super** raises all windows of the selected app and hides the switcher.

## Visual Design

- **Layout:** horizontal strip of app icons, centered on screen.
- **Container:** semi-translucent dark grey background (`rgba(30, 30, 30, 0.88)`), rounded corners (10px), generous padding (36px vertical, 44px horizontal).
- **App cells:** icon (52px) with app name below, 12px gap, 20px/24px padding, rounded corners (7px).
- **Selected app:** primary theme color background (blue, `rgba(56, 120, 240, 0.85)`), white icon and text.
- **Unselected apps:** dimmed grey icons and text (`#888`).
- **Icons:** Lucide icons for custom Sola apps. Generic icon for X11/Wayland apps. Full icon system TBD (tied to app registration, out of scope for this spec).

## App Grouping

Windows are grouped into apps by `app_id` (Wayland xdg_toplevel) or `WM_CLASS` (XWayland). When the selected app is raised, **all of its windows** come to the front as a group, maintaining their relative z-order.

## Super+Tab Flow

```
1. User presses Super+Tab

2. Compositor sees Super held → sends key event to bus (not to any client)

3. Switcher hears Super+Tab on bus
   → emits shell::GrabInput("sola-switcher")

4. Compositor hears GrabInput
   → shows switcher surface above all windows
   → routes all keyboard/pointer input to switcher's Wayland surface

5. User tabs through apps (Tab/Arrow/mouse go to switcher via normal Wayland focus)

6. User releases Super — switcher gets key-up via Wayland
   → emits shell::RaiseApp("zen-browser")
   → emits shell::ReleaseInput

7. Compositor hears ReleaseInput → hides switcher surface, restores normal focus
   Compositor hears RaiseApp → raises all windows of that app
```

## Architecture

### Workspace

```
apps/
  switcher/
    src/       # Rust: WebView host, bus client
    web/       # HTML/CSS/JS: switcher UI
```

### Components

**sola-compositor** (separate process, bus client):

- Sends all Super+key events to the bus (never forwards to Wayland clients)
- Handles `shell::GrabInput` — shows target surface, routes all input to it
- Handles `shell::ReleaseInput` — hides surface, restores normal focus
- Handles `shell::RaiseApp` — raises all windows of the specified app
- Maintains MRU-ordered app list (grouped by `app_id` / `WM_CLASS`)
- Responds to `shell::ListApps` with `shell::Apps`
- No knowledge of what the switcher is — just responds to generic topics

**sola-switcher** (separate process, bus client + Wayland client):

- WebView-based Wayland client
- Connects to the bus over Unix socket
- Listens for Super+Tab key events on the bus
- On Super+Tab: requests app list, grabs input, updates UI
- Handles input while grabbed: Tab (cycle), Left/Right (move selection), mouse hover (select)
- On Super release: emits RaiseApp and ReleaseInput
- Resilient to compositor and bus restarts

## Hot WebView

The switcher WebView process runs at all times. Its Wayland surface is mapped but the compositor controls whether it's composited (visible). This means:

- No WebView startup cost when Super+Tab is pressed — the DOM is loaded, event listeners are active.
- The compositor needs to identify the switcher's surface. When the switcher connects to the bus, it can announce its Wayland surface identity so the compositor can tag it.
- When hidden: the compositor skips the surface in its render pass. The WebView process is idle but alive.
- When shown (via GrabInput): the compositor composites the surface above all other windows and routes input to it.

## Surface Identity

The compositor identifies the switcher's surface via the `app_id` field on its xdg_toplevel. The switcher sets `app_id = "sola-switcher"` on its Wayland surface. When `GrabInput("sola-switcher")` arrives on the bus, the compositor looks up the window by app_id using `state.window_by_app_id()`. No custom protocol or bus announcement needed.

## What's Implemented (on master)

The following infrastructure is already in place:

- **Bus:** `sola-bus` crate with `Message` wire format, `Topic` enum via `define_topics!` macro, `BusClient` with `emit()`/`try_recv()`/`recv()`
- **Topics:** `Key(KeyEvent)`, `GrabInput(String)`, `ReleaseInput`, `ListApps`, `Apps(Vec<App>)`, `RaiseApp(String)`, `FocusChanged(String)`, `LaunchApp(String)`, `Shutdown`
- **Compositor input:** Super+key events sent to bus as `Topic::Key`, never forwarded to clients
- **Compositor handlers:** `GrabInput` (find window by app_id, raise, focus), `ReleaseInput` (clear grab), `RaiseApp` (raise all windows of an app, focus topmost)
- **Process manager:** sola launches sola-bus + sola-compositor, handles kill chord from bus, restart backoff, PR_SET_PDEATHSIG

## What Needs Building

- The `apps/switcher/` crate itself (Rust WebView host + bus client + web frontend)
- Compositor `ListApps` handler (build MRU app list from Space, respond with `Topic::Apps`)
- Compositor `FocusChanged` emission on window focus changes
- Surface hide/show in render pass (currently GrabInput raises but ReleaseInput doesn't hide)

## Open Questions

- **Icon system:** how apps register their icons. Lucide for Sola apps, generic for others, but the full registration system is out of scope.
- **Multi-monitor:** which monitor shows the switcher? Likely the focused monitor.
- **Animations:** fade in/out, selection slide — nice to have, not in scope for v1.
- **Theme system:** the primary color, background opacity, etc. are hardcoded for now.
