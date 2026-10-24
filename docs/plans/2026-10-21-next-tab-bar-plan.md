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

1. ~~**Browser pages as tabs** (slice 4)~~ — applied 2026-10-21 (Phase 4
   above); groups and the strip gone (4g).
2. ~~Rename `sola-workspaces` → **Projects** (`sola-projects`; N5)~~ —
   done 2026-10-21, applied (a workspace inside it is a **tab**: `tab.*`,
   `--tab`; clean break, no `solactl workspaces`). Then OS workspaces
   (board 04, slice 5) with a workspace bound to a project (N6).
3. ~~Projects sessions as tabs (slice 3)~~ — changed by Joshua
   (2026-10-21): a window per *project*, its tabs on a top bar inside it
   (the rail goes). Built and applied 2026-10-21 (`f6ef9cf8`). Its row in
   the shell's bar shows the project's activity (2026-10-22, `2c4b95a4`):
   a diamond when a tab waits on a decision, a ring when one works.
4. ~~Titlebars with menus, gaps, the bar as an island~~ — Phase 6 below,
   committed `ec059189` (2026-10-23). Islands (board 03) after.
5. ~~Desktop workspaces with a browser profile each~~ — Phase 5 below,
   committed `388df10c` (2026-10-23).
6. ~~The dark grey base~~ — Joshua asked for it ahead of the kit overhaul
   (2026-10-23): seed, kit, apps' own colors and `design/tokens.css` in
   neutral greys; the shell moves a saved slate theme
   (`Theme::retire_slate`). Committed `ba33fda9`, applied.
7. The apps that still keep their own document list move to a window per
   document, **one app at a time** (Joshua, 2026-10-23; paused before the
   first). Candidates: Paint, Design, Bots, the wrapper's popups.

## Phase 5 — Desktop workspaces, a browser profile each (slice 5)

