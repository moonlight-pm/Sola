# Unified sidebar — design freeze

**Date:** 2026-08-13  
**Branch / worktree:** `feature/unified-sidebar` · `.worktrees/unified-sidebar`  
**Status:** accepted / landed on master (2026-08-13)  
**Companion plan:** [`../plans/2026-08-13-unified-sidebar-plan.md`](../plans/2026-08-13-unified-sidebar-plan.md)

| | |
|--|--|
| **Implementation** | Kit `SidebarDensity` + etch list chrome; **kit-owned gesture** (`sidebar::State` + `Event`); browser/terminal/agent/workspaces on `SidebarPanel`; `vertical_tabs*` deleted |
| **Dogfood** | Gesture rewrite on `browser-polish` (2026-08-21) — **not installed**. Overflow chips require `section_scroll` + a measured viewport |
| **Gaps** | Monitor sticky list still custom; etch tokens not on the bus |

---

## Goal

One kit surface — **`SidebarPanel` / `SidebarItem` / `SidebarSection`** (+ thin
`sidebar()` helper) — that every app with a left nav / tab strip uses.

**Visual north star:** the browser’s etched vertical tab column
(`vertical_tabs_sized` · `TabSize::Large` after `0918b3d4`), not the older
selection-wash “nav pill” rows.

**Not a goal:** force agent session cards into the same material as browser
tabs. **Card** chrome stays a first-class second look for rich product rows.
The *API* unifies; the *chrome enum* still selects materials.

---

## Problem (as-built)

Three parallel looks, two public APIs, one module:

| Surface | API | Selection material | Used by |
|---------|-----|--------------------|---------|
| **Etched tabs** | `vertical_tabs` / `vertical_tabs_sized` + `TabDescriptor` + `TabSize` | Idle: transparent + muted type; active: 1px lip + hue-preserving lift of `CHROME_SURFACE` (same recipe as group pockets); hover `×` floats | **sola-browser** |
| **Row** | `SidebarItem` default / `sidebar()` / `SidebarPanel` | Quiet `theme::selection()` wash, packed pad, optional always-on close sibling | **terminal**, **settings**, **mail**, **preview**, storybook nav |
| **Card** | `SidebarItem::card()` + custom `content` on `SidebarPanel` | OD graphite raised cards, surface-only select | **sola-agent** sessions |

Monitor still rolls a fully custom sticky list (out of scope for v1 unless
easy).

The browser redesign **did not** restyle `SidebarItemChrome::Row` or migrate
browser onto `SidebarPanel`. Terminal and browser therefore diverge even
though both live under `crates/sola-kit/src/components/sidebar.rs`.

---

## Design principles

1. **One composition model** — sections of items; parent-controlled
   `active`; panel owns optional chrome (header, footer, resize, reorder,
   collapse, fill-scroll chips, item hover).
2. **Chrome is a knob, not a fork** — list etch vs product card vs (future)
   others. No second top-level widget for “tabs.”
3. **Browser appearance is the default list chrome** — quiet title stack,
   etched active, muted idle, no selection-teal wash on rows.
4. **Capabilities stay opt-in** — reorder, shortcuts, close, secondary,
   subtitle, indicator, hover_action, custom content: default off / empty.
5. **Kit owns gesture, hover, and animation** — the consumer holds an
   opaque [`State`] and maps [`Msg`] through [`State::update`]. Product
   meaning arrives as [`Event`] (`Activate`, `ToggleSection`, `Drop`,
   `Resize`). Overlay captures pointer while a press is live.
6. **Strictly additive migration where possible** — rename/redefault chrome;
   deprecate `vertical_tabs*` with thin wrappers until the last caller is
   gone, then delete.

---

## Target anatomy

```text
SidebarPanel
├── [optional] collapse rail
├── [optional] header (search, brand, …)
├── section*
│   ├── [optional] section label
│   ├── [optional] ↑ overflow chip
│   ├── items…  (scroll if section.fill)
│   └── [optional] ↓ overflow chip
├── [optional] footer
└── [optional] vertical resize divider (+ drag overlay)
```

