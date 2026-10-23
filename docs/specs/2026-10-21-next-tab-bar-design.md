# Next — one tab list (the bar)

**Date:** 2026-10-21  
**Status:** **Frozen** 2026-10-21 (Joshua: "Proceed"); **Locked models** row in `CURRENT.md`. Slices 1, 2 and 4 applied on `next`; slice 3 changed (a window per project) and applied; slice 6 (titlebars with menus, gaps, the bar as an island) and slice 5 (desktop workspaces, a browser profile each) applied and committed 2026-10-23; the dark grey base in the kit 2026-10-23; a Full zone (the halves keep their half) applied and committed 2026-10-23; Design a window per project applied 2026-10-23; the top row's look (board 03) 2026-10-23; the islands not started.  
**Related:** design `design/next/` boards 05 (the master sidebar), 06 (one tab list) and their continued boards; idea [desks](../ideas/2026-10-12-desks.md) (this is its panel model); plan [`plans/2026-10-21-next-tab-bar-plan.md`](../plans/2026-10-21-next-tab-bar-plan.md)

| | |
|--|--|
| **Implementation** | On `next`, applied on canto. Slice 1 (2026-10-21): `crates/sola-shell/src/stacks.rs` (model), `tabbar/` (window + view), `zoning.rs` (bar, tile, present frames; floats), `app.rs` / `app/bus.rs` (composition, placement, focus, zone keys, `TabLayout`); sola-bus `Topic::WindowParent` + `Topic::TabLayout`; sola-river forwards River's `parent`. Slice 2: `Topic::Tab` / `Topic::TabWindow`, `pairing.rs`, kit `tabs::TabWindows`; Terminal a window per tab. Slice 4: the browser a window per page, groups and the strip gone (4g). Slice 3 as changed: Projects a window per project with a top bar of tabs; its bar row carries the project's activity (`Topic::TabActivity`, 2026-10-22). Slice 6 (`ec059189`): kit `window_frame`, the gap grid, the bar island, `Topic::WindowChrome`. Slice 5 (`388df10c`): `stacks/` a desk per workspace, pills, `Topic::Workspaces` / `Topic::WindowWorkspace`; the browser's router runs every profile's helper, pages reopen under their workspace's profile. Grey base (`ba33fda9`): `Palette::seed` and the kit in neutral greys, `Theme::retire_slate`. Full zone (2026-10-23, § Full): `stacks/desk.rs` (`full`, `landing`, `pick`, `on_screen`, `front`), `zoning::tile_frame` (halves fixed, Full the content area, `cinema_frame`), `TabLayout.full`, kit `SidebarItem::bright`; the Fullscreen toggle is gone. |
| **Dogfood** | canto, branch `next` (rivulet); smoked by injection — detail in `CURRENT.md` and the `shell-desks` capability row. |
| **Gaps** | Apps that still keep their own document list (Paint, Design, Bots, the wrapper's popups) — next, one app at a time; the islands (board 03); a workspace overview; slice 6 leftovers (menu folding, titlebar zone glyph); a dialog riding its parent unsmoked; browser ⌘-click, vault, downloads unsmoked; no keyboard tab switching in a project's top bar. |

## Intent

The shell owns where everything is. A bar down the left lists every window
on the desk, grouped by zone. A tiled zone shows one window at a time; the
bar is how you reach the rest. In the end state every page, shell, and agent
session is its own window (a **tab**), the apps drop their own tab lists,
and the bar is the only tab strip Sola has.

The compositor already does the hard part: sola-river shows exactly the
windows the shell lists in `Topic::Composition` and hides the rest
(`apply_composition_visibility`, `crates/sola-river/src/client/manage.rs`).
Hidden windows keep running. So "a zone shows one tab" is a change to the
shell's list, not to sola-river.

## Decided (Joshua, 2026-10-21)

| | Decision |
|--|--|
| N1 | Write it up as a spec and plan first; code after review. |
| N2 | **Two tiled zones, Left and Right, 50 / 50**, plus Floating. canto has a 1920 × 1080 display. The layout is one constant in the shell; it becomes configurable if Next goes back to novus (5120 × 2160, where board 05 drew three columns). **Amended 2026-10-23:** a third tiled zone, **Full**, at the top of the bar; Full and the halves take turns on screen, and a half keeps its half when the other is empty (§ Full). |
| N3 | A new window joins the **focused zone**. |
| N4 | The bar is **docked**, 240 px, always on. |
| N8 | The bar comes first, under today's menubar. Titlebars with menus (board 01) arrive as each app moves to a window per tab; gaps (02) and islands (03) after; the dark grey base waits for the kit overhaul (**amended 2026-10-23:** Joshua asked for the grey base now — in the kit on `next`; the choosable accent still waits for the overhaul). |

Proposed on board 06 and accepted for now, confirmed at their slice:
**N6** a workspace is bound to a project (**dropped for now**, Joshua
2026-10-23: projects are not tied to workspaces); **N7** the browser's tab
groups are dropped.

Decided later (Joshua, 2026-10-21): **N5 — the agent app is renamed
Projects** (`sola-workspaces` → `sola-projects`, as board 04 draws) so
"workspace" means the OS concept; the rename lands before any workspace
code and updates the **Workspaces names** lock in the same change.
**Done 2026-10-21:** the unit inside a project is a **tab** (`tab.*`,
`--tab`, JSON `tabs` / `tab` / `tab_name`), with no `solactl workspaces`
alias; tmux `sola-ws` / `sws-` and the `SOLA_WS_*` names stay (live
sessions use them). The lock row is now **Projects names**.
**Order:** the browser (slice 4) comes before Projects sessions (slice 3).
**Profiles (Joshua, 2026-10-21; shaped 2026-10-23):** each desktop
workspace picks a browser profile and every browser window on it runs that
profile (slice 5); until then page windows use the active profile.

## Model

```text
desk (one, implicit, until slice 5)
  bar                       shell window, docked left
  zone Left                 stack: tab · tab · tab   (one shown)
  zone Right                stack: tab · tab         (one shown)
  zone Floating             every float shows, over the tiles
```

- **Tab.** One toplevel window. In slice 1 any app window is a tab (the
  apps have not changed). From slice 2 the apps open one window per page,
  shell, or agent session and say which tab each window is.
- **Tiled zone.** An ordered stack of tabs, one of them shown. The others
  are left out of the composition, so River hides them; they keep running.
- **Empty zone.** Gives its width to the other. Its header stays in the
  bar as a drop target. (**Amended 2026-10-23:** a half keeps its half;
  § Full.)
- **Floating.** Every float shows, stacked most recent on top, as today.
  The bar lists them under Floating.
- **Child windows.** A window with a River `parent` (a dialog, a file
  picker) floats above its parent, has no row, and shows only while its
  parent shows. sola-river forwards the parent (below).
- **Shell surfaces** (menubar, overlays, the bar) never get rows.
- **Fullscreen and Cinema** are ways of showing the shown tab, not zones.
  Fullscreen covers the content area (both zones); Cinema covers the whole
  output, menubar and bar included. The tab stays in its stack; leaving
  puts it back. (**Amended 2026-10-23:** Fullscreen is gone — the Full
  zone took its rectangle and its key; Cinema stays. § Full.)

## Geometry (canto, 1920 × 1080)

| Thing | Rect |
|--|--|
| Menubar | 0, 0, 1920 × 28 (unchanged until board 03) |
| Bar | 0, 28, 240 × 1052 |
| Content area | 240, 28, 1680 × 1052 |
| Left / Right | 840 × 1052 each; a lone zone takes 1680 |
| Floats | Inside the content area; a fresh float is the content area inset by `FLOAT_MARGIN` |
| Overlays (menu, launcher, switcher, notify, Super+K) | Unchanged: full width under the menubar, over the bar |

No gaps and no titlebars on tiles yet (N8).

## The bar

- A sola-shell window like the menubar: an iced daemon window titled
  `tabbar`, placed by `Topic::Frame`, composed right above the menubar
  (the menubar stays `order[0]`; `stabilize_composition` in sola-river
  relies on it).
- Built on the kit sidebar (`components/sidebar`), in today's theme tokens.
  Board 06's look; the dark grey base comes with the kit overhaul.
- **Head:** the tab count. (The workspace name joins in slice 5.)
- **Zone header:** zone glyph, name, count.
- **Row:** app icon and window title, truncated. Three states, as on
  board 05: shown (lifted a step), focused (the pill), behind (tertiary
  ink). Agent marks join in slice 3.
- **Click a row:** show that tab in its zone and focus it. A float row
  raises and focuses its float.
- **Drag a row:** within a zone to reorder; onto another zone's header or
  rows to move it there (it becomes that zone's shown tab).
- Not in slice 1: the `+` on a zone header (it needs tab kinds, slice 2),
  closing from a row (there is no per-window close on the bus yet).

## Where a window lands

- A **new** window joins the focused zone at the end of its stack, and is
  shown and focused. The focused zone is the focused tab's zone; with a
  float or nothing focused, the last focused tiled zone; on an empty desk,
  Left.
- A **returning** window (the app restarted, or the session restored it)
  goes back to its remembered zone and place.
- An app whose window was floated (the float key, or sola-scope, which
  remembers its float) lands in Floating.

## Keys

| Chord | Next | Today |
|--|--|--|
| Super+\` / Super+Shift+\` | Next / previous tab in the focused zone | Next window of the focused app |
| Super+KP_4 / KP_6 | Move the focused tab to Left / Right (end of stack, shown) | Snap to the left / right column |
| Super+KP_* | Move to Floating | Float |
| Super+KP_0 / KP_Enter | Fullscreen / Cinema the shown tab (toggle). **Amended 2026-10-23:** KP_0 moves the tab to Full; KP_Enter stays Cinema | Same zones |
| Super+KP_8, 2, 5, =, ., + | Unbound on the two-zone layout | Other snaps |
| Super+Tab | Same app list; confirming shows that app's most recent tab in its zone | Raises the app |
| Super+H | Hides the app as today; its rows stay, dimmed; clicking one unhides it | Hides the app |
| Launcher on a running app | Shows its most recent tab | Raises it |

## Focus

- Showing a tab focuses it.
- Focus follows the mouse only over what is on screen; hidden windows get
  no pointer events. The bar is excluded like the menubar.
- When a zone's shown tab closes, the next tab in its stack shows (else the
  previous). When a zone empties, the other zone takes its width.

## State

- **Runtime:** per window id, its zone, its place in the stack, and
  whether it is shown.
- **Persisted (slice 1):** a new persistent bus topic, `TabLayout`: for each
  zone, an ordered list of app ids and which one is shown. A window that
  comes back takes its app's place. Keyed by app, because today's apps are
  one window each. On `next`, it replaces `Topic::Zones` as the zone store;
  `FloatGeometry` stays.
- **From slice 2:** apps announce their tabs (a `Tabs` topic: per window, a
  tab key, kind, title, restore token, and mark), and `TabLayout` keys by
  tab key instead of app id.
- New `TopicKind` variants are appended at the end; new fields on existing
  payloads are `#[serde(default)]`.

## sola-river and the compositor

Slice 1 adds one thing: River's `parent` event, forwarded as the sticky
`Topic::WindowParent { window_id, parent }` (keyed by window id, retracted
on close) — a separate topic, not a field on `Window`, because postcard is
positional and every app decodes `Windows`. Show and hide already work
through the composition.

**On canto the compositor is rivulet**, Oath's C port of River 0.4.5
(`~/Projects/Oath/apps/rivulet`, `pkg:rivulet`, `svc:rivulet`). It serves
River's protocols at the same versions, so sola-river drives it unchanged.
Any River feature Sola starts using must work in rivulet; check before
relying on it, and ask the Oath tab for what is missing. Checked
2026-10-21: rivulet sends `parent` (`src/window.c`), `identifier`
(`river_window_manager_v1` version 4), and implements `use_ssd` and
`get_decoration_above` (titlebars for foreign apps, later).

Oath owns rivulet, so Sola can also get compositor features River does not
have, added there through the Oath tab. Such a feature is rivulet-only:
novus runs River, so how novus gets it is settled before `next` merges.

## Later slices

| Slice | What | Needs |
|--|--|--|
| 2 | Kit helpers for several windows per app; floating per window instead of by title; the `Tabs` topic; menu actions that name the window. Terminal opens a window per tab and drops its strip. | How a window learns its sola-river id (below) |
| 3 | **Changed (Joshua, 2026-10-21):** each *project* is a window (a tab in the bar, named for the project) and its tabs run along a top bar inside it; the rail goes. Closing a project window keeps the project (Project → Open reopens it). Built 2026-10-21. The bar's row for a project shows its mark (2026-10-22): sticky `Topic::TabActivity` (key → idle / working / waiting) in the row's icon slot; a tab waiting on a decision beats one working. | N5 |
| 4 | Browser: a frame slot, page size, and input target per window, as board pages already work; the strip and groups come out. | N7 |
| 5 | Desks of windows in the OS sense (board 04): the bar heads with the one showing; switching swaps it. Spec'd 2026-10-23: § Desktop workspaces, a browser profile each. | N5 |
| — | Titlebars with menus (01), gaps (02), islands (03), dark grey base. | N8 |

**Which tab a window is (settled 2026-10-21 for slice 2).** Before it
opens a window an app announces `Topic::Tab { key, app_id, pid, seq }`;
the shell pairs each process's tabs (by `seq`) with its new windows (by
sola-river id, which follows creation order) and publishes
`Topic::TabWindow { key, window_id }`. The app then announces the tab
again with that `window_id`, so a restarted shell — every window already
up, tabs replaying one at a time — pairs it directly; only tabs without a
live id pair by order (a sola-river restart renumbers windows, so a gone
id falls back to order). `TabLayout` then keys tabs by `key`.
No compositor change. A unique first title was rejected (it shows in the
bar), and a client-set tag needs a protocol River lacks (rivulet could add
one later if pairing proves fragile).

## Desktop workspaces, a browser profile each (slice 5)

Board 04 (workspaces), with the bar of board 06 listing only the
workspace showing. Started 2026-10-23.

**Decided (Joshua, 2026-10-23):** board 04's proposals — a switcher in
the top band next to the flower, names and marks; named workspaces you
keep; Super+Tab across every workspace; every window on exactly one
workspace. **Each workspace picks one browser profile** (Primary by
default; several may share one); every browser window on a workspace runs
that profile, and **a page sent to another workspace reopens there under
that workspace's profile**, so a workspace never mixes profiles. **N6 is
dropped for now:** projects are not tied to workspaces. My calls: the
vault stays shared across profiles (as frozen in the profiles spec);
Design's Actual Size moves from ⌘1 to ⌘⇧0.

### Model

```text
workspaces (ordered, named, kept; one showing)
  workspace "Sola"   profile Primary
    zone Left · zone Right · Floating      (as slices 1-6, per workspace)
  workspace "Mail"   profile Primary
  workspace "Work"   profile Work
```

- A **workspace** is an id (stable), a name, and a place in the order.
  The list and which one is showing are the shell's: persistent
  `Topic::Workspaces { workspaces: [{id, name}], showing }` (namespace
  `shell/workspaces`). A fresh install has one, "Main".
- **Stacks per workspace.** Each workspace has its own Left / Right /
  Floating stacks, shown tabs, last tiled zone and Fullscreen / Cinema
  tab. Dialogs follow their parent. `TabLayout` zones carry their
  workspace (`#[serde(default)]`: an old file is all the first
  workspace's).
- **Composition** is the showing workspace only (menubar, bar, its tiles
  and floats, overlays). Windows elsewhere are left out, so River hides
  them; they keep running, and `TabWindow.shown` already goes false.
- **Which workspace a window is on** goes out as sticky
  `Topic::WindowWorkspace { window_id, workspace }` (keyed by window id,
  retracted on close), for apps that care (the browser).

### Where a window lands

- A **new** window joins the showing workspace (its focused zone, as
  today). A **returning** one (restart, restore) takes its saved workspace
  and place.
- **Super+Ctrl+1…9** sends the focused window to workspace N (end of the
  same zone's stack there, shown); the showing workspace focuses its next
  window. The tab bar's rows can be dragged onto a workspace pill.

### Switching

- **Super+1…9** shows workspace N; **Super+Ctrl+Tab** / **Super+Ctrl+
  Shift+Tab** the next / previous. Showing restores that workspace's last
  focused window.
- **Super+Tab** lists apps on every workspace; confirming one whose window
  is elsewhere shows that workspace. So do the launcher on a running app,
  a notification, a tab-bar row, and `ShowTab`: anything that shows a
  window shows its workspace first.
- Focus fallbacks (a window closes, Super+H) stay on the showing
  workspace.

### The switcher (top band)

- Right of the flower, a pill per workspace: its number and name; the
  showing one lifted. Marks roll up from the workspace's windows: a
  **diamond** when a tab waits on a decision (`TabActivity::Waiting`), a
  **dot** when a Mail window there has unread mail. Working marks nothing.
- Click a pill to show it. Its menu (right-click, or click the showing
  one): **New Workspace**, **Rename…**, **Move Left / Right**, **Delete**
  (its windows go to the previous workspace). Board 04's overview of live
  thumbnails comes later.
- The tab bar's head names the showing workspace.

### The browser

- **Several profiles live at once.** Each profile already has its own CEF
  helper; the router stops treating one as "front": it routes every
  command by the page it addresses, accepts frames, tab lists, dialogs,
  context menus and popups from every helper, and makes page ids unique
  across helpers. A command that names no page (sola-wrapper, Design)
  still goes to the one helper.
- **A page's profile is the profile of its window's workspace.** The
  browser keeps the workspace → profile map in its profile registry
  (`profiles.json`); a workspace it has not seen starts on the profile of
  the workspace that was showing when it appeared (the registry's active
  profile for the first ones it hears of, so pages already open keep
  theirs).
- **New windows** (⌘T, `OpenUrl`, a second launch, `tab.open`) open under
  the showing workspace's profile; ⌘-click and popups under their
  opener's.
- **A page whose window lands on a workspace with another profile**
  (sent there, or its workspace's profile changed) reopens at the same
  URL under that profile, in the same window: its key and place stay; its
  back history and form state do not.
- **Profiles menu:** a check on this workspace's profile; picking another
  sets it for the workspace (its pages reopen); New / Rename / Delete as
  today. Deleting a profile moves its workspaces to Primary.
- **Sessions** are saved per profile (`session_path_for`); a restore
  opens the pages of every profile a workspace runs (and the active one),
  and the shell puts each window back on its workspace by key. A profile no
  workspace runs keeps its saved pages on disk, unopened.
- Per-page lookups (window title, notification and media permissions)
  use the page's profile, not the process-wide active one.

### Not in slice 5

The overview of thumbnails; a window on every workspace; workspace ↔
project binding (N6); a vault per profile; the islands' top row (the
switcher sits in today's band).

## Titlebars, gaps, the bar as an island (slice 6)

Boards 01 (menus on the window) and 02 (gaps and the ground), and the bar
of board 06 drawn as an island. Started 2026-10-22.

**Decided (Joshua, 2026-10-22):** the boards' PROPOSED picks as drawn —
one 38 px row (close · bold app name · menus · title right-aligned),
unfocused menus dimmed but clickable, a 10 px gap everywhere, 12 px
corners, focus as a lit 1 px rim (no accent ring), a black ground. The
dropdowns are drawn by the kit inside the window, not by the shell.
Windows that are not ours sit in the grid, square, with a hairline border
from the compositor, and keep their own title bars (server-drawn titlebars
later). The top band stays 28 px with the flower and the instruments; the
islands of board 03 come later.

### Geometry (canto, 1920 × 1080, gap 10)

| Thing | Rect |
|--|--|
| Menubar | 0, 0, 1920 × 28 (unchanged; **2026-10-23:** the top row, 38 — § The top row's look) |
| Bar | 10, 38, 240 × 1032 (**2026-10-23:** 10, 48, 240 × 1022) |
| Content area | 260, 38, 1650 × 1032 (**2026-10-23:** 260, 48, 1650 × 1022) |
| Left / Right | 820 × 1032 each, 10 apart (**2026-10-23:** a lone half keeps its 820) |
| Full (was Fullscreen) | The content area |
| Cinema | The whole output, no titlebar, no gap |
| Floats | Inside the content area as before |

### The window frame (kit)

- Every kit window draws it: tiled, floating, and Fullscreen. A 38 px
  titlebar on top, the app's content under it, 12 px corners, a 1 px rim
  drawn over the edge (18% white focused, 7% not).
- **Tiled** windows stay opaque (opaque-region scanout, `PERFORMANCE.md`):
  the corners outside the radius are painted the ground's black.
  **Floating** windows stay transparent there, keep the edge resize grips,
  and the titlebar drags them.
- **Titlebar row:** the close dot (12 px, 13 px in; red only when
  focused), the app's first menu under its name in bold, the other menus
  (13 px, 8 px pad, 24 px pills; the open one is a pill), the Window menu
  last (the kit's `ensure_window_menu`), then the window title in tertiary
  ink, right-aligned, truncated first. A title equal to the app name is
  not repeated.
- **Focus** comes from the window's own focus events: a focused titlebar
  is a step brighter; unfocused, the menus dim to tertiary ink but still
  open on click. Losing focus closes an open menu.
- **Dropdowns** hang 4 px under their title, inside the window: hovering
  another title while one is open switches; Esc, a click outside, or
  choosing an item closes it.
- **Where an item goes:** a Window-menu item goes on the bus as
  `MenuAction` and the shell acts on its focused window; every other item
  is handed straight back to the app as its own `MenuAction` (a local
  loopback into the kit's bus stream — the bus never echoes an ephemeral
  event to its sender). Apps keep their one `MenuAction` handler; keyboard
  shortcuts still go through the shell as today.
- **Menus come from the app's own publish:** `BusSetup::app_menu*` and a
  kit `menu::publish` (for apps that republish at runtime) keep the app's
  latest `AppMenuPayload` in the kit; the shell still caches it for
  shortcuts.

### The shell

- The menubar drops the focused app's name and menus; the flower (system
  menu), the instruments and the clock stay.
- The bar is an island: 12 px corners, a 1 px rim, ground-black corners,
  in the grid's left column.
- Foreign windows (no published menu) get a 1 px hairline border from the
  compositor while tiled (River `set_borders`, which draws outside the
  content, in the gap); every tiled window is told it is tiled
  (`set_tiled`) so clients that draw shadows drop them.

### Not in slice 6

Board 03's islands; titlebar drag to another zone; the zone glyph and
stack count on the right of the titlebar; menus folding into ··· on a
narrow tile; server-drawn titlebars for foreign apps; the dark grey base.

## Full, and halves that keep their half (2026-10-23)

**Decided (Joshua, 2026-10-23):** a **Full** zone at the top of the bar;
its windows cover the content area (everything right of the bar). A half
no longer widens when the other is empty. Picking a Full tab leaves Left
and Right off screen; picking a Left or Right tab draws both halves and
leaves Full off screen. The bar tells three rows apart. The first window
on an empty workspace goes to Full. Super+KP_0 moves a tab to Full (the
Fullscreen toggle is gone); Super+KP_Enter stays Cinema, the whole
output edge to edge.

### Model

```text
zone Full                 stack: tab · tab        (one shown)
zone Left · zone Right    stack each              (one shown each)
zone Floating             every float shows, over either
```

- Full is `Zone::Fullscreen` on the bus (its old meaning, the content
  area, is the same rectangle), so `TabLayout`, the Window menu and the
  zone keys carry it unchanged.
- **Which is up.** Each workspace remembers whether Full or the halves were
  picked last. Showing a tab (a row, Super+\`, the switcher, the launcher,
  a new window that takes the screen, a move, a send to another
  workspace) picks its side. Focus alone never switches: the pointer only
  reaches what is on screen. A side with nothing to show (empty, or every
  tab's app Super+H'd) gives the screen to the other.
- **On screen** is then Full's shown tab, or each half's shown tab. Every
  other tiled tab is left out of the composition, so River hides it. Floats
  stay over either.
- Saved in `TabLayout.full` (`#[serde(default)]`): the workspaces with Full
  up. An older file has none, so it comes back with its halves up.

### Where a window lands

- A **new** window joins Full while Full is up, or when no half has a tab
  (so the first window of a workspace takes Full); else the last focused
  half. It is shown and focused, and its side comes up.
- A **returning** window takes its saved place without switching sides.

### Focus

- When the focused tab closes, its zone's next tab takes focus; if the
  zone emptied, the tab in front (Full's, else the last focused half's,
  else the other half's); else the most recent app.
- A focused tab whose side left the screen hands focus to the tab in
  front.

### The bar

| Row | Look |
|--|--|
| A zone's shown tab, zone on screen | Lifted (the kit's active row) |
| A zone's shown tab, zone off screen | Full ink, no lift (kit `SidebarItem::bright`) |
| Behind in its stack | Muted (the idle row) |
| Floating | The focused float lifted, the rest muted (as before) |

Sections run Full · Left · Right · Floating. Dropping above a zone's
header puts a row at the end of the zone before it (the start of Full).

### Keys and the Window menu

| Chord | Does |
|--|--|
| Super+KP_0 | Move the focused tab to Full (end of its stack, shown) |
| Super+KP_4 / KP_6 | Move it to Left / Right |
| Super+KP_* | Float it |
| Super+KP_Enter | Cinema (toggle): the whole output, menubar and bar included |

The kit Window menu's KP_0 item reads **Full**.

## The top row's look (board 03, 2026-10-23)

**Decided (Joshua, 2026-10-23):** the system top bar takes board 03's look
— "a bubble per group", the PROPOSED pick — for what is there today. No
island behavior yet (no surfaces growing from their bubble, no live
center beyond the toast, no agents on the left).

| Thing | Rect / look |
|--|--|
| Top row | 0, 0, output × 38: a 10 px gap, then the islands; no fill (the ground shows) |
| Islands | 28 px pills from 10 to output − 10, 6 apart, at y 10 |
| Bar, content area | From y 48 (gap + island + gap), as the board's tiles |

- **An island:** its face between the raised and hover greys with a soft
  top sheen, a 1 px lit top edge (the face sits 1 px into a lighter pill,
  so the rim thins down the round ends), 12 px in either side. One whose
  panel shows lifts a step (the hover grey, a brighter sheen).
- **Left:** the flower in a 28 px circle, Sola amber; then one island of
  workspace pills (3 px in, 1 apart; a pill 22 px, 9 px in; the showing
  one filled at 14% ink; names at 52% ink, numbers at 34%).
- **Center:** the toast, as an island, while one shows.
- **Right, an island per group:** extras (mail, bell, mouse battery,
  Bluetooth), the spectrum, the meters (CPU, GPU, MEM), the network (RX,
  TX), the clock; items 12 apart, each its own hit target. Panels hang
  from their item (`menubar::anchors` walks the same metrics).
- The menubar window is 38 px; sola-river's float clamp follows. The
  `shell-menubar-bg` token is no longer drawn.

## Design: a window per project (2026-10-23)

The first of the app ports (one app at a time). **Decided (Joshua,
2026-10-23):** Design works like Projects — a project is one window (one
row in the shell's bar) and its canvases are tabs along the top. **A tab's
label is the canvas's folder name** (`design/<name>/`); there is no
separate title (`canvas.toml`'s `title` is no longer shown or written).

My calls, mirroring Projects:

- **Windows.** At start a window opens for every project (the one last
  shown in front, the rest behind); none yet → one window with Add
  Project. Closing a window keeps the project; Project → Open
  *name* brings it back; the last window closing quits. The window's
  title is the project's name, so the bar's row says it.
- **The top bar.** A chip per canvas in the project's saved order: the
  folder name, the selected one lifted, a working ring while an agent
  called on it in the last 30 s. A click shows it; dragging a chip onto
  another takes that place; `+` opens New Canvas (its name becomes the
  folder, slugged). No `×` (a canvas is a folder on disk) and no rename.
  The rail goes.
- **Each window keeps its own canvas and view.** The window last focused
  or touched is *active*: its canvas takes input, keys and menu actions,
  agents' default target. Another Design window on screen draws its
  canvas from that canvas's parked view (camera, pick, tour, open notes),
  so moving between two windows loses nothing; its boards pace as on
  screen. A canvas leaving its window (another tab picked, the window
  closed) drops its pick and tour as before. A window's canvas goes live
  when the window is first shown.
- **Menus.** Project: Add Project… (⌘N), New Canvas… (⌘⇧N), Drop
  Project, Close Window (⌘⇧W), Open *name* per closed project. View and
  Share act on the active window.
- **Agents.** Unchanged: calls run in the background; `--raise` selects
  the canvas in its project's window (opening it if closed) and shows that
  window. A canvas any window shows counts as on the human's screen
  (`canvas.close` refuses it; `status.shown`).

## Not in slice 1

App changes; titlebars; gaps; islands; the dark grey base; more than one
desk; the zone header `+`; agent marks; a configurable zone layout.

## Merge

`next` merges into master only when Next is done and tested on canto:
this tab bar, the window-per-tab apps, and the rest of Next that is not in
this spec yet (menus in window titlebars, the gaps, the islands top row).
Before that merge the zone layout becomes configurable (novus's 5120
output wants three columns), and any rivulet-only feature has an answer
for River.

## Risks

- **Hidden windows still render.** River stops frame callbacks for a hidden
  window, so iced idles, but an engine behind it may not (CEF paints on its
  own clock). Watch CPU with a browser or Design window behind another tab
  during the slice 1 smoke.
