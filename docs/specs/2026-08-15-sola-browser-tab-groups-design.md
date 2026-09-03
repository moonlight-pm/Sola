# sola-browser tab groups

**Date:** 2026-08-15  
**Status:** **Frozen** — implemented; pocket + insert rules + **⌘⌥G** / hover rename + pocket color
**Branch / worktree:** `sola-browser`  
**Related:** [unified sidebar](2026-08-13-unified-sidebar-design.md); [profiles](2026-08-10-sola-browser-profiles-design.md); [session persist](2026-06-15-session-persistence-design.md)

| | |
|--|--|
| **Implementation** | kit inset pocket + hairline rim + **flush** members + lucide header; kit-owned drop (`Event::Drop` / `Dest`); header drag moves the block; title drop is a no-op; well extra / origin hole; reserved etch lip; chrome groups + persist + strip; **⌘⌥G** new group (`⌘G` is Find Next); edit-mode color chip (ink ring) + check; compact kit picker (swatch toggles; anchored past the check); black/semibold titles on light fills; hue-preserving selected lift + 1px lip |
| **Dogfood** | **installed** `kit` + `browser --release` 2026-08-31 (picker polish + contrast chip). ⌘T / OpenUrl append loose at bottom; ⌘-click still inserts beside; **⌘⌥G** focuses and selects the name (`⌘G` is Find Next); hover pencil to rename; edit mode shows a color chip and a check to save |
| **Gaps** | spaces; drag-to-create |

## Intent

Named, **collapsible folders** in the vertical tab strip. A group is a
contiguous block of tabs under a header. Spaces (switchable collections in
the same profile) come later and wrap this same column — they are not in
this slice.

Profiles stay “identity + workspace.” Groups do not get their own cookies.

## Product rules

| Rule | Choice |
|------|--------|
| Shape | In-strip folders. Group blocks and loose tabs **intermix** (Morph2). A group is a contiguous header+members atom; it may sit anywhere in the strip. |
| Spaces | Later. Persist must not assume a single global strip forever. |
| Membership | Drag across the group / loose boundary. |
| New group | **⌘⌥G** (Browser → New Group) on the **selected loose tab**. No-op if that tab is already in a group. Dragging one loose tab onto another does **not** create a group. `⌘G` is Find Next. |
| ⌘T / New Tab | Always loose, appended at the **end of the strip**. |
| Collapse + active | Stay on that page. Member rows hide. Header uses the selected etch. |
| Empty group | Dissolves. No parked empty headers. |
| Nesting | No. |
| Color | Pocket fill. Default is the kit inset well. Edit mode shows a contrast-ring chip (toggles a compact kit picker, anchored past the check) plus a check to commit. Light fills use black semibold titles; dark fills stay white. Selected / drag ghost is a quiet lift of the fill plus a 1px lip. Persist hex; omit when default. |
| Close-all | No. No `×` on the header. |

## Strip

```text
▾ Work
    tab
loose tab
▸ Research          ← collapsed; members omitted; etch if that page is showing
loose tab           ← ⌘T appends at the end of the strip
```

**Header (kit collapsible section, Large density):** lucide chevron + name
(folder caption, not a tab title). Collapsed headers may show a member
count. Members sit flush in the inset pocket (the well is the
containment — no extra indent). Not the uppercase settings section label.

**Click header** → expand / collapse (same **2px** click-vs-drag threshold as
tabs). Live reorder is kit Morph2 (hole + FLIP).

**Rename** → header field with the default name selected, as soon as ⌘G
creates the group. Enter commits, Esc reverts. Hover the header for a
pencil to rename later. Edit mode also shows a color swatch for the
pocket. There is no strip right-click.

## Drag

Visible rows are group headers, expanded members, and the loose run. Drop
targets are those rows plus the slots between them.

| Gesture | Result |
|---------|--------|
| Loose tab → slot among a group's members | Join that group at that index |
| Loose tab → floor of a group well (top half of the next header / first loose) | Join that group, **append** |
| Loose tab → group **title** | Invalid — tab returns to its origin |
| Grouped tab → slot in the loose run | Ungroup; insert at that index |
| Grouped tab → another group (member slot) | Move; member slot inserts |
| Grouped tab → another group's **title** | Invalid — tab returns to its origin |
| Last member dragged out | Group dissolves; tab is loose at the drop |
| Member among siblings | Reorder only |
| Loose among loose | Reorder only |
| Group header | The whole block moves. May land anywhere among groups **or** loose tabs. |

Dragging a hidden member is impossible (expand first).

Live preview must match the commit: a loose tab crossing into a group block
reads as joining, not as “order only.”

## New group (⌘⌥G)

Default name `Group`, then `Group 2`, …. The **selected loose tab**
becomes the first member; the header wraps it **in place** (groups and
loose tabs intermix). Starts expanded. The name field is focused with
the default selected so the next keystroke replaces it.

