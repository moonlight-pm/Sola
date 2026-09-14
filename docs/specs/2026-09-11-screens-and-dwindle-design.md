# Screens + dwindle tiling

**Date:** 2026-09-11  
**Status:** Frozen — implemented in `sola-shell` + `sola-kit` + `sola-river` + `sola-bus`  
**Related:** [composition authority](2026-04-15-composition-authority-design.md); [window menu + Super+K](2026-08-31-window-menu-and-shortcuts-design.md); [floating](2026-06-24-floating-windows-design.md); [image clipboard](2026-09-01-image-clipboard-design.md); [omarchy consideration](../ideas/2026-08-22-omarchy-consideration.md)  
**Implementation:** shell `screens` + `tiling`; kit Window menu; river Super+Shift pointer bindings; persistent `Topic::ScreenLayout`  
**Dogfood:** **Installed** `bus`+`kit`+`shell`+`river` release 2026-09-11, unsmoked  
**Gaps:** desk smoke; groups / scratchpad / scrolling layout / Super+scroll / silent send; per-output screens; remappable chords; kvm listen Super+Tab confirm

## Intent

Bring Omarchy/Hyprland **screens** (virtual desktops) and **dwindle tiling** onto Sola without replacing River or stealing kit Cmd chords. New windows still **float**. Super+Y inserts the focused window into that screen’s BSP. Zone snaps go away.

Geometry stays shell-owned (`Topic::Frame` / `Composition`). River applies frames and pointer grabs.

## Product rules

| Rule | Choice |
|------|--------|
| Name | **Screens** — not workspaces (that is `sola-workspaces`) |
| Count | **5**, global (same 1–5 on every output) |
| Empty | Super+N with nothing there shows menubar + wallpaper |
| Default | New windows **float** on the current screen (app size + CSD) |
| Tile | Super+Y toggles tile ↔ float |
| Algorithm | Hyprland **dwindle**: split the focused tiled leaf along its longer edge; new window is right/bottom; close collapses the parent; `preserve_split` so Super+J sticks |
| Gaps | 12px outer, 8px inner, under the 28px menubar |
| Persist | Current/former screen, per-window screen, BSP (app_id + split dir/ratio), float rects, fullscreen/cinema |
| Super | Remains macOS Command for apps (T/F/W/L/H/K/Q/`/Space, ←→ back/forward) |
| Switcher | **Alt+Tab** HUD, **this screen only** (hidden apps on this screen stay listed). Confirm on Alt release |
| Capture | Super+Ctrl+3/4/5 (full / selection / window). Super+Shift+1…5 is send-to-screen |
| Mouse | Super+Shift+left drag move; Super+Shift+right drag resize; instant. Super-click and Ctrl-click reach clients |
| Zones | Retired as a layout model. Numpad snaps and zone Window-menu items go. Overlay Frame helpers stay |
| Cinema | Super+Shift+M — true fullscreen including the menubar (`Frame.fullscreen`) |
| Fullscreen | Super+M — usable area under the menubar, no gaps |
| Toasts | Menubar `AppToast` whispers become desk notifications (`app_id` sola-shell) |

## Chords

| Chord | Action |
|-------|--------|
| Super+1…5 | Switch to that screen |
| Super+Shift+1…5 | Send focused window there and follow |
| Super+Tab / Super+Shift+Tab | Next / previous screen |
| Super+Ctrl+Tab | Former screen |
| Alt+Tab / Alt+Shift+Tab | Switcher HUD, this screen only |
| Super+↑↓ | Focus nearest window on this screen in that direction (tiled and float) |
| Super+Shift+←↑↓→ | Swap with neighbor (tiled) |
| Super+J | Toggle split of the focused tile’s parent |
| Super+Y | Toggle tile / float |
| Super+M | Fullscreen under the menubar |
| Super+Shift+M | Cinema |
| Super+Shift + left drag | Move (float: free; tiled: follow pointer, drop swaps or snaps back) |
| Super+Shift + right drag | Resize (float: free; tiled: split ratio) |
| Super+Ctrl+3/4/5 | Screenshot full / selection / window |

Unchanged: Super+Space launcher, Super+K cheatsheet, Super+H hide, Super+Q close app, Super+` cycle windows of focused app, Super+T/F/W/L app chords, Super+←→ back/forward.

## Window menu

```
Hide                 ⌘H
Cycle Windows        ⌘`
---
Tile                 ⌘Y
---
Fullscreen           ⌘M
Cinema               ⇧⌘M
```

## Menubar

Five chrome-sized numerals **1–5** in the bar’s middle (between app menus and the stats cluster). Current = accent; occupied = primary chrome; empty = muted. Click switches. No toast overlay in the bar — former whispers are notifications.

## Out of v1

Window groups, scratchpad, scrolling layout, pop-out/pin, Super+scroll through screens, Super+Shift+Alt silent send, per-output screens, focus left/right (browser Back/Forward), Super+W as close-window.