**Item (list etch):**

```text
┌─────────────────────────────────────┐  column: CHROME_SURFACE
│  title…                    [1]  [×] │  idle: flat / muted
│  ░░░░░ active etch well ░░░░░  [×] │  active: 1px lip + inset fill
└─────────────────────────────────────┘  × only while hovered (stack)
```

**Item (card):** unchanged agent/OD contract — raised graphite, hairline,
optional custom body, spacing via `item_spacing`.

---

## API shape (desired)

### Keep

- `SidebarItem`, `SidebarSection`, `SidebarPanel`, `sidebar` /
  `sidebar_with_header`
- Panel: `header`, `footer`, `collapsible`, `resizable` /
  `resizable_with`, `reorderable`, `section_scroll`, `item_hover`,
  `item_spacing`
- Item: `active`, `shortcut`, `on_close`, `secondary`, `subtitle`,
  `on_double_click`, `indicator`, `id`, `hover_action`, `chrome`,
  `content`, `height_hint`

### Change (visual + small API)

| Change | Detail |
|--------|--------|
| **Default list chrome = etch** | `SidebarItemChrome::Row` either renamed to `List` (or `Etch`) **or** kept as name but **materials** match `tab_item_style` / `tab_etch_lip` / inset well. Prefer rename only if call sites are few; otherwise redefault materials and document. |
| **Column surface** | Panel / `sidebar()` column fill uses `CHROME_SURFACE` (same as vertical tabs + browser chrome strip), not ad-hoc raised-only styling, unless a panel-level override is supplied later. |
| **Active type** | Active list rows use `fonts::ui_medium()` for the primary label (browser does today). |
| **Floating close** | When `on_close` is set, default to **hover-only stacked `×`** (vertical_tabs pattern), not a permanent trailing sibling that steals width. Reorder path must keep press on the row body without the `×` eating the gesture (already partially solved for hover_action). |
| **Density** | Promote `TabSize::{Normal, Large}` (or rename `SidebarDensity`) onto **panel or item defaults**: pad, font size, inter-item gap. Browser → Large; settings/mail/storybook → Normal; terminal → Large or Normal (pick in plan; default Large for tab strips). |
| **Hover tracking for close** | List rows with `on_close` need hover id/index. Today vertical_tabs uses index + `on_hover`; panel uses string `id` + `item_hover`. Unify on **`item_hover` + optional `id`**, auto-generating stable ids for close-only rows if needed, *or* allow index-based hover when reorder is off. Prefer id-based everywhere for one code path. |
| **Deprecate** | `TabDescriptor`, `vertical_tabs`, `vertical_tabs_sized`, public `TabSize` once browser is migrated. Temporary: implement vertical_tabs as a thin adapter over `SidebarPanel` / items so nothing breaks mid-PR. |

### Explicit non-goals (v1)

- Drag-reorder live preview changes beyond what Row already has  
- Collapsing browser profile into the tab column again (profile stays in
  chrome bar)  
- Unifying monitor’s sticky list  
- Theming etch depth via bus atoms (hardcode graphite math like today;
  atomize later if storybook needs it)

---

## Consumer migration matrix

| Consumer | Today | Target |
|----------|-------|--------|
| **sola-browser** | App-built width + `vertical_tabs_sized` + separate `vertical_divider_with` | `SidebarPanel` unlabeled section of etch items (`on_close`, density Large, **`reorderable`**). Prefer **panel `resizable_with`** so divider/overlay match terminal/agent; drop duplicate drag overlay if panel already stacks it. Profile picker stays in chrome bar. |
| **sola-terminal** | `SidebarPanel` Row + reorder + shortcuts | Same structure; free visual upgrade to etch. Keep reorder + `1`…`9` shortcuts. Density: Large (tab strip). |
| **sola-agent** | Card + custom content + hover + scroll chips | Unchanged API; ensure list etch changes do not bleed into Card styles. |
| **sola-settings / mail / preview** | `sidebar(sections)` Row | Same call sites; inherit etch list look. Density Normal. |
| **storybook** | Nav via `sidebar()`; Sidebar page dogfoods panel + vertical_tabs demo | Nav inherits etch; Sidebar page becomes **one** showcase (etch strip, card stack, density, reorder, close). Remove dual vertical_tabs column demo after delete. |
| **sola-monitor** | Custom | Optional later; not blocking. |

