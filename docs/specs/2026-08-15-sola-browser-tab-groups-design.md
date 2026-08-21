# sola-browser tab groups

**Date:** 2026-08-15  
**Status:** **Frozen** — implemented on `browser-polish`; pocket + insert rules installed  
**Branch / worktree:** `browser-polish`  
**Related:** [unified sidebar](2026-08-13-unified-sidebar-design.md); [profiles](2026-08-10-sola-browser-profiles-design.md); [session persist](2026-06-15-session-persistence-design.md)

| | |
|--|--|
| **Implementation** | kit inset pocket + hairline rim + **flush** members + lucide header; header drag moves the block; title drop is a no-op; well extra / origin hole; reserved etch lip; chrome groups + persist + strip |
| **Dogfood** | **installed** this worktree. ⌘T / OpenUrl append loose at bottom; ⌘-click still inserts beside |
| **Gaps** | spaces; color; drag-to-create |

## Intent

Named, **collapsible folders** in the vertical tab strip. A group is a
contiguous block of tabs under a header. Spaces (switchable collections in
the same profile) come later and wrap this same column — they are not in
this slice.

Profiles stay “identity + workspace.” Groups do not get their own cookies.

## Product rules

| Rule | Choice |
|------|--------|
| Shape | In-strip folders. Groups stack at the **top**; every loose tab lives in **one run under them**. |
| Spaces | Later. Persist must not assume a single global strip forever. |
| Membership | Context menu **and** drag across the group / loose boundary. |
| New group | Context menu only. Dragging one loose tab onto another does **not** create a group. |
| ⌘T / New Tab | Always loose, appended at the bottom of the loose run. |
| Collapse + active | Stay on that page. Member rows hide. Header uses the selected etch. |
| Empty group | Dissolves. No parked empty headers. |
| Nesting | No. |
| Color | No. Quiet named header. |
| Close-all | No. No `×` on the header. |

## Strip

```text
▾ Work
    tab
    tab
▸ Research          ← collapsed; members omitted; etch if that page is showing
loose tab
loose tab           ← ⌘T lands here
```

**Header (kit collapsible section, Large density):** lucide chevron + name
(folder caption, not a tab title). Collapsed headers may show a member
count. Members sit flush in the inset pocket (the well is the
containment — no extra indent). Not the uppercase settings section label.

**Click header** → expand / collapse (same 5px click-vs-drag threshold as
tabs).

**Rename** → header field; Enter commits, Esc reverts. Offered from the
header menu.

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
| Group header among other headers | The whole block moves. Header drag **stays in the groups region** — it does not dump members into the loose run |

Dragging a hidden member is impossible (expand first, or use Ungroup).

Live preview must match the commit: a loose tab crossing into a group block
reads as joining, not as “order only.”

## Context menu

Kit has no right-click menu today (`popover` / `popover_anchored` only).
This slice adds a kit primitive. Browser is the first consumer — not a
private chrome menu.

**Kit**

- Flat actions, separators, disabled rows. **No submenu** in v1
- Opens at the **pointer**
- Outside click / Escape dismisses
- App owns `Option<MenuState>` (one menu per window)

**Sidebar hook (opt-in):** `SidebarItem::on_context` and the same on a
collapsible header. Right-click does **not** start reorder.

**Tab row**

| Item | When |
|------|------|
| New group | always |
| Add to *Name* | one row per **other** group |
| Ungroup | tab is in a group |

No Close in the menu (hover `×` stays).

**Group header**

| Item | When |
|------|------|
| Rename | always |
| Ungroup | always — dissolve; members go to the **end** of the loose run, keeping relative order |

**New group:** default name `Group`, then `Group 2`, …. The clicked tab
becomes the first member; the new block is inserted at the **end of the
groups region**. If that tab was already in a group it leaves first (and
dissolves the old group when it was the last member). Starts expanded.

**Add to *Name*:** tab moves to the **end** of that block.

**Ungroup (tab):** tab goes to the **end** of the loose run.

## Model and persist

Groups are **chrome-only**. CEF still sees a flat tab list. `TabId` stays a
runtime engine id and is not written to disk. Do not add `group_id` to
`TabInfo` / helper IPC.

**Runtime**

- `TabGroup { id, name, collapsed }`
- Each tab has an optional `group_id` in chrome state
- After every mutation the vec is **normalized**: all group blocks (in group
  order), then the loose run

**`session.json` (additive; old files still load)**

```text
tabs: [ { url, title, group_id? }, … ]   # sidebar order
groups: [ { id, name, collapsed }, … ]
active_index, sidebar_w                  # unchanged
```

Restore: open tabs in vec order, mint new `TabId`s, reattach `group_id`.
Drop unknown ids and empty groups. If a group is split in the file, gather
members to the first run, then run the same top/loose normalize.

Spaces later attach as a wrapper (`space_id` or a `spaces[]` list) around
this column. Not written in this slice.

Fingerprint includes group id / name / collapsed so persist dirties on
rename and collapse, not only url/title.

## Kit

**`SidebarSection` (opt-in; static labels unchanged)**

- `collapsible(collapsed, on_toggle)` — inset pocket; members nest one step
- `header_active(bool)` — selected etch when the collapsed group holds the
  current page
- Items are not built when `collapsed`
- List etch reserves the 1px lip on every row so selecting does not shift
  the title

Settings / mail / terminal / agent keep today’s inert labels.

**`context_menu`** — new component (`components/context_menu.rs`). Storybook
**Sidebar** page demos collapsible sections + row/header right-click; add a
Context menu page (or a section on Sidebar) in the **same change**.

Reorder maths stay app-driven (`ReorderCfg` + chrome `finish_reorder`).
Chrome maps visible-row index → tab or header and applies the table above.
Kit does not learn membership.

## Out of scope

- Spaces / switchable collections
- Nested groups
- Color pills
- Drag-two-loose-tabs-to-create
- Close-all / header `×`
- Bookmarks, tab search, auto-group-by-domain
- Engine awareness of groups

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| Kit collapsible section + context menu | **done** |
| Chrome groups + persist + strip | **done** |
| Dogfood | installed (pocket; ⌘T / OpenUrl append loose; ⌘-click beside) |

## Decision log

| Date | Choice |
|------|--------|
| 2026-08-15 | Folders now; spaces later (same column, later wrapper) |
| 2026-08-15 | Groups at top; all loose tabs in one run at the bottom |
| 2026-08-15 | ⌘T always loose |
| 2026-08-15 | Collapse keeps the page; header selected |
| 2026-08-15 | Empty group dissolves |
| 2026-08-15 | Menu **and** drag for join / leave; New group is menu-only |
| 2026-08-15 | Drag header reorders blocks only; does not ungroup |
| 2026-08-15 | Kit-native context menu + opt-in collapsible sections |
| 2026-08-15 | No color, no nesting, no close-all |
