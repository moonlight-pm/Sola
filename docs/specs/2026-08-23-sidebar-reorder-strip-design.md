# Sidebar reorder strip

**Date:** 2026-08-23  
**Status:** Frozen / accepted — Morph2 is the reorder law  
**Branch / worktree:** `browser-polish`  
**Related:** [unified sidebar](2026-08-13-unified-sidebar-design.md); [tab groups](2026-08-15-sola-browser-tab-groups-design.md)  
**Reference:** `~/Workspace/Scratch/morph2.js` (working HTML/CSS/JS)

| | |
|--|--|
| **Implementation** | `ReorderStrip` — morphing hole + FLIP |
| **Dogfood** | kit storybook Sidebar; sola-browser tab strip |
| **Gaps** | terminal migration |

---

## Why extras failed

Animating `dest_slot` 0→H once, then hopping dest index, snaps later rows. Frozen rest vs live flex Y snapped the whole list 1px. Morph2 does not extra-offset a frozen list. It **moves a hole in the list** and FLIPs.

---

## Law (Morph2)

1. **`items` / view is truth.** Pickup splices the origin span and inserts one empty hole of the held kind and content height. Dest, wells, and layout run on the **view** (collapsed members omitted; the hole always shows).
2. **One hole.** `origin+1` means the hole stays. At most one hole move per frame (RedrawRequested), so pointer samples cannot stack FLIPs.
3. **Dest uses rest Y** (`offsetTop` equivalent), never transformed rects. Interior rows yield immediately (pointer above origin → insert-before). Last-member / first-after (C5 / U1) split on the **midline**. Coming **up** at U1 uses the whole row (C5 must not steal U1’s top half).
4. **Seam absorb.** C5 bottom half → join that group. Gap between C5 and U1 splits at its midpoint. Drop: absorb ⇒ `Join` append; else if the previous real row is a last member ⇒ `Loose`.
5. **Group drag.** Other groups are atoms. Swap at 50/50 of the **whole group box**. Click without crossing 2px toggles collapse.
6. **FLIP.** Snapshot visual tops, mutate hole, invert `translateY`, ease 180ms ease-out. A new hop may interrupt mid-ease (visual rect, not rest). Collapse/expand is the same FLIP; wells ease `top`/`height` 180ms.
7. **Integer pixels.** Pointer, Y, H, grab, 3px row gap, 6px group-end gap, 3px well pad. Row content height is 32px; trailing gap is margin, not hole height.
8. **Wells** painted from rest, not nested DOM. Bottom = `min(last.content, next.y − pad)` so extra group-end margin stays a gap.
9. **Ghost** follows `pointer − grab`. 2px threshold. Release without drag = click / toggle.
10. **Kit strip = groups.** Loose items are singletons. Groups and loose rows **intermix**; the browser does not force groups to the top.

---

## Drop payload

- Members of a named group → `Dest::Join { section, before }`
- Singleton / leave group → `Dest::Loose { before }`
- Before a named group title → `Dest::BeforeGroup { id }`
- Header-block reorder → `Dest::BlockBefore { before }` (group atom among mixed rows)

---

## Storybook

Group A/B/C + U1–U5. Click title to fold; drag title to move the group. Drag C4 through C5 and U1: join vs leave is a Y half.
