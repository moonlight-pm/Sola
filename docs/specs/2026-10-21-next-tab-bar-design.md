# Next — one tab list (the bar)

**Date:** 2026-10-21  
**Status:** **Frozen** 2026-10-21 (Joshua: "Proceed"); **Locked models** row in `CURRENT.md`. Slice 1 in progress on `next`.  
**Related:** design `design/next/` boards 05 (the master sidebar), 06 (one tab list) and their continued boards; idea [desks](../ideas/2026-10-12-desks.md) (this is its panel model); plan [`plans/2026-10-21-next-tab-bar-plan.md`](../plans/2026-10-21-next-tab-bar-plan.md)

| | |
|--|--|
| **Implementation** | Slice 1 on `next` (built 2026-10-21, not applied): `crates/sola-shell/src/stacks.rs` (model), `tabbar/` (window + view), `zoning.rs` (bar, tile, present frames; floats), `app.rs` / `app/bus.rs` (composition, placement, focus, zone keys, `TabLayout`); sola-bus `Topic::WindowParent` + `Topic::TabLayout`; sola-river forwards River's `parent`. |
| **Dogfood** | — (canto, branch `next`) |
| **Gaps** | Everything below. |

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
| N2 | **Two tiled zones, Left and Right, 50 / 50**, plus Floating. canto has a 1920 × 1080 display. The layout is one constant in the shell; it becomes configurable if Next goes back to novus (5120 × 2160, where board 05 drew three columns). |
| N3 | A new window joins the **focused zone**. |
| N4 | The bar is **docked**, 240 px, always on. |
| N8 | The bar comes first, under today's menubar. Titlebars with menus (board 01) arrive as each app moves to a window per tab; gaps (02) and islands (03) after; the dark grey base waits for the kit overhaul. |

Proposed on board 06 and accepted for now, confirmed at their slice:
**N6** a workspace is bound to a project; **N7** the browser's tab groups
are dropped. **Open:** N5, the word "workspace" (`sola-workspaces` owns it
under a locked name) — decided before any workspace code.

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
  bar as a drop target.
- **Floating.** Every float shows, stacked most recent on top, as today.
  The bar lists them under Floating.
- **Child windows.** A window with a River `parent` (a dialog, a file
  picker) floats above its parent, has no row, and shows only while its
  parent shows. sola-river forwards the parent (below).
- **Shell surfaces** (menubar, overlays, the bar) never get rows.
- **Fullscreen and Cinema** are ways of showing the shown tab, not zones.
  Fullscreen covers the content area (both zones); Cinema covers the whole
  output, menubar and bar included. The tab stays in its stack; leaving
  puts it back.

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
| Super+KP_0 / KP_Enter | Fullscreen / Cinema the shown tab (toggle) | Same zones |
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
| 3 | Each Workspaces session is a window; the rail goes; marks ride the rows. | N5, N6 |
| 4 | Browser: a frame slot, page size, and input target per window, as board pages already work; the strip and groups come out. | N7 |
| 5 | Desks of windows in the OS sense (board 04): the bar heads with the one showing; switching swaps it. | N5 |
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
