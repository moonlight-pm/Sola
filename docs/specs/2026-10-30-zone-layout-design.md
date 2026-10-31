# Zone layout — zones drawn on a grid (Settings → Zones)

**Date:** 2026-10-30  
**Status:** **Draft** — for review (Joshua); no code yet.  
**Related:** [Next tab bar](2026-10-21-next-tab-bar-design.md) (§ Full; § Merge: "the zone layout becomes configurable" before novus); [keybindings](2026-10-30-keybindings-design.md) (Z14); capabilities `shell-zoning`, `shell-desks`, `settings`

| | |
|--|--|
| **Today** | The tiled zones are a constant: Full (the content area) and `HALVES` (Left, Right, 50 / 50) in `crates/sola-shell/src/stacks/mod.rs`, framed by `zoning::tile_frame`. Cinema is a toggle on the shown tab (`Desk.cinema`, `Shell::snap_focused_zone`); Float has no rectangle. Keys are fixed: Super+KP_0 Full, KP_4 / KP_6 Left / Right, KP_* Float, KP_Enter Cinema. An app's own fullscreen request is granted by sola-river straight away, and the shell never learns of it (`manage::apply_fullscreen_requests`). `Zone` on the bus is a closed enum that still carries the old zones (Top, Bottom, TopMiddle, FullMiddle, BottomMiddle, MiddleRight); the Window menu and Super+K still list them. |
| **Gaps this closes** | novus (5120 × 2160) wants three columns; the Window menu and Super+K listing zones that do nothing (`shell-zoning`, found 2026-10-29). |
| **Depends on** | [Keybindings](2026-10-30-keybindings-design.md) slice 1 (Z14): zone keys are bindings in it. |

## Decided (Joshua, 2026-10-30)

| | Decision |
|--|--|
| Z1 | The layout is set in **Settings**, on a drawing of the screen at its aspect ratio. |
| Z2 | **Rows and columns first**: the content area is a grid of equal cells. Rows and columns change only with **− / +** buttons. |
| Z3 | A zone is a **rectangle of cells**: drag across cells, or click one. Then give it its key. |
| Z4 | Each zone can have a **key**. It is one key, and **Super is added** to it. |
| Z5 | **Full** (the content area), **Cinema** (the entire screen, for games and such) and **Floating** always exist. Their keys can be changed or removed; they cannot be deleted. |
| Z6 | **A new window comes up in the zone you were last looking at.** Only the first window of a workspace goes to Full. |
| Z7 | A zone is shown by **its key and a little drawing of it**. Zones have no names. |
| Z8 | When **−** would cut zones, the page lists them and asks before deleting them. |
| Z9 | **Edit a zone's cells** without touching its key. |
| Z10 | **Several screens: parked.** When it comes, every screen is on the page, each set up in the same place, and a key belongs to one screen's zone (§ Several screens). |
| Z11 | **No toggles.** Cinema is a zone with its own stack, as Full is. To change a window's size, send it to another zone. |
| Z12 | **Cinema is completely opaque**: no desktop background bleeds into a game (§ Cinema). |
| Z13 | **+** keeps each zone's cells for now. The best flow is found by using it. |
| Z14 | **Keys belong to the [keybinding system](2026-10-30-keybindings-design.md)**: every binding in the system is user editable, it keeps them free of conflicts, and Settings gets a full **Keybindings** page. Zone keys are still set on the Zones page, through it. |
| Z15 | **An app that asks for fullscreen** (a game, a video, F11) moves to Cinema; when it asks to leave, it goes back to the zone it came from. |
| Z16 | **A window opened while Cinema is up comes up in Full.** Nothing keeps track of the zone before Cinema. |

Proposed here, for review:

| | Proposal |
|--|--|
| P1 | Zones may overlap. **Showing a zone pushes off whatever overlaps it; a zone pushed off comes back once nothing covers its cells** (§ What is on screen). This is today's Full / halves rule, made general. |

## Model

