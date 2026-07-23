# P7 — Kit controls implementation plan

> **For agentic workers:** Execute pass-by-pass in worktrees. One signature
> move per pass; `cargo make build` only — never install without explicit
> user permission. Prefer storybook screenshots for visual stops.
>
> **Parent roadmap:** `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` §4 P7  
> **North star:** `docs/manual/design-language.md`  
> **Audit source:** sola-kit examination (session 2026-07-21)

**Goal:** Make sola-kit controls read as macOS Dark Mode density and calm —
correct theme bindings on shell chrome, consistent type/control sizes,
quiet selection/hover, storybook-complete — so P8 apps can inherit without
per-app snowflakes.

**Architecture:** Tokens and kit style fns first; no shell layout rewrites
in this phase. Fix binding leaks before visual density so shell and storybook
share the same palette map. Grow missing form primitives only where settings
and other kit apps already need them.

**Tech stack:** Rust, iced 0.14, `crates/sola-kit`, storybook binary.

## Global constraints

- Worktrees only under `.worktrees/` — never commit feature work on `master`
  directly.
- `cargo make build` (or `cargo make build kit`) to verify; **no**
  `cargo make install` without express user permission each time.
- Tokens / palette slots / `style::{RADIUS_*, SPACE_*}` before view hex or
  raw padding literals.
- Storybook page updated for every visual kit change that has a page.
- One pass = one signature move; stop for visual inspection after each
  mergeable unit unless the user batches.
- Do not invent SF Symbols, vibrancy/blur, or traffic lights on zoned
  windows (design language §1 / deferred table).
- Prefer macOS answer when not an explicit departure (design language §1).

## Out of scope for P7 (defer)

| Item | Why / when |
|------|------------|
| P8 restyle of settings / monitor / agent / browser | Separate phase after kit primitives exist |
| Real compositor blur / vibrancy | Roadmap deferred |
| SF Symbols / replacing Lucide | Asset + product decision |
| Collapsing forked `text_input` to iced stock | Strategy task only documents; rewrite is its own project |
| Focus-ring infrastructure for all controls | Optional late pass if time; iced Status is pointer-centric |
| SidebarPanel API rewrite | Heavy, working; density-only tweaks only |
| Automated pixel-diff CI | Roadmap deferred |

---

## File map (where work lands)

| Path | Role in P7 |
|------|------------|
| `crates/sola-kit/src/theme.rs` | `overlay` / `menubar` Extended binding; selection install already here |
| `crates/sola-kit/src/components/style.rs` | Optional control-pad constants; SPACE scale notes |
| `crates/sola-kit/src/components/button.rs` | Density, ghost hover, control-size helpers |
| `crates/sola-kit/src/components/text.rs` | Type role sizes (body 13, etc.) |
| `crates/sola-kit/src/components/field.rs` | Label density, SPACE_*, error optional |
| `crates/sola-kit/src/components/text_input/mod.rs` | Padding, selection color, style only (not fork rewrite) |
| `crates/sola-kit/src/components/badge.rs` | Quieter neutral default treatment |
| `crates/sola-kit/src/components/card.rs` | Optional borderless elevation; no layout change |
| `crates/sola-kit/src/components/sidebar.rs` | Section header style, pad on SPACE scale |
| `crates/sola-kit/src/components/toolbar.rs` | Align size with control system |
| `crates/sola-kit/src/storybook/pages/*` | Regression surfaces per pass |
| `docs/manual/design-language.md` | Only if a new intentional departure or type table change |
| `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` | Check off P7 sub-items as passes merge |
| `.grok/rules/active-work.md` | Point Current at next incomplete pass |

Shell (`sola-shell`) should need **no** visual rewrites for Pass A (binding
fix only). Pass B–E may require zero shell changes if shell already uses kit
styles and tokens.

---

## Pass overview

| Pass | Signature move | Risk | Depends |
|------|----------------|------|---------|
| **A** | Fix `overlay` / `menubar` Extended binding | Correctness; subtle color shifts in shell chrome | — |
| **B** | Type + control density baseline | Visible kit/storybook density | A recommended first |
| **C** | Quiet interactions (ghost, primary, selection unify) | Control calm | B |
| **D** | Form primitives (field polish + settings row + toggle/checkbox styles) | Unblocks P8 settings | B |
| **E** | Surfaces polish (badge, card, sidebar headers) + storybook completeness | Polish | C |
| **F** | Docs + consumer note + handoff to P8 | Process | E |

