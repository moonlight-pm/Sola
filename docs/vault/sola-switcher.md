# sola-switcher

**Crate:** `apps/switcher/` (not yet built)
**Binary:** `sola-switcher`
**Role:** App switcher — macOS Cmd+Tab style hold-to-browse, release-to-select.

## Interaction

1. **Super+Tab** — compositor sends key to bus, switcher hears it
2. Switcher emits `Topic::GrabInput("sola-switcher")` — compositor raises and focuses it
3. Shows horizontal strip of app icons (grouped by `app_id`, MRU ordered)
4. **Tab** cycles forward, **Left/Right** moves selection, **mouse hover** selects
5. **Super release** — switcher emits `Topic::RaiseApp(selected)` + `Topic::ReleaseInput`

## Visual Design

- Dark grey translucent container (`rgba(30, 30, 30, 0.88)`), 10px corner radius
- Icons 52px with app name below, 7px corner radius on selection
- Selected app: blue background (`rgba(56, 120, 240, 0.85)`), white text
- Unselected: dimmed grey (`#888`)
- Lucide icons for Sola apps, generic icon for X11/Wayland apps

## Architecture

- **Rust host:** WebView container + [[Sola Bus]] client
- **Web frontend:** HTML/CSS/JS presentation layer
- **Surface identity:** sets `app_id = "sola-switcher"` on its xdg_toplevel
- **Hot WebView:** always running, surface always mapped but hidden. Compositor controls visibility via GrabInput/ReleaseInput.

## Flow

```
Super+Tab pressed
  → Compositor sends Topic::Key to bus
  → Switcher hears it
  → Emits Topic::GrabInput("sola-switcher")
  → Emits Topic::ListApps
  → Compositor responds with Topic::Apps
  → Switcher updates DOM

User browses (Tab/Arrow/mouse via Wayland focus)

Super released
  → Switcher emits Topic::RaiseApp("selected-app")
  → Switcher emits Topic::ReleaseInput
  → Compositor raises app, hides switcher
```

## Status

Not yet built. The compositor infrastructure (GrabInput, ReleaseInput, RaiseApp handlers, Super-key-to-bus routing) is in place. Remaining work:
- The `apps/switcher/` crate itself
- Compositor `ListApps` handler
- Compositor `FocusChanged` emission
- Surface hide/show in render pass

See: [[Sola]] > full spec at `docs/specs/2026-04-09-switcher-design.md`