```text
screen
  grid            rows × columns of equal cells over the content area
  Full            built-in: every cell                       stack (one shown)
  grid zone …     a rectangle of cells                       stack (one shown)
  Cinema          built-in: the whole output (top row, bar)  stack (one shown)
  Floating        built-in: no rectangle                     every float shows
```

- **Grid.** 1–12 rows and 1–12 columns of equal cells. Uneven splits take
  more cells: 25 / 50 / 25 is four columns, a zone two wide in the middle.
- **Grid zone.** A stable id (never reused in the layout) and its cells
  (first row, first column, rows, columns). Two zones cannot have the same
  cells, and a grid zone cannot cover every cell (that is Full).
- **Full** covers every cell. **Cinema** covers the whole output. Both are
  stacks, as every tiled zone is.
- **Stacks** are unchanged: an ordered stack of tabs, one shown; the rest
  are left out of the composition. The bar is unchanged: one run per
  workspace, no zone headers.
- **One layout per screen**, the same on every workspace.

### What is on screen (P1)

Each workspace remembers the order its zones were last picked (a row in
the bar, Super+\`, a zone key, a new window). To draw it, walk that order
from the most recent, then the zones never picked, in layout order. Each
zone that has a tab to show takes its cells, unless a zone already taken
overlaps it.

With the default layout this is today's rule: picking a Full tab takes
both halves off; picking a Left tab takes Full off and brings Right back,
because nothing covers Right any more. Cells no shown zone covers show
the ground. A zone keeps its size, so nothing widens into an empty
neighbour (as "a half keeps its half" today).

Cinema overlaps everything, so a Cinema tab on screen is the only window
drawn (with its dialogs); floats come back when it goes. It covers the bar
and the top row, so the way out is a key: another zone's key sends the
tab there, and Super+\` or the launcher picks another tab.

Saved: `TabLayout.full` (the workspaces with Full up) becomes each
workspace's pick order. An older file reads as `[Full]` for the
workspaces it lists, and nothing picked for the rest.

### Geometry

The content area (`zoning::content_rect`, right of the bar and under the
top row, inside the gap grid) is cut into cells with one `GAP` between
neighbouring cells. Column *c*'s left edge is
`cx + round(avail × c / columns) + c × GAP`, where
`avail = cw − GAP × (columns − 1)`; rows the same way. A zone spanning
several cells takes the gaps inside it, so neighbouring zones sit one gap
apart. On one row and two columns this gives exactly today's Left and
Right frames. Full stays the content area; Cinema the whole output, in
true fullscreen (`cinema_frame`).

### Cinema (Z12)

Nothing of the desk shows behind, through or over a Cinema tab:

- **Behind** it is black, never the background picture. A game that does
  not fill the output (a smaller mode) sits on black.
- **Through** it: the desk's opacity (`Appearance.opacity`) and the idle
  fade do not apply (sola-river already lifts its overlay for a fullscreen
  window), and a window that draws with alpha shows black where it is
  clear, not the picture. The plan finds where the black comes from
  (River's fullscreen, or a black surface under the tab) and checks it
  with a picture up.
- **Over** it: no floats, bar, top row, shadow or rim. What you call up
  yourself (the launcher, Super+K) still comes up over it.

### Where a window lands (Z6)

- A **new** window comes up in the tiled zone you last looked at (the
  last focused one; Full counts): shown and focused. The first window of a
  workspace goes to Full, and so does one opened while Cinema is up
  (Z16). Apps that float by
  default (`floats_by_default`) still float.
- A **returning** window takes its saved place, without switching sides.
  If its zone is gone, it goes to the end of Full.
- An **app asking for fullscreen** goes to Cinema, and back when it asks
  to leave (Z15). sola-river forwards the request to the shell instead of
  granting it itself.
- When the layout changes live, windows in a deleted zone move to the end
  of Full; windows in a zone whose cells changed are framed again where
  they are.

## Keys (Z4, Z14)

- Every zone has an action, **Move to** that zone: the focused tab goes to
  the end of the zone's stack, shown, as KP_4 and KP_6 do now. Cinema's is
  the same (no toggle); Floating's floats the tab.
- A key is one key with Super added. Settings records the key alone, so
  River's bindings never catch it first.
- **Default keys** are today's: one row, two columns, the left column on
  KP_4 and the right on KP_6; Full KP_0, Cinema KP_Enter, Floating KP_*.
- The keybinding system holds the keys, checks for conflicts and registers
  them with River. A zone's key is set on the Zones page and shows on the
  Keybindings page too. Deleting a zone frees its key.

## Settings → Zones

A page in the shape every Settings page has (`page.rs`): sections, each a
heading and a group of rows.

**Screen.** The drawing: a box at the output's aspect ratio. The top row
and the bar are drawn as fixed chrome; the content area shows the grid's
cells. Zones show as outlines with their key on them; the selected or
hovered zone is filled. Under it: **Rows − n +** and **Columns − n +**.

**Zones.** One row per zone. Full, Cinema and Floating come first, with
their names (their drawings alone look alike) and their key. Each grid
zone's row is its little drawing and its key, or "No key". A row opens its
editor in place: **Key** (press a key; **Clear**), and for a grid zone
**Edit Cells** and **Delete**. The section's action is **Add Zone**.

- **Add Zone.** The drawing waits for cells: drag across cells (the
  selection snaps to whole cells) or click one. On release, "Press a key
  for this zone": the next key press becomes its key. Esc leaves it
  without one, to set later. Esc while choosing cells cancels.
- **A key that is taken.** The page names what has it (from the
  keybinding system) and offers to move it here; that action is left
  without a key.
- **Edit Cells.** The same, starting from the zone's cells. Release
  replaces its cells; its key stays.
- **+** adds a row at the bottom or a column on the right. Zones keep their
  cells, so they get smaller in proportion (Z13). Nothing is deleted.
- **−** removes the bottom row or the right column. Any zone touching it
  would be deleted (Z8): the drawing marks them and asks "Delete *n*
  zones?". Cancel leaves the grid as it was. With none, it just shrinks.

Every change applies at once: Settings sends the layout and the shell
frames it.

## On the bus

- **`Topic::ZoneLayout`**, persistent, Settings → shell (as
  `Topic::Appearance` is): rows, columns and the grid zones (id, cells).
  Keys are not in it; they are the keybinding system's. The shell uses the
  default layout until there is one.
- **`Zone`** keeps `Fullscreen` (Full), `Cinema` and `Float`, and gains
  `Grid(id)`. Left, Right, Top, Bottom, TopMiddle, FullMiddle,
  BottomMiddle, MiddleRight and `Zone::rect` go. A saved `Left` reads as
  the default layout's zone 1 and `Right` as zone 2. `Desk.cinema` (the
  toggle) goes; Cinema's stack is saved as the others are.
- The legacy persistent `Topic::Zones` (unused) goes.
- **The Window menu and Super+K** list the layout's zones, each its
  drawing and key, in place of the kit's fixed zone items
  (`WINDOW_MENU_ENTRIES`). The kit builds the Window menu in every app, so
  it reads the layout, and a menu item learns to carry a zone drawing. The
  plan settles where that drawing is made.

## Several screens (parked, Z10)

Not built until a second screen is tested. The shape: the page draws every
screen as they sit, and each has its own grid, its own Full, Cinema and
Floating, and its own zones. A key belongs to one screen's zone, so keys
are unique across screens. `Topic::ZoneLayout` then holds a layout per
screen.

## Not in this slice

Several screens; uneven cells (dragging grid lines — use more cells);
a layout per workspace; Shift, Ctrl or Alt chords for zones; the titlebar
zone glyph (the same drawing, later).

## Risks

- **Many overlaps.** A layout with many overlapping zones can push off
  more than expected. The bar already shows each zone's shown tab that is
  off screen (full ink, no lift), so it stays readable.
- **Fullscreen through the shell** (Z15). Today sola-river grants it on the
  spot, and X games under XWayland fall back to an unmanaged surface if a
  request goes unanswered (`apply_fullscreen_requests`). The shell must
  answer every request, quickly.