---

## Behaviour contracts to preserve

### Terminal

- Click-without-drag selects (reorder threshold).  
- Live sibling glide while dragging.  
- Resize divider colours: raised/chrome | line | **term cell bg**.  
- Shortcut hints on first 9 tabs.

### Browser

- Hover reveals close; close never reflows title width.  
- Active etch (not teal selection).  
- Truncation remains caller-side (title/url) with `Wrapping::None`,
  width-aware so the etch well fills (no fixed 20-char cap).  
- Drag-reorder via panel `reorderable` (same click-vs-drag threshold as
  terminal); chrome owns order (`merge_tab_snapshot` keeps it).  
- New tab still ⌘T / app chrome — no required “+” footer in v1.  
- Instant close / tab list from cache still works (only view path changes).

### Agent

- Card surfaces, context bar body, hover × in rail, section fill + overflow
  chips, header search — no visual regression.

### Settings / mail / preview

- Section labels still render.  
- Single-click select.  
- No accidental reorder or close affordances.

---

## Module / code layout (suggested)

Keep one crate path; optionally split for readability after behaviour is
stable:

```text
components/sidebar/
  mod.rs          // re-exports
  item.rs         // SidebarItem, chrome, render_item
  panel.rs        // SidebarPanel, sidebar()
  etch.rs         // list materials (from vertical_tabs styles)
  card.rs         // card_surface_style
  reorder.rs      // ReorderAnim, pure geometry (already tested)
```

v1 may stay in a single `sidebar.rs` if the diff is clearer; split is
cleanup, not a product requirement.

---

## Storybook

- Storybook **Sidebar** page is the dogfood for etch density + panel
  features.  
- Per kit-storybook rule: always update the matching storybook page.

---

## Risks

| Risk | Mitigation |
|------|------------|
| Global visual jolt for settings/mail | Intentional; ship kit first, dogfood storybook + one app, then rest |
| Close + reorder gesture conflict | Reuse hover_action stack pattern; keep × outside reorder press target |
| Floating close needs hover state apps don’t have | Panel `item_hover` required when any item has `on_close`; browser already has hover — map to ids |
| Density defaults wrong for terminal | Plan picks Large; one-line flip if dogfood hates it |
| Card regressions from shared render path | Keep `match chrome` branches; tests for card_surface vs etch |
| Browser layout: chrome bar vs sidebar width | Preserve full-width chrome bar; panel only owns lower tab column |

---

## Success criteria

1. No production caller of `vertical_tabs` / `TabDescriptor` (wrappers
   deleted or `#[deprecated]` and unused).  
2. Browser tab strip **looks** like today’s etched column (screenshot parity).  
3. Terminal uses the same item materials; reorder/resize still work.  
4. Agent session cards unchanged by eye.  
5. Storybook Sidebar page documents density + chrome variants.  
6. `docs/capabilities.md` / `CURRENT.md` / manual only if operator-visible
   chrome description changes.

---

## Decision points (resolved 2026-08-13)

1. **Density for terminal** — **Large** (match browser).  
2. **Rename `Row` → `List`/`Etch`?** — **No.** Redefaulted materials under `Row`.  
3. **Browser divider ownership** — **Same change** as item migrate (`resizable_with`).  
4. **Selection atom** — **Yes.** Settings / mail / preview nav use etch, not teal.

---

## Out of scope / later

- Atom-editable etch tokens in theme editor  
- Monitor sticky list on `SidebarPanel`  
- Mail “unread count” polish beyond existing secondary  
- Icon-leading rows as a first-class item field (callers can still use
  `content`)
