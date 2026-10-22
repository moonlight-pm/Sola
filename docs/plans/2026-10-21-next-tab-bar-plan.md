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
- [x] Applied 2026-10-21 (`pkg:sola` 984819fd from `5c2a0d38`). Smoked by
      injection: the Terminal tab kept its Right place as a tab key; ⌘T
      opened a tab window in Right (focused, same folder); a split and a
      pane close in it; Super+\` between the Terminal tabs; closing the
      last pane closed the window (Right fell to the other tab); a launcher
      launch while running handed off (\"terminal already running\") and
      the running process opened a tab. **Restart Shell swapped two
      Terminal tabs' pairings:** every window was up and the tabs replayed
      one at a time, so pairing by order ran before it had them all. Fixed:
      the app re-announces each tab with its window id once paired, and
      named tabs pair first (`Tab.window_id`).
- [x] Pairing fix applied 2026-10-21 (`pkg:sola` 5eb94ecb from `46fa76a7`):
      a fresh start paired both Terminal tabs in restore order; Restart
      Shell kept both pairings, the Right order, and the shown tab.
- [ ] Remaining: a dialog riding its parent; ⌘T, close, splits in a
      tab window, restart restores each terminal tab to its zone and place.

## Phase 4 — Browser pages as tabs (slice 4)

Mapped 2026-10-21 (`crates/sola-browser`, ~33k lines). Today every page
paints into one shared texture (iced shares one shader pipeline across
windows), any message redraws every window, and the CEF helper aims size,
painting, input, find, navigation, edit and JS at its one active tab (~30
sites). Design's boards already paint many pages at once, each at its own
size with input by tab id: the template.

**Decided (Joshua, 2026-10-21):** groups go (N7); **profiles move to
desktop workspaces** — each workspace has its own profile (slice 5). Until
then every page window uses the active profile, which is the router's
one-front-profile model unchanged; the Profiles menu swaps the windows.
**My calls:** ⌘-click and `target=_blank` open a window *behind* in the
focused zone (a background tab; the shell is told via `Tab.background`);
DevTools stay one per helper, so one page window at a time; the last page
window closing quits, as Terminal.

Order — the engine first, applied on its own with the old single-window
chrome driving it, so the risky half is smoked before the chrome changes:

- [x] **4a Session:** a stable page id in `SessionTab` (`#[serde(default)]`
      mints one for old files); groups and sidebar width read but unused
      (dropped in 4g; old files still load).
- [x] **4b Helper:** per-tab view state (size, shown, focused) generalised
      from boards; tab-addressed commands (resize, input, focus, nav, edit,
      paste, find, JS, show/hide) beside the active-tab ones sola-wrapper
      uses; events that lacked a tab id carry one (context menu, find
      result, IME caret, clipboard, WebAuthn); history for every tab.
- [x] **4c Router:** forwards tab-addressed commands; no resize broadcast
      for addressed resizes. Built as `Cmd::Target { tab, cmd }` (runs any
      active-tab command against `tab`; a `Resize` inside gives the page its
      own size) and `Cmd::ShowPage { id, shown }` (a shown page paints
      whether or not it is active), so the ~30 active-tab sites stay; the
      context menu, find result and IME caret carry their tab id. The
      single-window chrome sends neither, so nothing changes until 4e — no
      separate apply (2026-10-21).
- [x] **4d Rendering:** a `FrameSlot` per window, the frame stream
      demuxed by tab id, shader textures keyed by tab, per-window pump and
      `NewFrame` coalescing (so one page's 60 fps does not redraw all).
- [x] **4e Chrome** (built smaller than planned): no split of `App` — the
      page window you are in is the active tab and keeps the whole chrome
      (location bar, find, menus, dialogs, DevTools, vault, downloads); the
      other page windows show their page live under a read-only address
      bar, and focusing one makes it active (today's tab switch).
      `page_windows.rs` keeps a window and a frame slot per page;
      `sync_windows` opens and closes windows to match the pages, sends
      `ShowPage` as the shell reports windows on and off screen, and asks
      the shell to show the active page when it changed from outside its
      window. Planned: `App` split into app state + a `PageWindow` per page;
      `iced::daemon` + `TabWindows` (key `sola-browser/<page id>`); menus
      and keys to the focused window; per-window state for location bar,
      find, dialogs, context menu, vault and downloads panels, DevTools;
      process-wide statics keyed by window; the tab strip, groups and
      sidebar go.
- [x] **4f Lifecycle:** ⌘T / close / ⌘⇧T; `window.open` popups and
      ⌘-click as windows (behind); a second launch or `OpenUrl` opens a
      window; notification and JS-dialog activation show their window
      (`Topic::ShowTab { key }` → the shell shows and focuses it).
- [x] **4g Calls:** `tab.focus` / `--select` show the window; `group.*`
      and `tab.move` go (docs, tests, sola-bots instructions). Done
      2026-10-21: `groups.rs`, the dead strip view and its sidebar state,
      the favicon cache, `group.*` / `tab.move`, `tab.open --group`, the
      session's groups and sidebar width (old files load; the next save
      drops them). Applied (`pkg:sola-browser` 7c2541c2); smoked: the
      method list, a background `tab.open`, `tab.focus`, `--select`, ⌘W,
      ⌘⇧T back in its place, an old `session.json` restoring.
- [x] **4h Hidden pages rest:** the shell says which tabs are shown
      (`TabWindow.shown`); hidden page windows are `was_hidden` and paced
      down, so a stack of pages costs nothing behind the shown one.
- [ ] **Apply + smoke:** open, switch, split zones, popups, ⌘-click,
      find, DevTools, vault, downloads, restart restore; sola-wrapper and
      Design unchanged.
      - Applied 2026-10-21 (`pkg:sola-browser` bab0cc35 from `5f8c89d8`).
        Smoked by injection: restore, ⌘T, `solactl browser goto`, a
        background open joins its zone behind, ⌘W, focus follows, find.
        Found: a page opened behind stayed blank when shown; DevTools
        blanked the page and its pane.
      - Fixed in `e6c0c8e2` (a page window's slot stopped asking for
        redraws after its first frame; page slots start unsized), applied
        2026-10-21 (`pkg:sola-browser` cb88b909). Smoked: a page opened
        behind paints when shown, DevTools opens and closes with page and
        pane painting, a restart restores the pages, Design renders.
      - A `target=_blank` popup opened as its window, focused, but stayed
        black until repainted: a new window is sized twice and CEF can
        paint a stale size last. Fixed in `f035991e` (the window asks for
        its size again per stray size; the engine repaints when asked),
        applied (`pkg:sola-browser` 2e1f63db): 6 of 6 popups painted,
        DevTools on a popup too.
      - Not smoked: ⌘-click (no injector holds ⌘ over a click), vault,
        downloads, sola-wrapper.

## Next phases — order (Joshua, 2026-10-21)

1. **Browser pages as tabs** (slice 4) — next; Phase 4 below.
2. ~~Rename `sola-workspaces` → **Projects** (`sola-projects`; N5)~~ —
   done 2026-10-21, applied (a workspace inside it is a **tab**: `tab.*`,
   `--tab`; clean break, no `solactl workspaces`). Then OS workspaces
   (board 04, slice 5) with a workspace bound to a project (N6).
3. ~~Projects sessions as tabs (slice 3)~~ — changed by Joshua
   (2026-10-21): a window per *project*, its tabs on a top bar inside it
   (the rail goes). Built and applied 2026-10-21 (`f6ef9cf8`).
4. Titlebars with menus, gaps, islands.