No-op when the selected tab is already in a group. Chrome owns ⌘⌥G (the
page does not see it). `⌘G` is Find Next.

Hover the header for a **pencil** to rename later (same overlay as
the tab ×). Edit mode also shows a **color swatch** (default well
fill) and a **check** to commit. Click the swatch for the kit color
picker; the pocket updates live. Title ink is black or white from
pocket luminance; the selected row (and drag ghost) is a quiet
lighten or darken of the fill plus a 1px lip (same job as the
ungrouped etch rim), not a grey overlay. Enter or the check commits the name; Esc reverts
the name (color already written).

Join / leave / dissolve stay **drag**. The tab strip has **no**
right-click menu.

The kit `context_menu` primitive remains for **page** right-click and
hold-back/forward history — not for tabs or group headers.

## Model and persist

Groups are **chrome-only**. CEF still sees a flat tab list. `TabId` stays a
runtime engine id and is not written to disk. Do not add `group_id` to
`TabInfo` / helper IPC.

**Runtime**

- `TabGroup { id, name, collapsed, color? }`
- Each tab has an optional `group_id` in chrome state
- After every mutation the vec is **normalized**: all group blocks (in group
  order), then the loose run

**`session.json` (additive; old files still load)**

```text
tabs: [ { url, title, group_id? }, … ]   # sidebar order
groups: [ { id, name, collapsed, color? }, … ]   # color is #rrggbb / #rrggbbaa
active_index, sidebar_w                  # unchanged
```

Restore: open tabs in vec order, mint new `TabId`s, reattach `group_id`.
Drop unknown ids and empty groups. If a group is split in the file, gather
members to the first run, then run the same top/loose normalize.

Spaces later attach as a wrapper (`space_id` or a `spaces[]` list) around
this column. Not written in this slice.

Fingerprint includes group id / name / collapsed / color so persist
dirties on rename, collapse, and recolor, not only url/title. Old
files without `color` still load (default well).

## Kit

**`SidebarSection` (opt-in; static labels unchanged)**

- `collapsible(collapsed, on_toggle)` — inset pocket; members nest one step
- `header_active(bool)` — selected etch when the collapsed group holds the
  current page
- `color(Option<Color>)` — pocket fill; `None` is the default inset well
- `header_content` + `header_color` + `header_color_picker` + `header_commit`
  — rename field, edit-mode swatch, anchored kit picker, check to save
- Pocket rows: `on_color` ink + hue-preserving lift for selected/hover
- Items are not built when `collapsed`
- List etch reserves the 1px lip on every row so selecting does not shift
  the title

Settings / mail / terminal / agent keep today’s inert labels.

**`context_menu`** — new component (`components/context_menu.rs`). Storybook
**Sidebar** page demos collapsible sections + row/header right-click; add a
Context menu page (or a section on Sidebar) in the **same change**.

Reorder maths live in kit (`sidebar::State` + `Event::Drop`). Chrome
applies `Dest` (join / loose / section order) to `Groups`. Kit does not
learn membership.

## Out of scope

- Spaces / switchable collections
- Nested groups
- Color pills on idle headers (swatch is edit-mode only)
- Drag-two-loose-tabs-to-create
- Close-all / header `×`
- Bookmarks, tab search, auto-group-by-domain
- Engine awareness of groups

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| Kit collapsible section + context menu | **done** (page + history; strip no longer uses it) |
| Chrome groups + persist + strip | **done** |
| ⌘G new group + inline rename | **done** (focus + select-all; hover pencil to rename later) |
| Pocket color | **done** (chip + check; compact picker; black/semibold on light; lift + lip; persist hex) |
| Dogfood | pocket + insert rules installed; **⌘G** installed `browser --release` 2026-08-29; pocket color + selected lift **installed** `kit`+`browser --release` 2026-08-31 |

## Decision log

| Date | Choice |
|------|--------|
| 2026-08-15 | Folders now; spaces later (same column, later wrapper) |
| 2026-08-15 | Groups at top; all loose tabs in one run at the bottom |
| 2026-08-15 | ⌘T always loose |
| 2026-08-15 | Collapse keeps the page; header selected |
| 2026-08-15 | Empty group dissolves |
| 2026-08-15 | Menu **and** drag for join / leave; New group is menu-only |
| 2026-08-29 | New group is **⌘G** on a selected loose tab (name selected). Strip right-click removed; join / leave stay drag |
| 2026-08-29 | Hover pencil on the group header to rename later |
| 2026-08-15 | Drag header reorders blocks only; does not ungroup |
| 2026-08-15 | Kit-native context menu + opt-in collapsible sections |
| 2026-08-15 | No nesting, no close-all (color later reversed) |
| 2026-08-31 | Pocket color: edit-mode swatch (default kit well); kit picker; persist `#rrggbb`/`#rrggbbaa`; omit when default |