Each pass is one worktree branch, one mergeable unit, one visual stop.

---

## Pass A — Theme binding: overlay & menubar

**Why first:** Shell paints menus, launcher, switcher, and menubar through
`theme::overlay` / `theme::menubar`, which currently call iced’s
`Extended::generate` and **drop** the sola atom→slot map from
`extended_from_atoms`. Kit style fns that read `background.weaker` /
`strong` / `stronger` get the wrong hierarchy on every shell surface.

**Files:**
- Modify: `crates/sola-kit/src/theme.rs` (`overlay`, `menubar`, tests)
- No storybook page required (logic); optional after-shot of menu open vs
  baseline `02-menu-open.png`

### Design

1. Extract or reuse a path that starts from **sola’s** Extended (same as
   `build_theme` / `extended_from_atoms`), not iced’s generator.
2. **`overlay`:** keep all tiers from the base sola theme; force only
   `background.base.color = TRANSPARENT` (preserve base `.text` and every
   other tier alpha/color). Prefer cloning the base theme’s extended
   palette over regenerating from a stripped `Palette`.
3. **`menubar`:** start from sola Extended; rebind background ladder so
   chrome over black still works:
   - `background.base` fill = supplied `bg` (menubar token), text from
     base `fg`
   - Do **not** lose `primary`, `secondary`, `success`, `warning`,
     `danger`, or raised/hover/border mapping used by kit buttons
   - Hover fills for menubar labels already use fg-alpha in
     `button::menubar` — leave that style alone
4. Expand unit tests beyond “base transparent / black-ish”:
   - Overlay: `background.weaker` equals `default_theme()`’s weaker
     (raised atom), not iced-generated mid-grey
   - Overlay: `background.stronger` equals border atom path
   - Menubar: `primary.base` unchanged; `secondary.base.text` still muted
   - Menubar: `background.base.color` is the supplied `bg`

### Implementation sketch

```rust
// Preferred approach — mutate a full sola Extended, don't Extended::generate:
fn extended_from_theme(base: &Theme) -> Extended {
    // Option 1: re-derive via atoms_from palette if available
    // Option 2: copy base.extended_palette() fields (Extended is Copy-ish)
    *base.extended_palette()
}

pub fn overlay(base: &Theme) -> Theme {
    let palette = base.palette();
    let mut ext = *base.extended_palette();
    ext.background.base.color = Color::TRANSPARENT;
    Theme::custom_with_fn("sola-overlay".into(), palette, move |_| ext)
}

pub fn menubar(base: &Theme, bg: Color) -> Theme {
    let mut palette = base.palette();
    palette.background = bg;
    let mut ext = *base.extended_palette();
    // Re-pair base surface to menubar bg; keep text/fg from sola atoms.
    let fg = ext.background.base.text;
    ext.background.base = Pair::new(bg, fg);
    // weakest often aliases base for scrollables — keep menubar consistent
    ext.background.weakest = Pair::new(bg, fg);
    Theme::custom_with_fn("sola-menubar".into(), palette, move |_| ext)
}
```

Exact field choices for menubar mid-tiers: keep raised/hover/border from
the sola map (do not recolor them to black). Menubar is a thin strip; row
hovers use `button::menubar`’s own alpha, not `background.strong`.

### Acceptance

- [ ] Unit tests for overlay/menubar assert sola tier identity (weaker =
      raised, stronger = border path)
- [ ] Existing overlay/menubar tests still pass
- [ ] `cargo make build kit` (or full `cargo make build`) succeeds
- [ ] User install smoke (when permitted): open menu / launcher — no
      washed-out or Primer-like greys; popover/card chrome matches storybook

### Visual stop

Optional: `docs/visual/passes/p7a-theme-binding/` menu + launcher shots
vs baseline. If colors look unchanged, still merge — correctness may be
invisible until a control that relied on wrong tiers is exercised.

### Commit message shape

`fix(kit): preserve sola Extended binding in overlay/menubar themes`

---

## Pass B — Type & control density baseline

**Signature move:** Align kit type and default control metrics with shell
chrome density (macOS-like 13 body, compact pads) without redesigning
layouts.