Spec: § Desktop workspaces, a browser profile each. Joshua (2026-10-23):
board 04 as proposed; each workspace picks a profile (shared allowed); a
page sent elsewhere reopens under the target's profile; N6 dropped. Shell
first (it works with today's browser), then the browser's several live
profiles, then the binding.

- [x] **5a Bus:** sticky `Topic::Workspaces` and `Topic::WindowWorkspace`;
      the workspaces persist inside `TabLayout` (`workspaces`, `showing`,
      `TabLayoutZone.workspace`, all `#[serde(default)]`: an old file is
      the first workspace's).
- [x] **5b Shell model:** stacks per workspace (dialogs follow their
      parent; ghosts and restore slots carry their workspace); the
      composition is the showing workspace only (no unplaced-window leak
      from other workspaces); frames, chrome and focus fallbacks per
      workspace; anything that shows a window shows its workspace first.
      Unit tests.
- [x] **5c Shell keys and UI:** Super+1…9, Super+Ctrl+1…9, Super+Ctrl+
      (Shift+)Tab; Super+K rows; the switcher pills in the band (names,
      waiting diamond, unread dot; click shows; menu: New, Rename…, Move
      Left / Right, Delete); the bar's head names the workspace; Design's
      Actual Size → ⌘⇧0. Built: `stacks/` (a `Desk` per workspace;
      readers without a window answer for the showing one), `app/
      workspaces.rs`, `menubar/pills.rs`, `menu/workspace.rs` (menu and
      rename card in the menu overlay); shell 219 tests.
- [x] **5d Browser, several profiles live:** the router routes by page
      (`Target`, `ShowPage`, close, active-tab commands wrapped as
      `Target`, dialog answers by source), accepts every helper's frames,
      tabs, dialogs, popups and context menus, and makes page ids unique;
      the app keeps a profile per page, merges tab lists, saves sessions
      per profile, and looks permissions and titles up by the page's
      profile. sola-wrapper and Design unchanged.
- [x] **5e Browser, bound to workspaces:** workspace → profile in
      `profiles.json`; new windows in the showing workspace's profile; a
      page on a workspace with another profile reopens in place; the
      Profiles menu sets this workspace's profile; `tabs` reports the
      profile. Built: router (`owners`, `prompts`, merged `pages`, fronts =
      helpers with a page on screen or the active page; helper-minted ids
      in per-profile ranges, `cef::host::tab_id_base`); `Cmd::InProfile`;
      `TabInfo.profile`; `app/pages.rs` (open in a profile, reopen in place
      via `PageWindows::retarget`, per-profile sessions, boot from the
      profiles workspaces run); `profiles.json` `workspaces`; dialog and
      notification answers go back to the asking page's helper. Browser 287
      tests. The parked-profile swap is gone (its dead helper code stays).
- [x] **5f Verify:** build, tests; apply and smoke with Joshua's OK
      (switch, send, restart restores each workspace, two profiles' pages
      painting at once on two workspaces, a page sent across reopens,
      CPU with hidden workspaces' pages).
      - Applied by Joshua 2026-10-23 (`pkg:sola` d6bb469f,
        `pkg:sola-browser` 01fecf7a). Smoked by injection: the pill and
        its menu, New Workspace, Rename (the card takes the keyboard),
        a page opened on the new workspace, Profiles → New Profile sets
        it for the workspace and the page reopens under it in place,
        Super+1 / Super+2, Super+Ctrl+2 sends a page and it reopens under
        the target's profile, helpers idle (0%) with pages on a hidden
        workspace.
      - Found on Restart Shell: the new shell said where windows were
        before its saved layout was back (everything on the first
        workspace, then the showing one), and the browser reopened pages
        back and forth; one came back twice (its old id left the closed
        set while its helper lagged). Fixed: the shell publishes
        workspaces only once its layout is loaded; the browser waits for
        the word to settle (600 ms) and never takes a reopened page's old
        id back.
      - Fix applied by Joshua 2026-10-23 (`pkg:sola` 96b78a20,
        `pkg:sola-browser` 335396fc); the apply restarted the whole session.
        Both workspaces and their windows came back, each profile's pages
        (two each) in their own windows, and no page reopened. Cleanup by
        the same means: Delete Profile on the test profile reopened its
        page under Primary in place; Delete on the test workspace moved its
        window to Main.
      - After: the Delete Profile dialog said the profile's open tabs are
        removed (they reopen under the first profile left; it says so
        now), and `profiles.json` kept a deleted workspace's profile (the
        browser now forgets workspaces the shell no longer lists). Built,
        not yet applied.

## Phase 6 — Titlebars, gaps, the bar as an island (slice 6)

Spec: § Titlebars, gaps, the bar as an island. Joshua took the boards'
picks as drawn, kit-drawn dropdowns, a compositor border for foreign
windows, and the 28 px band kept (2026-10-22).

- [x] **6a Gaps:** `GAP = 10` in `zoning.rs`; bar, content area, tiles,
      Fullscreen from it; Cinema unchanged. Tests at 1920 × 1080.
- [x] **6b The bar island:** kit `island` — rounded, rimmed, ground-black
      corners.
- [x] **6c Kit frame:** `components/window_frame/` (`frame`, `bar`,
      `dropdown`, `grips`, `island`, `paint`) — titlebar row with menus,
      dropdown overlay, rim, corners (`float_clip::draw_ears` clears them
      on floats, paints them the ground on tiles); `menu::publish` /
      `current` / `choose` (Window items on the bus, the rest delivered
      locally); storybook page (Window frame). `titlebar.rs` is gone.
- [x] **6d Apps:** `wrap_if_floating` → `window_frame` at every call
      site; Design gains the frame, Install too when it runs in a session;
      Terminal, Design, Projects and the browser publish through
      `menu::publish`.
- [x] **6e Shell menubar:** the app name, menus and their label-position
      plumbing go; Window-menu `MenuAction`s from apps act on the focused
      window. The shell's menu state still carries `is_system` though only
      the flower opens a dropdown now (cleanup left for later).
- [x] **6f sola-river:** sticky `Topic::WindowChrome { window_id, tiled,
      cinema, rim }` from the shell (`chrome.rs`, diffed); a 1 px rim (7%,
      18% focused) on windows whose app published no menu; `set_tiled`
      sent on change; float shadow radius 12.
- [x] **6g Cinema:** `FloatState::chrome` / `chrome_any` → `Chrome::Bare`.
- [x] **6h Verify:** debug build of the workspace and tests pass (bus 110,
      kit 166, shell 210, terminal 195, projects 109, design 105, mail 287,
      browser 90). Applied by Joshua 2026-10-23 (`pkg:sola` 28a139da,
      `pkg:sola-browser` f4e3fcd7, `pkg:sola-design` 06be463b). Smoked by
      injection: frames and gaps at 5×, the bar island, a menu open /
      hover-switch / Esc, Window → Float and → Right from the titlebar,
      Cinema without a frame and back. Not smoked: the close dot, edge
      resize, a foreign window's rim (none running).

## Phase 7 — A Full zone; halves keep their half

Spec: § Full, and halves that keep their half. Joshua (2026-10-23): Full
at the top of the bar, Full and the halves take turns on screen, three row
inks, the first window to Full, KP_0 to Full (Fullscreen retired),
KP_Enter Cinema.

- [x] **7a Model:** `stacks/`: `FULL` (`Zone::Fullscreen`), `HALVES`,
      `TILED`, `ZONES` (Full first); per desk `full` (picked last),
      `last_half`, `cinema`; `landing` / `pick` / `on_screen` / `front`;
      show, move, place and send pick a side, focus does not.
      `TabLayout.full` saves the side per workspace.
- [x] **7b Geometry and composition:** `tile_frame` gives Full the content
      area and each half its share always; `cinema_frame`; composition is
      the zones on screen, then floats, then Cinema. `Present` is gone.
- [x] **7c Keys and focus:** KP_0 / Window → Full moves a tab to Full;
      a closed or off-screen focused tab hands focus to the tab in front.
- [x] **7d The bar:** a Full section on top; rows lifted / bright / muted;
      kit `SidebarItem::bright` with a storybook "Row states" demo.
- [x] **7e Verify:** tests pass (shell 223, kit 166, bus 110).
- [x] **7f Apply:** applied 2026-10-23 (`pkg:sola` c0e3656a,
      `pkg:sola-browser` 1e692e51, `pkg:sola-design` 13ed999c); Joshua:
      "looks good". Not checked item by item:
      KP_0 / KP_4 / KP_6, row clicks switching sides, the last Full tab
      closing, the three inks, a shell restart keeping the side.

## Phase 8 — Design: a window per project

Spec: § Design: a window per project. Joshua (2026-10-23): like Projects,
a project is a window, its canvases tabs along the top, labelled by
folder name.

- [x] **8a Catalog and canvases:** `Project.selected` (per project),
      `collapsed` and project reordering gone; `canvas::Entry` without a
      title; `canvas::create` names the folder and writes no title.
- [x] **8b Windows:** `windows.rs` (kit `TabWindows`, project ↔ window,
      each window's plane and scale, the active one); `iced::daemon`;
      open a window per project; Close Window / Open *name*; the last
      window quits.
- [x] **8c Views per window:** the active window's canvas unparks into
      the app; others draw from their parked view (camera, pick, tour,
      notes); every message from a window activates it first; pacing,
      eviction, budget and `shown` count every window's canvas.
- [x] **8d The top bar:** `topbar.rs` — folder-name chips, agent ring,
      drag to reorder, `+` New Canvas; the rail goes (`rail.rs` →
      `dialogs.rs`).
- [ ] **8e Verify:** tests pass (design 107), release build; applied
      2026-10-23 (`pkg:sola-design` a296fbfc). Smoked by injection: the
      window and its top bar, a chip click switching canvases. Not yet:
      two Design windows on screen at once, drag reorder, New Canvas,
      Close Window / Open, `--raise`, an agent-registered project.

## Phase 9 — The top row's look (board 03)

Spec: § The top row's look. Joshua (2026-10-23): board 03's bubbles for
what the bar shows today; no island behavior.

- [x] **9a Geometry:** `zoning::ISLAND_H`, `MENUBAR_HEIGHT` = gap +
      island (38); the bar and tiles from 48; sola-river's float clamp at
      38.
- [x] **9b Islands:** `menubar::view::island` (sheen, lit top edge,
      open lift) and `item`; flower circle in Sola amber; the workspace
      island of pills (`pills.rs`); toast island; extras, spectrum,
      meters, network and clock islands; no window fill.
- [x] **9c Anchors:** `menubar/anchors.rs` walks the right side with the
      view's metrics; the flower menu hangs a gap in; right-edge panels
      line up with the row.
- [x] **9d Verify:** shell 225, river 69; release build.
- [x] **9e Apply and look:** applied 2026-10-23 (`pkg:sola` 1e38fcad);
      a screenshot shows the islands at y 10–38, the edges 10 in, tiles
      from 48.

## Phase 10 — Paint: a window per image

Spec: § Paint: a window per image. Joshua (2026-10-23).

- [x] **10a Windows:** `windows.rs` (kit `TabWindows` keyed by path, a
      stage per window, the focused one); `iced::daemon`; the sidebar
      gone; Close Window closes the image, the last quits; the no-image
      window after a 400 ms wait for the session.
- [x] **10b Acting on a window:** every message from a window (and its
      keys) focuses it; tools, crop, zoom and the file dialog act there.
- [x] **10c Verify:** paint 12 tests, clippy clean, release build.
- [x] **10d Apply and smoke:** Paint was not in canto's Oath packages;
      Joshua added it to `pkg:sola` (`sola-make` `oath/packages.rs`),
      applied 2026-10-23 (`pkg:sola` ba25bab6, `/bin/sola-paint`). Smoked
      by injection: two `solactl open`s (the second hands off) open a
      window each, rows in the bar named for the file; the close dot
      closes one image and the other stays; Super+Q then a bare launch
      brings both windows back to their places (Joshua saw both); closing
      the last window quits.

## Phase 11 — The wrapper: a window per popup; Settings on Oath

Spec: § The wrapper: a window per popup. Joshua (2026-10-23): Bots
parked; the wrapper working on canto, with Settings to set one up.

- [x] **11a Ship:** `sola-settings` in `pkg:sola`, `sola-wrapper` in
      `pkg:sola-browser` (`sola-make` `oath/packages.rs`); applied
      (`/bin/sola-settings`, `/bin/sola-wrapper`). Settings runs.
- [x] **11b Settings at half width:** the Applications list is a fixed
      280 px and the detail takes the rest (it was a fixed 380 px detail
      leaving the list ~150 px in a half tile).
- [x] **11c Wrapper windows:** `iced::daemon` over the browser's
      `PageWindows`; the site's window opens at boot; a popup gets a
      window in front, loses it when it closes; focus sets the active
      page; the engine paints the pages on screen; dialogs over their
      page; closing the site's window quits.
- [x] **11d Verify:** wrapper builds, clippy clean; its tests pass but
      one (`menu::tests::edit_menu_has_super_chords`), stale since
      `bd9ae322` (2026-09-03: the shared Edit menu gained Find, which the
      wrapper lists but does not handle). Applied (`pkg:sola` 506d5835,
      `pkg:sola-browser` 3d7f7339).
- [ ] **11e Smoke:** needs a wrapper app (Joshua sets one up in
      Settings): the site's window, a huddle popup in its own window,
      closing it, the page closing it.

## Phase 12 — Web apps: wrap a page in the browser

Spec: [`specs/2026-10-23-web-apps-design.md`](../specs/2026-10-23-web-apps-design.md).
Joshua (2026-10-23): run in the browser (W1), add from the browser (W2),
own menus and removed from there (W3). Accepted 2026-10-23 ("go ahead").

- [x] **12a Plumbing:** kit `TabWindows` under another app id
      (`open_as`); `menu::publish_for`; shell pairing by `(pid, app_id)`.
- [x] **12b App windows:** the browser opens a page as an app window (its
      id, name, icon; no location bar; the app's menus); a link the user
      follows to another site opens as a browser page (engine
      `Cmd::AppSite`; redirects stay); same-site popups join the app;
      notifications under the app's id and name.
- [x] **12c Wrap This Page…:** the sheet (name; the favicon as the icon;
      the id `web-<uuid>`); the page turns into the app in place; the
      catalog entry (kind `web`, `url`, `profile`).
- [x] **12d Launch:** `sola-browser --app <id>` and the `@app` handoff
      (with no browser up, the session is asked for `sola-browser` and the
      app handed over, so the browser never runs as the app); the session
      keeps each page's `app`; a restored app page waits up to 2 s for its
      catalog entry, so it opens as the app in its place.
- [x] **12e Remove and retire:** Remove Web App…; Settings' wrapper
      fields go (it lists command apps only); `sola-wrapper` deleted and
      out of the Oath packages (no migration, W5).
- [ ] **12f Smoke** on canto: wrap a signed-in page, launch it, a huddle
      popup, remove it. *2026-10-23, example.com:* wrap in place, its
      menus, Reload, Close Window, `--app` launch and a second launch
      raising it, an off-site link as a browser page, restore in its zone,
      Remove, a cold `--app` launch — all good. Fixed on the way: the
      session fingerprint ignored a page's app (wrapping never saved); a
      page resized several times in a row left dark (no frame at its last
      size) — page windows open at the last page window size, and a page
      without a frame at its size 300 ms on is hidden and shown again; a
      cold `--app` made the session track the browser as the app.
      Left: a signed-in site and a huddle popup.


## Phase 13 — The bar's head is the workspace switcher; the focused row

Joshua (2026-10-23): the bar did not show which window has focus; drop
the tab counts (total and per zone); move workspace switching from the
top row's pills into the bar's head as a menu, to save the top row's
space. Spec § The bar (head, rows), § The switcher (the tab bar's head).

- [x] **13a Rows:** board 05's three looks — the focused window is the
      pill (`active`), a shown tab without focus a step (kit
      `SidebarItem::shown`, 5% lift), the rest muted; a dialog's focus is
      its tab's. Kit storybook's Row states shows all four.
- [x] **13b Counts:** gone from the head and the zone headers.
- [x] **13c Switcher:** the head (name, chevron) opens the workspace menu
      under it — every workspace (checked, Super+1…9), New, Rename, Move,
      Delete for the showing one; a diamond / dot right of the name when
      another workspace waits / has unread mail. The menu overlay takes a
      `y` (`MenuOverlaySpec`). The top row's pills are gone.
- [x] **13d Close from a row** (Joshua: "an on hover close button … make
      it a stoplight"): kit `SidebarItem::stoplight` (the hover close as
      the titlebar's red dot). A tab's window: `Topic::CloseTab(key)` to
      its app (kit `TabWindows::close_request`; the browser, Paint, Design,
      Projects and Terminal handle it as that window's `CloseRequested`).
      Any other window: `Topic::CloseWindow(window_id)` → sola-river
      `river_window_v1.close`. (A first cut sent tabs through the
      compositor too, which iced closed under the app — the browser
      reopened the page; letting apps take compositor close requests
      instead would have closed every tab one by one on quit.)
- [x] **13e Smoke** on canto: the focused row against a shown one
      (pixels 53 vs 32 vs 20 idle), the menu under the head, New
      Workspace, Delete, the rename card; the stoplight on hover closing a
      browser page (closed, not reopened) and a Paint image (Paint quit
      with its last window). The diamond showed for a waiting Projects
      tab on its row; by the head (another workspace) unsmoked.

## Phase 14 — Agent islands

Spec: [`specs/2026-10-23-agent-islands-design.md`](../specs/2026-10-23-agent-islands-design.md).
Joshua (2026-10-23): one island per session, left after the flower (A1);
answers as keystrokes (A2); a finished session shows its last message and
a reply box, no jump (A3); a separate Show (A4); gone when answered or
looked at (A5).

- [x] **14a Projects keeps the ask** (`islands.rs`): what it asks from
      the hook (Claude `tool_name` / `tool_input`, `AskUserQuestion`'s
      first question, a notification's message); the choices read off the
      pane's last 40 lines (numbered lines → label + key; "Type something"
      left to the reply field); the last message on done from Claude's
      transcript — the turn's own answer, read again while Claude has not
      flushed it — else the screen's end without the input box. A need
      comes only from a status change seen live: a status first seen
      (restored after a restart) makes none.
- [x] **14b Bus:** sticky `Topic::AgentIsland` (keyed by pane) and
      `Topic::AgentIslandAction { key, Choose | Reply | Show }`.
- [x] **14c Shell islands** (`agents/`): left after the flower, newest
      first, +N past the room (a list card); the card under its island
      (head, body, choices / reply, Show; Esc closes).
- [x] **14d Projects acts:** a choice's key (`tmux send-keys -l`), a reply
      pasted then Return (a question's "Type something" first), Show
      selects the tab and shows its window; retracted on working / closed
      / answered / looked at (its tab shown in its focused window), and
      on start for any left from a previous run.
- [x] **14e Smoke** on canto with a throwaway Claude tab (removed after):
      a question (its card, Blue picked → "Which color? → Blue"), a
      finished turn (its last message), a reply ("thanks" → "You're
      welcome."), Show (the tab selected, the island gone), Esc, a
      restart (no stale islands). Unsmoked: a permission prompt (that
      session ran in auto mode), Codex / Grok sessions, the +N list.

## Phase 15 — Rounded corners for other apps' tiles

Joshua (2026-10-23): Steam's window "to also have the curved corners".

- [x] **15a sola-river `corners.rs`:** for a tiled window whose app draws
      no frame (`WindowChrome.rim && tiled`, not floating / fullscreen /
      Cinema), four decorations above it: top and bottom strips (the
      corners outside 12 px painted the ground, the rim along the curve
      and the edge), 1 px side strips (the rim); repainted on a size or
      focus change; River's square border off for it.
- [x] **15b Smoke** on canto with Steam tiled Right: four corners match a
      kit tile's, the rim 7% → 18% on focus, sola-river ~0.7% idle.
- [ ] **Later (Oath tab):** floating foreign windows — rivulet clipping a
      window's corners (stock wlroots 0.20 scene cannot).

## Phase 16 — The background picture

Joshua (2026-10-24): a background picture that fits the desktop, set from
Paint, Sola keeping its own copy; "pick something from james webb". Spec
[`specs/2026-10-24-background-design.md`](../specs/2026-10-24-background-design.md).

- [x] **16a Bus + store:** persistent `Topic::Background { path, set_at }`
      (retracted: none); `sola_core::background::{adopt, adopt_bytes}` copy
      into `~/.config/sola/background/`, the one picture there.
- [x] **16b sola-river `background.rs`:** a shell surface at the bottom,
      the output's size × integer scale, the picture filled and
      centre-cropped (decoded off the loop, EXIF-upright, Lanczos3), input
      region empty; re-decoded when the picture or the output changes.
- [x] **16c Corners on the picture:** kit frames and the bar clear their
      corners tiled or floating (`theme_for(chrome)`: overlay unless
      Cinema; `float_clip` paint mode removed); foreign tiles' corners
      painted from the picture under them, keyed by position.
- [x] **16d Setters:** Paint File → Set as Background (the file, or the
      edited pixels as PNG); `solactl background set <image>` / `clear`.
- [x] **16e Smoke** on canto (2026-10-24, `pkg:sola` caba6654): the Webb
      picture set from Paint's menu; the top row, gaps, the bar, a Paint
      tile and float and Steam's four corners show it (8× crops); clear
      and set again with `solactl`; a sola-river restart brings it back;
      sola-river idle unchanged.

## Phase 17 — The theme adapts to the background (experiment)

Joshua (2026-10-24): a setting to adapt the theme's colors to the
background picture, sampling its hues for the islands, titlebars and
sidebars. Spec [`specs/2026-10-24-background-design.md`](../specs/2026-10-24-background-design.md)
§ Adapting the theme.

- [x] **17a Recipe** (`sola_core::theme::adapt`): OKLab/OKLCH; a hue
      histogram for the tint and the accent; each adapted token keeps its
      seed grey's lightness; `Theme::adapt` / `unadapt`, idempotent.
- [x] **17b Setting:** persistent `Topic::Appearance { adapt_theme }`;
      Settings → Appearance (the picture's name, Clear, the toggle).
- [x] **17c Shell** (`adapt.rs`, `app/adapt.rs`): samples the picture
      off the UI thread and writes the adapted colors into `Topic::Theme`;
      off or no picture puts the greys back.
- [x] **17d Chrome surface follows the theme:** `style::chrome_surface()`
      (was the constant `CHROME_SURFACE`), installed by
      `theme::install_atoms`.
- [x] **17e Smoke** on canto (2026-10-24, `pkg:sola` bebed065): toggle on
      with the Cosmic Cliffs — the bar, titlebars, the top row's islands and
      sidebars take the tint, the accent the sky's blue; toggle off → greys;
      clear the picture while on → greys; set it again → adapted.
- [x] **17f Strength** (Joshua: "everything looks the same"): the first
      tint (≤ 0.03 chroma) was too faint; `Appearance.strength` (0–100,
      default 60) scales the picture's mean chroma (≤ 0.08), thinning
      below L 0.18; Settings' slider sends on release.
- [x] **17g Smoke** (`pkg:sola` 61da9435): the saved setting took the
      60% default (`bg-primary #170502`); the slider at 30% → `#120806`,
      back to 60%.

## Phase 18 — Theme variations; the idle fade

Joshua (2026-10-24): "3-4 variations … switching through the other major
colors. Let the user pick one"; "a 'fade out' effect … in place of a
screen saver" (5 minutes configurable, 30 minutes to transparent, any
input snaps back). Spec [`specs/2026-10-24-background-design.md`](../specs/2026-10-24-background-design.md)
§ Variations, § The idle fade.

- [x] **18a Variations** (`Scheme::variations`, `name`, `preview`):
      up to 4 major hues, 30° apart, ≥ 5% weight; each with its accent.
- [x] **18b Bus:** `Appearance { variation, fade_idle, fade_after_min }`
      (serde defaults, so a saved one reads); sticky
      `Topic::ThemeVariations` (shell → Settings).
- [x] **18c Shell:** keeps every scheme, applies the picked one,
      publishes the choices (after sampling, on a strength change).
- [x] **18d Settings:** the scheme cards under Strength; Idle: the toggle
      and *Start after* 1–60 min (sends on release).
- [x] **18e sola-river `fade.rs`:** `ext_idle_notifier_v1` +
      `wp_alpha_modifier_v1`; the picture (or black) over everything,
      opacity rising over 30 minutes; input → gone at once; not while
      fullscreen; a failed overlay stops asking for render passes.
- [x] **18f Smoke** on canto (`pkg:sola` b3e66f22): the Cosmic Cliffs
      offer Ember, Blue, Amber, Rose (two hue families: 30° spacing, not
      45°, gives the warm one's shades); Blue picked → `#040a1a` / accent
      `#fd9f79`. Fade with a 1-minute delay: started at 60 s; a window
      pixel `#040a1a` → `#0e0f1d` over 2 min (≈7%, on the 30-minute ramp);
      a pointer move → every pixel back at once.

## Phase 19 — Custom colors; highlights that keep the hue

Joshua (2026-10-24): custom colors in Settings → Appearance ("base
backgrounds, highlights, etc."); the tab highlights looked grey on a
tinted scheme. Spec § Custom colors and highlights.

- [x] **19a** `highlight` token / kit atom (`#353535`; storybook Theme
      page lists it); the column's active row uses it; row lifts keep the
      column's hue (`lift_in_hue`, `keep_hue`).
- [x] **19b** `Appearance.colors` + `Theme::customize` (Window, Panels,
      Highlight, Accent families); the shell's pipeline: greys → scheme →
      yours.
- [x] **19c** Settings: *Custom colors* (swatch → picker → Use / Reset);
      the panel scrolls.
- [x] **19d Smoke** on canto: a theme saved before `highlight` existed
      lacked it (the rewrite skipped absent tokens; now the seed's is
      added first); then the focused row on the Blue scheme `#29354b`
      (was a grey lift); a custom Highlight picked in the picker
      (`#8831a1`; selection `#430054`) — the bar's focused row magenta;
      Reset → `#29354b`. The scrollbar sat on the swatches once the page
      scrolled: the page has a right margin.
- [x] **19e The focus rim follows the highlight** (Joshua: "tint the
      focus rim too"): `theme::focus_rim` (the highlight 4% lighter in its
      hue; `#3d3d3d` on grey); kit frames, and sola-river for foreign
      windows (follows `Topic::Theme`: River's border, the corners' rim).
      Smoked: `#313d54` on the Blue scheme, Projects and Steam alike.
      Unfocused and island rims opaque (the picture bled through the
      edge's partly covered pixel); foreign windows' unfocused rim opaque
      too (`rest_rim`) — Steam's black XWayland edge showed as a dark line
      (16× snapshots focused / unfocused: `#121212` → `#131b2d`).
