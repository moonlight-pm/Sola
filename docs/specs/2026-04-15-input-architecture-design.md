# Input Architecture Redesign

**Date:** 2026-04-15

## Overview

Super+key events route directly from the compositor to the shell's Wayland surface — no bus, no grabs, no focus changes. The bus carries actions and state, never raw input.

## Compositor Input Routing

When a key event arrives from libinput:

1. `ModifierState::update()` tracks Super/Shift state (unchanged).
2. **If Super held or Super just released**: send the key event to sola-shell's keyboard_target surface via `wl_keyboard.key`. Do NOT change keyboard focus. Do NOT forward to the focused client. Do NOT emit to the bus.
3. **If Super not held**: forward to the focused Wayland client as normal.

"Super just released" means the transition from held to not held. The shell receives the release event so it can detect when Super is lifted (for switcher confirmation).

### Shell Surface Discovery

The compositor hardcodes that Super+key events go to `sola-shell`. It finds the target surface by looking up the window policy for `sola-shell` and finding the surface whose policy has `keyboard_target: true`. The compositor caches this surface reference for fast lookup.

If no sola-shell keyboard_target surface exists (shell not running), Super+key events are silently dropped.

### Sending Keys Without Focus

The compositor sends `wl_keyboard.key` directly to the shell's surface Wayland resource, bypassing Smithay's `keyboard.input()` which targets the focused surface. The focused app's keyboard state is unaffected — it never sees these events.

## WindowPolicy Extension

```rust
pub struct WindowPolicy {
    pub title: String,
    pub zoned: bool,
    pub auto_focus: bool,
    pub keyboard_target: bool,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
}
```

`keyboard_target` is per-app — one surface per app receives keyboard focus when the compositor needs to focus that app (RaiseApp, new window auto-focus). For sola-shell specifically, the compositor also routes Super+key here.

## Shell Key Handling

The shell's menubar window (keyboard_target surface) has a GTK key controller that handles all Super+key events received via Wayland:

- **Super+Tab**: activate switcher (render app list in overlay)
- **Tab/Arrow while Super held**: navigate switcher selection
- **Super+Numpad**: zone snap (emit SetWindowGeometry)
- **Super+key matching a menu shortcut**: emit MenuAction
- **Super release**: if switcher active, confirm selection (emit RaiseApp), clear overlay
- **Super+Shift+Backspace**: handled via menu system — the shell's system menu declares this as the shortcut for "Exit Sola", the shortcut lookup matches, and the shell emits Shutdown

## Switcher Flow

1. Super+Tab → compositor routes to shell surface → key controller activates switcher, renders overlay from cached app list
2. Tab/Arrow (Super still held) → compositor routes to shell → key controller navigates
3. Mouse hover on overlay → normal Wayland pointer events → JS mouseenter → selection update via document.title
4. Super release → compositor routes to shell → key controller confirms, emits RaiseApp, clears overlay
5. Click on overlay does nothing — only Super release confirms

No grab. No bus key events. No focus changes during switching.

## Proactive App List

The compositor emits `Topic::Apps` as a sticky message whenever the app list changes:
- Window mapped or unmapped
- MRU order changes (focus change)

The shell caches the list and uses it immediately when the switcher activates. `Topic::ListApps` (request/response) is removed.

## What Gets Removed

- **`Topic::Key`** — bus never carries raw key events
- **`Topic::ListApps`** — replaced by proactive `Topic::Apps` emission
- **`GrabInput` / `ReleaseInput`** — no longer needed
- **`input_grab` state in compositor** — removed along with `clear_stale_grab`
- **`handle_grab_input` / `handle_release_input`** in compositor lifecycle
- **Key event handling in shell's `on_bus_event`** — replaced by GTK key controller
- **`KeyEvent` struct** — no longer on the bus (compositor constructs Wayland key events directly)

## What Changes

- **Compositor `input.rs`**: Super+key routing sends to shell surface instead of bus
- **Compositor lifecycle**: proactive Apps emission on window map/unmap/focus, remove grab handlers
- **Shell `main.rs`**: all key logic moves from `on_bus_event` to GTK key controller on menubar
- **Shell system menu**: "Exit Sola" declares shortcut "Super+Shift+Backspace", handled via normal menu shortcut matching
- **Terminal**: already migrated to MenuAction — no changes needed

## Resilience

- Shell restarts: compositor detects no keyboard_target surface, drops Super+key events. When shell reconnects and maps its surfaces, compositor picks up the new keyboard_target. Sticky `Apps` replay gives the shell the current app list immediately.
- Compositor restarts: shell loses its surfaces, re-creates them on Wayland reconnect, re-emits policies. New compositor picks up the keyboard_target from the fresh policy.