**Files:**
- Modify: `crates/sola-kit/src/components/text.rs`
- Modify: `crates/sola-kit/src/components/style.rs` (add named control pads)
- Modify: `crates/sola-kit/src/components/button.rs` (helpers that apply pad + type)
- Modify: `crates/sola-kit/src/components/toolbar.rs` (align with helpers)
- Modify: `crates/sola-kit/src/components/field.rs` (label uses body size path)
- Modify: `crates/sola-kit/src/components/text_input/mod.rs` (`DEFAULT_PADDING`)
- Modify: storybook `pages/text.rs`, `pages/button.rs`, `pages/field.rs`
- Optionally note in `docs/manual/design-language.md` §2.4 size table if
  body size is stated there

### Concrete metrics (lock these)

| Token / helper | Value | Use |
|----------------|-------|-----|
| `text::body` | **13** (was 14) | Default UI copy |
| `text::caption` | **11** | Secondary / help |
| `text::subheading` | **15** (was 18) | Section titles in apps |
| `text::heading` | **22** (was 24) | Page titles |
| `text::code` | **12** | Unchanged |
| `PAD_CONTROL` | `[5, 12]` | Regular button content pad |
| `PAD_CONTROL_SM` | `[3, 10]` | Compact / toolbar |
| `text_input::DEFAULT_PADDING` | **4** vertical-ish → `Padding::from([4, 8])` if API allows, else `Padding::new(4.0)` → prefer `[4, 8]` | Fields |
| Field label | `body` + `muted` (13), not caption | macOS form label weight |

Add to `style.rs`:

```rust
/// Regular control content padding (buttons, default actions).
pub const PAD_CONTROL: [u16; 2] = [5, 12];
/// Compact control padding (toolbar, steppers, dense chrome).
pub const PAD_CONTROL_SM: [u16; 2] = [3, 10];
```

### Button helpers (additive, non-breaking)

Keep existing style fns (`primary`, `secondary`, …). Add factories that
bake density so apps stop inventing pads:

```rust
// button.rs — names indicative
pub fn labeled<'a, Message: Clone + 'a>(
    label: impl text::IntoFragment<'a>,
    style_fn: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> button::Button<'a, Message> {
    button(text(label).font(fonts::ui()).size(13))
        .padding(PAD_CONTROL)
        .style(style_fn)
}

// optional compact:
pub fn labeled_sm(...) // size 12, PAD_CONTROL_SM
```

Wire `toolbar_button` to `PAD_CONTROL_SM` + size 12 (already close).

### Field

```rust
// field.rs
let label_el = body(label.into()).style(muted); // 13 muted, not caption
let mut col = column![label_el, input.into()].spacing(SPACE_SM);
// help stays caption + muted
```

### Acceptance

- [ ] Text role sizes match the table (unit test optional; storybook is
      primary)
- [ ] Field page shows 13pt labels; buttons page documents `labeled` /
      default pads
- [ ] No new hex; pads use named constants
- [ ] `cargo make build` OK
- [ ] Visual: storybook Text + Button + Field pages

### Commit

`feat(kit): densify type roles and control padding (P7b)`

---

## Pass C — Quiet interactions

**Signature move:** Accent becomes sparse again on everyday controls;
selection is one language.

**Files:**
- Modify: `crates/sola-kit/src/components/button.rs` (`ghost`, maybe
  `primary` comment / optional neutral default — see below)
- Modify: `crates/sola-kit/src/components/text_input/mod.rs` (`style`
  selection color → `theme::selection()` or a softer mix)
- Modify: storybook `pages/button.rs` (danger_outline, confirm, ghost hover)

### Rules

1. **`ghost`:** hover/press lift background only; **keep text**
   `background.base.text` (do not switch to `primary.base.color`).
2. **`primary`:** keep filled accent for true primary actions, but
   storybook copy must say “one primary per group.” No new style required
   unless visual review wants a quieter filled (e.g. `background.strong`
   fill + accent text) — **default stay cyan fill** unless user asks
   otherwise after screenshots (cyan is product identity).
3. **Text input selection:** set `selection` field in kit `style` to
   `crate::theme::selection()` (quiet teal-grey), not `primary.weak`.
4. **Focused border:** remain `primary.base` (accent as focus signal is
   correct HIG use of accent).

### Acceptance

