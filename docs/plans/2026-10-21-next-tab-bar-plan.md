# Next tab bar — implementation plan

**Date:** 2026-10-21  
**Branch:** `next` (canto, root checkout)  
**Design:** [`../specs/2026-10-21-next-tab-bar-design.md`](../specs/2026-10-21-next-tab-bar-design.md)  
**Status:** spec frozen 2026-10-21; Phase 1 (slice 1) in progress.

---

## Phase 0 — Align (no code)

- [x] Read the Next boards (05, 06 and continued), the desks idea, and
      the shell / sola-river / kit / app code; prep brief published.
- [x] Joshua's answers (2026-10-21): N1 spec first; N2 two zones 50 / 50
      on canto, configurable later; N3 focused zone; N4 docked 240 px;
      N8 the bar first, under today's menubar.
- [x] Spec draft + this plan; `CURRENT.md` Now; capabilities row
      `shell-desks` → spec'd.
- [x] Joshua's review of the spec → **Frozen**, and the **Locked models**
      row in `CURRENT.md` (2026-10-21).
- [x] rivulet (canto's compositor) sends `parent`; nothing to ask Oath for
      slice 1.

---

## Phase 1 — The bar over today's apps (slice 1)

**Files:** `crates/sola-shell/src/` (`app.rs`, `app/bus.rs`, `zoning.rs`,
new `tabbar/` and `stacks.rs`), `crates/sola-bus/src/topics.rs`,
`crates/sola-river/src/client/window.rs` + `registry.rs`.

### 1a — Model and geometry (no new window yet)

- [x] `stacks.rs`: `ZoneId { Left, Right, Floating }`, the two-column
      layout constant (`[(Left, 0.5), (Right, 0.5)]`), per-zone ordered
      window ids + shown id; join (end of stack, shown), move, reorder,
      remove (shown → next, else previous), show, step (Super+\`).
      Pure; unit tests.
- [x] Geometry: `BAR_WIDTH = 240`; content area right of the bar under
      the menubar; zone rects from the layout with empty zones giving
      their width away; Fullscreen = content area, Cinema = output.
      Fresh floats inset from the content area. Unit tests at 1920 × 1080.
- [x] `build_composition_entries`: menubar, bar, each tiled zone's shown
      tab, then floats (MRU) with their child windows, then overlays.
      Hidden apps (Super+H) stay out as today.
- [x] `on_windows`: a new window joins the focused zone (or Floating for a
      remembered float, or rides its parent); a returning app takes its
      `TabLayout` place; departed windows leave their stacks.
- [x] Super+KP_4 / KP_6 / KP_* move between zones; KP_0 / KP_Enter
      toggle Fullscreen / Cinema; other numpad snaps unbound.
- [x] Super+\` / Super+Shift+\` step the focused zone's stack.
- [x] Switcher confirm, launcher-on-running-app, and click activation show
      the app's most recent tab in its zone.

### 1b — sola-river parent

- [x] Track River's `parent` event per window → sticky
      `Topic::WindowParent { window_id, parent }` (keyed by window id,
      retracted on close). Not a field on `Window`: postcard is positional
      and every app decodes `Windows`.

### 1c — Persistence

- [x] `Topic::TabLayout` (persistent, appended to `TopicKind`): per zone,
      ordered app ids + shown app id. Emitted on change; replayed at start.
- [x] Stop writing `Topic::Zones` on `next`; `FloatGeometry` unchanged.

### 1d — The bar window

- [x] `WindowKind::TabBar`, title `tabbar`, opened at boot after the
      menubar; Frame `0, 28, 240 × (H − 28)`; excluded from focus-follows-
      mouse and click-raise like the menubar.
- [x] `tabbar/view.rs` on the kit sidebar: head (tab count), zone headers
      (glyph, name, count), rows (app icon, title; shown / focused /
      behind states), Hidden-app rows dimmed.
- [x] Click a row → show + focus (floats raise). Drag rows: reorder within
      a zone, drop on another zone's header or rows to move.

### 1e — Verify

- [x] `cargo make build`; shell + bus + river unit tests (shell 200 pass).
- [x] Applied twice with Joshua's OK (2026-10-21; `pkg:sola` a3236c19 from
      `b91a8ca8`): bar draws, hidden tabs idle, a full restart restores order
      and the shown tab.
- [x] Smoke by injection (2026-10-21): row click, Super+\` both ways,
      KP_6 / KP_* / KP_4 moves, Fullscreen and Cinema toggles; after
      `82c43f6a` (solactl press / release / drag, \` and keypad names) a
      row drag and the keys through rivulet. Open: a dialog.
- [ ] Remaining hands-on smoke on canto: open two
      Terminals and a Browser, stack them, switch from the bar and with
      Super+\`, move between zones, restart the shell and the apps (layout
      returns), check CPU with the browser hidden behind a tab.
- [ ] Docs: capabilities `shell-desks` / `shell-zoning` / `shell-float`,
      `CURRENT.md`, `architecture.md` (the bar, `TabLayout`, `parent`),
      `docs/manual/sola-shell.md` once installed.

---

## Phase 2 — A window per tab; Terminal first (slice 2)

**Which tab a window is (the slice 2 question).** River gives the window
manager no client-set id, so pairing is by process: before it opens a
window, an app announces `Topic::Tab { key, app_id, pid, seq }` (sticky,
keyed by `key`; `seq` counts the windows this process has opened, from 1).
sola-river mints window ids in creation order, so the shell pairs each
pid's unpaired tabs (by `seq`) with that pid's unpaired windows (by id),
in order, keeps pairs while both live, and publishes
`Topic::TabWindow { key, window_id }` (sticky, keyed by `key`) so the app
knows its windows' ids. No compositor change; rivulet and River behave
the same.

**Files:** `crates/sola-bus/src/topics.rs`, `crates/sola-shell/src/`
(`stacks.rs`, new `pairing.rs`, `app/bus.rs`), `crates/sola-kit/src/`
(new `tabs.rs`, `float.rs`), `crates/sola-terminal/src/` (`main.rs`,
`menu.rs`, `sidebar.rs` goes).

- [x] Bus: `Tab` and `TabWindow`, appended; `TabLayout` entries are keys
      (a tab key, or the app id for a window with no tab).
- [x] Shell: `pairing.rs` (pure, tested): pair by pid + order, stable while
      both live. Stacks keyed by tab key: a window paired late takes its
      key's waiting place (restore or restart). Retract `TabWindow` when
      its window closes.
- [x] Kit `tabs.rs`: open a window as a tab (announce, then
      `window::open`), close it (retract), map iced window ↔ sola-river id
      from `TabWindow`; `FloatState` answers per window.
- [x] Terminal on `iced::daemon`: a window per tab, sized and focused per
      window; keys and menu actions go to the focused window; the tab strip
      and its menu list go; ⌘T opens a tab window (the shell puts it in the
      focused zone); closing a window closes its tab; the last window
      closing quits; restore opens a window per saved `TerminalSession`;
      tab key `sola-terminal/<tab id>`.
- [x] Build, tests (shell 204, kit 163, bus 108, terminal 203; release stage of
      every package).
- [x] Terminal is one process: a second launch asks the running one for a
      new tab (`MenuAction new_tab`) — a second process would reopen every
      saved tab as windows.
- [ ] **Ask before** apply; smoke: ⌘T, close, splits in a
      tab window, restart restores each terminal tab to its zone and place.

## Phase 3+ — outline

3 Workspaces sessions as tabs; 4 browser pages as tabs; 5 desks of
windows (board 04); then titlebars, gaps, islands.
