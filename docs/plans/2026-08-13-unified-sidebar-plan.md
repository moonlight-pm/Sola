# Unified sidebar — implementation plan

**Date:** 2026-08-13  
**Worktree:** `.worktrees/unified-sidebar` · branch `feature/unified-sidebar`  
**Design:** [`../specs/2026-08-13-unified-sidebar-design.md`](../specs/2026-08-13-unified-sidebar-design.md)  
**Status:** checklist — do not start code until design is accepted

---

## Phase 0 — Align (no code)

- [ ] Human accepts design north star (browser etch = default list).
- [ ] Resolve design Decision points (terminal density, Row rename,
      browser divider, selection atom).
- [ ] Point `CURRENT.md` **Now** at this plan when work starts.

---

## Phase 1 — Kit: etch list chrome (no app migrates yet)

**Files:** `crates/sola-kit/src/components/sidebar.rs` (+ `style.rs` if
needed), storybook `pages/sidebar.rs`.

- [ ] Extract / share materials from `tab_item_style`, `tab_etch_lip`,
      `inset_surface(CHROME_SURFACE, …)` into the **Row/list** path of
      `item_style_chrome` and `row_container_style`.
- [ ] Idle list text: muted; active: full fg + `ui_medium` on primary label.
- [ ] Column / panel body background: `CHROME_SURFACE` (match
      `tab_column_style` / existing panel chrome surface).
- [ ] Add **density** (`SidebarDensity` or reuse `TabSize`) applied by
      `SidebarPanel` (pad, font, default `item_spacing` for list).
- [ ] **Floating close:** `on_close` renders hover-only stacked × when
      hover is known; document that `item_hover` + `id` is required for
      close-on-hover (or auto-wire index ids).
- [ ] Preserve **Card** branch pixel-for-pixel (agent).
- [ ] Unit tests: density metrics stable; chrome match arms don’t collapse
      Card into etch.
- [ ] Storybook Sidebar page: show etch strip (Normal + Large) + existing
      panel features; keep vertical_tabs demo only as “legacy adapter”
      until Phase 3.
- [ ] `cargo make build` kit (+ storybook target if separate).

**Exit:** storybook list rows look like browser tabs; agent card demo
still correct; no production app forced yet (they pick up etch on next
rebuild — acceptable once design accepted).

---

## Phase 2 — Compatibility adapter

- [ ] Implement `vertical_tabs_sized` as a thin wrapper:
      one unlabeled `SidebarSection` of items with `on_close` + density +
      hover, optionally without panel resize (caller supplies width).
- [ ] Or: keep old function body until browser migrates, then delete
      (choose based on Phase 1 risk). Prefer wrapper so materials stay
      single-sourced immediately.

**Exit:** browser binary rebuilt against kit still looks correct without
app edits (if wrapper); or Phase 3 is mandatory same PR.

---

## Phase 3 — Migrate sola-browser

**Files:** `crates/sola-browser/src/app.rs` (view layout), maybe session
unchanged.

- [ ] Replace `view_tab_sidebar` `TabDescriptor` / `vertical_tabs_sized`
      with `SidebarItem`s (`label`, `active`, activate message, `on_close`,
      `.id(tab_id)`).
- [ ] Wire `item_hover` from existing `hovered_tab` (map index ↔ id or
      store `Option<TabId>`).
- [ ] Prefer `SidebarPanel::new(…).density(Large).item_hover(…).`
      + `resizable_with` using existing divider colours; remove duplicate
      full-window drag overlay if panel already provides it.
- [ ] Keep full-width chrome bar + profile select unchanged.
- [ ] Build browser; user smokes: tab activate, hover close, resize,
      profile bar alignment with column width.

**Exit:** browser has **zero** direct use of `vertical_tabs*`.

---

## Phase 4 — Terminal / light consumers

- [ ] **Terminal:** set density Large (or agreed default); confirm reorder,
      shortcuts, divider→term_bg still correct under etch materials.
      Minimal code if Phase 1 already redefaults Row.
- [ ] **Settings / mail / preview / storybook nav:** no API change
      expected; visual smoke only.
- [ ] **Agent:** visual smoke cards only.

**Exit:** terminal tab strip reads as sibling to browser tabs.

---

## Phase 5 — Delete parallel API

- [ ] Remove `TabDescriptor`, `vertical_tabs`, `vertical_tabs_sized`, and
      public `TabSize` if fully replaced by `SidebarDensity` (or type-alias
      deprecate one release — prefer delete in same branch; no public
      crates.io consumers).
- [ ] Clean `components/mod.rs` re-exports.
- [ ] Storybook: drop legacy dual-column vertical_tabs demo.
- [ ] Grep workspace for leftovers.

**Exit:** one public sidebar composition path.

---

## Phase 6 — Docs (mandatory with ship)

- [ ] `CURRENT.md` — Now / dogfood if chrome language changed.
- [ ] `docs/capabilities.md` — kit sidebar / browser / terminal rows if
      maturity or gaps change.
- [ ] `docs/architecture.md` only if the system map mentions dual APIs.
- [ ] `docs/manual/` only if operator-facing descriptions of chrome
      change (usually N/A).
- [ ] Mark this plan complete; design status → accepted/landed.
- [ ] Follow `.grok/skills/sola-progress-docs`.

---

## Suggested PR / commit slices

1. **kit: etch list chrome + density + floating close**  
2. **browser: SidebarPanel tab strip**  
3. **kit: remove vertical_tabs API + storybook cleanup**  
4. **docs: progress**  

(Or 1+2 together if you want screenshot parity in one dogfood install.)

---

## Verification

```bash
# from worktree
cargo make build kit
cargo make build browser terminal agent settings mail preview
# install only with user OK:
# cargo make install kit browser terminal
```

Manual dogfood:

1. Storybook → Sidebar: etch Normal/Large, reorder, resize, cards.  
2. Browser: many tabs, close hover, resize column, switch profile.  
3. Terminal: reorder tabs, shortcuts, resize against black grid.  
4. Agent: session cards + filter + delete hover.  
5. Settings: Applications / Mail nav select.

---

## Effort (rough)

| Phase | Size |
|-------|------|
| 0 Align | short conversation |
| 1 Kit etch + density + close | medium (careful render_item paths) |
| 2 Adapter | small |
| 3 Browser migrate | small–medium (hover id + layout) |
| 4 Terminal / sweep | small |
| 5 Delete API | small |
| 6 Docs | small |

Highest risk is **Phase 1** `render_item` (plain vs reorder vs hover_action
vs close). Touch with tests and storybook, not by guessing in browser only.