- [ ] Ghost hover is grey lift, not cyan text
- [ ] Focused field still shows accent border
- [ ] Selected text in inputs uses selection atom
- [ ] Storybook Button page includes ghost + confirm_button
- [ ] Visual stop: Button + Field storybook

### Commit

`fix(kit): quiet ghost hover and unify text selection with selection atom`

---

## Pass D — Form primitives

**Signature move:** Give kit apps a settings-grade form path so P8 does
not invent rows.

**Files:**
- Modify: `crates/sola-kit/src/components/field.rs` (error line optional)
- Create or extend: `crates/sola-kit/src/components/form.rs` (or keep in
  `field.rs` if small) — `form_row`, checkbox/toggle **styles**
- Modify: `crates/sola-kit/src/components/mod.rs` re-exports
- Create: `crates/sola-kit/src/storybook/pages/form.rs` (or expand Field)
- Modify: storybook `Page` enum + nav

### API (minimal, parent-owned state)

```rust
/// Stacked label + control + optional help + optional error (danger caption).
pub fn field(...) // extend with `error: Option<&str>`

/// Horizontal settings row: label (left, fill) | control (right, shrink).
/// Height ~28–32; vertical align center; no card chrome.
pub fn form_row<'a, Message: 'a>(
    label: impl Into<String>,
    control: impl Into<Element<'a, Message, Theme>>,
) -> Row<'a, Message, Theme>

/// Style helpers for iced checkbox / toggler if iced 0.14 exposes them;
/// otherwise small composed widgets:
pub fn checkbox_style(...)
pub fn toggle_style(...)  // or `toggler` wrapper
```

Implementation notes:

- Prefer styling iced’s `checkbox` / `toggler` if present in 0.14 with
  Catalog traits; do not fork widgets.
- Toggle on = accent (sparse); off = raised/hover grey — match macOS
  switch proportions as closely as iced allows (height ~16–20, pill).
- Checkbox selected = accent fill + check; unselected = hairline on
  raised/base.
- `form_row` padding: `SPACE_MD` vertical rhythm; label `body`, not muted
  unless inactive.

### Error field

```rust
pub fn field(..., help: Option<&str>, error: Option<&str>)
// if error.is_some(), show danger caption; help can still show or yield
```

Breaking change to `field` signature: acceptable inside monorepo; update
all call sites (`rg 'components::field|field\('`).

### Acceptance

- [x] Storybook Form/Field shows stacked field, form_row, checkbox, toggle,
      error state
- [x] No consumer migration required beyond compile fixes for `field`
- [x] `cargo make build` OK
- [x] Visual stop

### Commit

`feat(kit): form_row, field errors, checkbox/toggle styles (P7d)` — merged
`a911207` (2026-07-22).

---

## Pass E — Surfaces + storybook completeness

**Signature move:** Badge/card/sidebar headers match quiet macOS chrome;
storybook covers every public control style.

**Files:**
- Modify: `crates/sola-kit/src/components/badge.rs`
- Modify: `crates/sola-kit/src/components/card.rs` (optional borderless)
- Modify: `crates/sola-kit/src/components/sidebar.rs` (`section_header`)
- Modify: storybook pages badge, card, sidebar, button

### Badge

- **Neutral:** translucent / secondary surface + muted text (not solid
  border-colored slab). Example: `background.strong` fill @ full opacity
  or `secondary.weak`, text `secondary.base.text`.
- **Accent / Success / Warning / Danger:** keep stronger fills (status
  must scan) but slightly reduce “pill shout” if Neutral change makes
  hierarchy obvious.
- Size stays ~10–11 medium, pad `[2, 8]`.

### Card

- Default `card::style` keeps hairline for now **or** add
  `card::style_plain` (raised bg, no border) and show both in storybook.
- Prefer not changing default if many consumers rely on hairline outline;
  add plain variant instead (non-breaking).

### Sidebar section headers

- Title case (not `to_uppercase()`), size 11–12, `fonts::chrome()` or
  `ui_medium()`, muted color — closer to macOS sidebar group labels.
- Pads: use `SPACE_*` / existing row pad constants; replace bare `[6, 10]`
  where easy without layout thrash.

### Storybook checklist

- [ ] Button: primary, secondary, ghost, danger, danger_outline, confirm,
      list_item, menu_item, menubar, labeled helpers
