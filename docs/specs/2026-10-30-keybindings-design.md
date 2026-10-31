# Keybindings — every key editable, none in conflict (Settings → Keybindings)

**Date:** 2026-10-30  
**Status:** **Agreed in shape** 2026-10-30 (Joshua: "keybinding shape looks great"); slices 1 and 2 built 2026-10-31 on branch `zone-layout` (not merged), applied on novus; slice 3 in part (the kit applies your keys to every app's menus and declares them; hand-matched keys and the shared menu definitions remain).  
**Related:** [zone layout](2026-10-30-zone-layout-design.md) (Z4, Z14: zone keys are bindings here); [Window menu and shortcuts](2026-08-31-window-menu-and-shortcuts-design.md); capabilities `shell-zoning`, `settings`

| | |
|--|--|
| **Today** | No key is stored anywhere and nothing checks for conflicts. A shell key is written in up to five places: `Shell::shell_key_chords` (what River grabs), the `on_chord` predicate chain (what it does), the Super+K rows (`shortcuts/state.rs`, missing Lock), sometimes a menu (`menu/state.rs`, kit `WINDOW_MENU_ENTRIES`), and the manual. App Super keys reach the shell (River grabs them) and go to the focused app as a `MenuAction`; Mail's and Calendar's letter keys are matched by hand in each app. The keysym tables (`sola-shell/src/keys.rs`) and `KeyCode::display` cover some keys only. |
| **Bugs this fixes** (read in code, not reproduced) | Super+L in the browser locks the screen: Lock and Focus URL share it and the shell's own shortcuts are matched first (`app/bus.rs`). ⌘= and ⌘− (zoom in Paint, Scope, Design) never fire: `=` and `-` have no keysym, so River is asked for the raw keycode. |

## Decided (Joshua, 2026-10-30)

| | Decision |
|--|--|
| K1 | A **keybinding system**: every binding in the system is user editable, and it keeps them free of conflicts. **Settings → Keybindings** lists them all. |
| K2 | **No new process.** The shell still registers keys with River and acts on them, because it knows when the launcher or a menu takes the keyboard. |
| K3 | **Every named action** is editable: the shell's actions, every menu item of every app, and apps' letter keys (Mail, Calendar). The plain movement and typing keys are not: Esc to close, Enter, the arrows, keys typed into a terminal or a field. |
| K4 | **Shared menus come from the kit.** Copy, Paste, Quit, the Window menu and the like are defined once there; no app defines ⌘V itself. |
| K5 | **System keys include Super** (media keys aside), so a change can never take a key an app needs for typing. |
| K6 | Today's two key bugs are fixed by this work, not one by one. |
| K7 | **Zone keys** are set on the Zones page; the Keybindings page lists them. |

Where the code goes (left to the implementer, "you decide"): no new crate.
The key table belongs with `KeyCode` and `KeyChord` in **sola-core**
(`keys`), which solactl and the shell already use without iced. Everything
apps use — the shared actions, a pressed iced key to a `KeyChord`, the
conflict rules, applying your changes — goes in **sola-kit** (`keys`),
where the shared Window menu already lives.

## Model

```text
action      owner · id · label · default key (or none)
owner       "system" (the shell; zones too), "common" (the kit's shared
            actions), or an app id
binding     an action's key now: your change if you made one, else its default
```

- **Declared by owners.** The shell declares its actions (and one per
  zone). The kit declares the shared ones. An app declares its own through
  the menus it already publishes. Declarations are stored on the bus, so
  Settings lists an app's keys while it is closed.
- **Your changes only are stored.** Defaults stay in code. An override is
  an action and its key, or none (removed). Reset deletes the override.
- **Conflicts.** No two system actions share a key, and no app action has
  a system key. Within one app (its own actions and the shared ones), no
  two share a key. Two different apps may share one: only the focused app
  gets it.
- **When two still meet** (a new default lands on a key you chose, or two
  defaults clash): your choice wins over a default, and the system wins
  over an app. The loser has no key until you give it one; Settings marks
  it. Super+L stays Lock; the browser's Focus URL shows "No key".

## On the bus

- **`Topic::KeyActions`**, persistent, keyed by owner: the owner's label
  and its actions (id, label, default key). The shell publishes `system`,
  each kit app its own (slice 3).
- **`Topic::Keybindings`**, persistent: your overrides (owner, action,
  key or none). Settings writes it; the shell (and, slice 3, every kit
  app) reads it.
- **`Topic::KeyCapture`**, sticky, Settings → shell: Settings is waiting
  for a key. The shell drops its grabs, media keys included, so the key
  reaches Settings' window; it registers them again when the topic is
  retracted. Settings retracts it on the key, on Esc, after 15 s, and on
  its way out.

## The shell

- `bindings` (new): the system actions — id, label, default — and the
  table of keys now (defaults with your overrides). `shell_key_chords`
  registers from it; `on_chord` looks the chord up in it and runs the
  action. The modal gates stay in front (the launcher eats keys, Escape
  closes overlays, Super+K works while the launcher is up).
- Super+K and the shell's own menu show the table's keys (Lock comes
  back to Super+K).
- Zones (zone layout slice): an action per zone, built from the layout.

## Settings → Keybindings

Sections in the page shape (`page.rs`): **System**, **Zones**, then
**Every app** (the kit's shared actions) and each app. A row is the
action's label and its key ("No key" when none). A row opens its editor in
place: **Change** (press the new key), **Remove**, **Reset** (back to the
default). A taken key names what has it and offers to move it here; the
other action is left without one. Zone rows open Settings → Zones.

Capture: while waiting, Settings shows "Press a key", sends
`Topic::KeyCapture`, and takes the next key press with its modifiers.
Esc cancels. A system action's key must include Super (K5).

## Slices

1. **System keys.** sola-core's key table (every key on a US keyboard,
   the number pad, F1–F12; keysyms both ways; names), the three topics,
   kit `keys` (conflicts, iced → `KeyChord`), the shell's table and
   dispatch, Super+K from it, Settings → Keybindings for System.
2. **Zones** (the zone layout spec): zone actions, set on the Zones page.
3. **Apps.** The kit's shared actions (Quit, Close, Undo, Redo, Cut, Copy,
   Paste, Select All, Find, the Window menu) replace each app's copies; the
   kit publishes each app's actions and applies your overrides to its
   menus; letter keys (Mail, Calendar) and the local key tables (Paint,
   Scope, the browser's fallback) go through it.

## Not in this slice

Media keys stay fixed; per-layout keysyms (the table assumes a US layout,
as today); chords of two keys in a row; apps outside the kit (their
synthesized Quit menu stays).
