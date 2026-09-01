# Window menu + Super+K shortcuts overlay

**Date:** 2026-08-31  
**Status:** Frozen — implemented in `sola-kit` + `sola-shell`; installed 2026-08-31  
**Related:** [shell iced](2026-05-22-sola-shell-iced-port-design.md); [floating](2026-06-24-floating-windows-design.md); [omarchy consideration](../ideas/2026-08-22-omarchy-consideration.md) § Super+K  
**Implementation:** kit `crates/sola-kit/src/menu.rs`; shell inject + intercept; overlay `crates/sola-shell/src/shortcuts/`; terminal publishes Window after Edit  
**Dogfood:** unit tests; **Installed** `kit`+`shell`+`paint`+`terminal` debug 2026-08-31 (desk smoke next)  
**Gaps:** overlay is not yet every keyboard-only surface (launcher/switcher already clickable); no user-remappable chords; Window menu has no live checkmarks for the current zone

## Intent

Drive compositor window actions from the mouse, and give one system chord that lists every built-in shortcut. Omarchy’s cheatsheet is **Super+K**; Sola uses the same chord.

## Product rules

| Rule | Choice |
|------|--------|
| Window menu | Kit helper [`window_menu`](../../crates/sola-kit/src/menu.rs). Apps **may** include / replace it (`BusSetup::window_menu` or a custom `"Window"` definition). The shell **injects** the default when the focused app omits it, so XWayland and other external windows still get a mouse path. |
| Actions | Hide, Cycle Windows, Float, every `Zone`. Shell handles `window.*` ids; unknown ids still go to the app as `MenuAction`. |
| Super+K | Shell overlay. Searchable list of built-in shell/window/capture chords plus the focused app’s published menu shortcuts (window ids not duplicated). Click or Enter runs the action. Flower menu **Keyboard Shortcuts**. Escape / Super+K again / backdrop dismisses. |
| Paint Crop | Super+K is the system cheatsheet. Paint Crop is **Super+Shift+K**. |

## Window menu (default)

```
Hide                 ⌘H
Cycle Windows        ⌘`
---
Float                ⌘KP*
---
Left / Right / Top / Bottom
---
Top Middle / Full Middle / Bottom Middle / Middle Right
---
Fullscreen
Cinema
```

## Overlay

Same modal chrome as the launcher (dim backdrop, raised card, filter field). Groups: Shell, Capture, Window, then the focused app’s menus.

## Out of scope

Remappable chords; a Window submenu (MenuItem has no submenu); titlebar window menu (D2); injecting Edit the same way.