- [ ] Field/Form: all states
- [ ] Badge: all tones after Neutral change
- [ ] Card: default + plain + modal + backplate
- [ ] Sidebar: section headers visible in showcase

### Acceptance

- [x] Visual: Badge, Card, Sidebar storybook
- [x] Build green

### Commit

`feat(kit): quieter badges/headers and complete storybook control matrix` —
merged `f064642` (2026-07-23).

---

## Pass F — Docs, debt notes, P8 handoff

**Signature move:** Process closure — not a visual redesign.

**Files:**
- Modify: `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` (P7
  checklist complete)
- Modify: `.grok/rules/active-work.md` → Current = P8 or next
- Modify: `docs/manual/design-language.md` only if type sizes or control
  rules changed in B–E (sync tables)
- Optional short note in `docs/specs/` or kit module docs:
  - **text_input fork:** keep for now; do not expand; future: evaluate
    iced stock + style only
  - **AGENTS.md** font loading: fix if still describing bundled
    `load_all` / `/opt/sola/share/fonts` as primary path

### text_input strategy (document only)

Record in plan completion notes / kit `text_input` module docs:

1. Fork exists for kit Theme defaults and historical reasons.
2. P7 only touches `style` + padding.
3. Future work: try stock `iced::widget::text_input` + `Catalog` style;
   delete fork if behavior parity holds.

### Acceptance

- [ ] Roadmap P7 marked done with pass commits listed
- [ ] active-work points to P8
- [ ] AGENTS.md / design-language not contradictory

### Commit

`docs: close P7 kit controls; hand off to P8`

---

## Testing matrix (every code pass)

| Check | Command / action |
|-------|------------------|
| Unit tests in touched modules | `cargo make build` (compiles tests) or `cargo test -p sola-kit …` if make supports it |
| Build | `cargo make build` or `cargo make build kit` |
| Install | **Only when user says so** |
| Visual | Storybook pages for that pass; shell only for Pass A |
| Capture | Prefer `docs/visual/passes/p7<letter>-…/` when screenshot tooling works |

---

## Suggested branch / worktree names

```
.worktrees/p7a-theme-binding     branch p7a-theme-binding
.worktrees/p7b-control-density   branch p7b-control-density
.worktrees/p7c-quiet-interactions
.worktrees/p7d-form-primitives
.worktrees/p7e-surfaces-storybook
.worktrees/p7f-docs-handoff
```

Merge each to master only with user approval; clean up worktree + branch
after merge (project rule).

---

## Dependencies graph

```
A (binding fix)
 └── B (density)
      ├── C (quiet interactions)
      │    └── E (surfaces + storybook)
      └── D (form primitives) ──┐
                               E can parallel D after B if needed
 F after C+D+E (or after E if D merged)
```

If time-boxed: **A + B + C** are the minimum that make kit “feel macOS”;
**D** is the unlock for P8 settings; **E/F** polish and process.

---

## Success criteria for “P7 done”

1. Shell overlay/menubar themes use sola Extended mapping (tests prove it).
2. Kit body type is 13; control pads are named and used by helpers.
3. Ghost hover and text selection no longer shout cyan.
4. Storybook can demonstrate the full button matrix + form row +
   checkbox/toggle.
5. Neutral badges and sidebar headers no longer read as generic web UI.
6. Roadmap + active-work hand off cleanly to **P8 — Kit apps inherit**.

---

## Open decisions (resolve at visual stops, not by guessing)

| Decision | Default in plan | User may override |
|----------|-----------------|-------------------|
| Primary button stays cyan fill | Yes | Quieter neutral primary |
| Card default keeps hairline | Yes; add plain variant | Flip default to plain |
| Field signature gains `error` | Yes (breaking monorepo) | Separate `field_error` fn |
| Toggle widget vs style-only | Style iced toggler if available | Composed fake switch |
| Pass A shell visual change | May be subtle | If ugly, tune menubar mid-tiers |

---

## Execution handoff

When ready to implement:

1. Set `.grok/rules/active-work.md` **Current** to Pass A (or next open
   pass) with link to this file.
2. Create worktree → implement → build → user install/smoke → visual stop
   → approve → merge + cleanup → advance Current.

**Do not** start P8 until Pass D exists (form_row at minimum) or the user
explicitly scopes P8 to token-only inheritance.
