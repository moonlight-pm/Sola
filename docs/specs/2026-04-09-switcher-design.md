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

2. Compositor intercepts (Super+Tab never reaches other clients)
   → emits on bus: { topic: "shell:show-switcher" }

3. Switcher hears show-switcher
   → emits on bus: { topic: "shell:list-apps" }

4. Compositor hears list-apps
   → emits on bus: { topic: "shell:apps", payload: [...] }

5. Switcher receives apps, updates DOM, highlights position 1

6. Compositor also hears its own show-switcher event — makes switcher
   surface visible, routes input exclusively to the switcher

7. User tabs through apps, then releases Super

8. Switcher emits: { topic: "shell:raise-app", payload: { app_id: "zen-browser" } }
   Switcher emits: { topic: "shell:hide-switcher" }

9. Compositor raises zen-browser windows, hides switcher surface, releases input
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

- Intercepts Super+Tab, emits `shell:show-switcher` on bus
- Maintains MRU-ordered app list (grouped by `app_id` / `WM_CLASS`)
- Responds to `shell:list-apps` with `shell:apps`
- Handles `shell:raise-app` — raises all windows of the specified app
- Handles `shell:show-switcher` / `shell:hide-switcher` — controls switcher surface visibility
- Controls input routing: while switcher is visible, input goes exclusively to the switcher

**sola-switcher** (separate process, bus client + Wayland client):

- WebView-based Wayland client
- Connects to the bus over Unix socket
- WebView surface stays hot — always mapped, compositor controls visibility
- Listens for `shell:show-switcher`, requests app list, updates UI
- Renders horizontal strip of app icons with names
- Handles input while visible: Tab (cycle), Left/Right (move selection), mouse hover (select)
- On Super release: emits `shell:raise-app` and `shell:hide-switcher`
- Resilient to compositor and bus restarts

## Input Handling

The compositor owns all input. Key behaviors:

- **Super+Tab** is intercepted by the compositor and never forwarded to clients. The compositor emits `shell:show-switcher` on the bus.
- **While switcher is visible:** all keyboard and mouse input is routed exclusively to the switcher's Wayland surface. No other client receives input.
- **Super release** is detected by the switcher (it receives key events while it has input). The switcher emits raise-app and hide-switcher.
- **Escape** while switcher is visible: hides without changing focus (emits `shell:hide-switcher` only).

## Hot WebView

The switcher WebView process runs at all times. Its Wayland surface is mapped but the compositor controls whether it's composited (visible). This means:

- No WebView startup cost when Super+Tab is pressed — the DOM is loaded, event listeners are active.
- The compositor needs to identify the switcher's surface. When the switcher connects to the bus, it can announce its Wayland surface identity so the compositor can tag it.
- When hidden: the compositor skips the surface in its render pass. The WebView process is idle but alive.
- When shown: the compositor composites the surface above all other windows and routes input to it.

## Open Questions

- **Icon system:** how apps register their icons. Lucide for Sola apps, generic for others, but the full registration system is out of scope.
- **Surface identity:** exact mechanism for the switcher to tell the compositor which Wayland surface is its overlay. Could be a bus event with the Wayland client ID, or a custom Wayland protocol just for surface tagging.
- **Multi-monitor:** which monitor shows the switcher? Likely the focused monitor.
- **Animations:** fade in/out, selection slide — nice to have, not in scope for v1.
- **Theme system:** the primary color, background opacity, etc. are hardcoded for now.
