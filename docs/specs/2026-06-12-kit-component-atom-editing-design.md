# Kit Component Atom Editing — Design

**Goal:** Let the storybook edit theme atoms *in context* on each component's
page (not just the global Theme grid), give the sidebar's selected row a real,
independently-tunable highlight, and replace per-edit auto-persistence with an
explicit Save/Revert workflow.

**Status:** Implemented 2026-06-12 — §§1–4 landed (live edits + manual
Save/Revert, per-atom reset, `selection` atom + stronger sidebar highlight,
per-component panels). §5 (page-header/description redesign) deferred to a later
UI pass per the user. Uncommitted on `master`.

**Date:** 2026-06-12

**Crate:** `sola-kit` (Iced app kit + `sola-kit` storybook binary).

---

## Background — how it works today

- **Atoms are shared and derived.** The kit's 10 colour atoms (`Atoms` in
  `theme.rs`) are mapped into iced's `Extended` palette by `extended_from_atoms`
  (`theme.rs:115`) — the single binding point. Components never read atoms
  directly; they read palette slots via `theme.extended_palette()`. So one atom
  feeds many components.
- **Editing is immediately live *and* persisted.** `apply_atom`
  (`storybook/mod.rs:642`) writes one atom, then `refresh_active_theme` +
  `broadcast_theme` (emits the live `Topic::Theme`, recolouring the whole
  desktop) + `persist_active_theme` (emits `Topic::CustomTheme`, the durable
  copy) — on every drag of the picker. Gated to non-default themes
  (`is_default_active` → read-only Default).
- **Atoms are only editable on the Theme page.** `pages::theme::atom_grid`
  (`pages/theme.rs:123`) renders all 10 as a swatch grid; each swatch opens an
  anchored colour picker. The `AtomField` enum (`storybook/mod.rs:184`) +
  `atom_grid`'s `rows` table are the UI's view onto `Atoms`; `ATOM_BINDINGS`
  (`theme.rs:299`) is the parallel token table that round-trips `Atoms` ↔ the bus
  theme.
- **The sidebar highlight has no dedicated atom.** `sidebar::item_style`
  (`components/sidebar.rs:639`) fills the active row with `background.strong`
  (= the `bg_hover` atom, shared with every hover state) and colours its text
  `primary.base` (= `accent`). There is no way to make the selection stronger
  without dragging hover states or the global accent along with it.
- **Side-channel precedent.** Things iced's palette can't hold already ride a
  process-wide slot reinstalled on each theme build: fonts (`fonts::install` /
  `fonts::ui()` …) and shell-`*` tokens (`ShellStyle`). The new `selection` atom
  uses the same rail.

---

## What changes (overview)

1. **Edit model:** edits stay live (broadcast) but no longer auto-persist.
   **Save** and **Revert** become explicit, manual, and global, living in the
   app top bar. A dirty indicator shows when there's unsaved work.
2. **Per-atom reset to default:** each swatch can reset *that one atom* to its
   compile-time default — surgical, not whole-set.
3. **New `selection` atom:** a dedicated, independently-tunable colour for the
   sidebar's selected row (and future selection states), surfaced via the
   fonts/shell side-channel.
4. **Per-component atom panels:** every component page shows a compact panel of
   just the atoms it visibly uses, editable in place.
5. **Page-header / description redesign:** replace the run-on muted paragraph
   atop each page with a readable, intentional header block (visual treatment
   mocked in Paper before implementation).

---

## 1. Edit model — live edits, manual Save / Revert

The working set is *all atom changes across all tabs* since the last checkpoint.
There is no per-tab scoping of commit/discard.

- **Live edit:** `apply_atom` keeps `refresh_active_theme` + `broadcast_theme`
  but **drops `persist_active_theme`**. Editing still recolours the live desktop;
  it just isn't durable yet.
- **Checkpoint:** the active theme carries a `checkpoint` snapshot of its
  *entire* editable state — atoms, fonts, and `shell-*` tokens. Dirty ⇔
  `active != checkpoint`.
- **Unified across edit kinds.** Atoms are the focus of this work, but for the
  "one working set, one Save/Revert" model to be coherent the font picker and
  shell-token edits must join it: their immediate `persist_active_theme` calls
  drop too, so all three (atoms, fonts, shell) become live-but-unsaved and are
  committed/discarded together by the top-bar Save/Revert.
- **Save** (top bar): `checkpoint = active.clone()` and `persist_active_theme`
  (emit `Topic::CustomTheme`). Clears dirty.
- **Revert** (top bar): restore `active = checkpoint`, then `refresh` +
  `broadcast` so the desktop snaps back. Clears dirty.
- **No automatic revert.** Switching component tabs only changes which atoms are
  shown; in-progress edits ride along untouched until Save or Revert.
- **Dirty indicator** (top bar): e.g. `Edited •` next to the Save/Revert
  buttons; hidden when clean.
- **Theme switch while dirty:** picking a different theme in the top bar
  abandons the unsaved working set and loads the selected theme fresh. The dirty
  dot is the only heads-up (decided: simplest behaviour consistent with
  manual-only commit).
- **Default theme stays read-only.** Editing on the Default theme remains a
  no-op (`apply_atom`'s existing guard); Save/Revert are inert there.

The Save/Revert controls slot into the existing `Storybook::header`
(`storybook/mod.rs:918`), alongside the theme picker / New / Delete row.

## 2. Per-atom reset to default

Each swatch gains a **reset affordance** that resets only that atom to its
compile-time `hex::*` default:

- A small ↺ control on the swatch tile corner, **shown only when that atom
  differs from its default**, plus a "Reset to default" line inside the swatch's
  picker popover.
- Reset is an ordinary live edit (broadcast, contributes to dirty); the global
  Save/Revert sit on top of it.
- Lives on the shared `swatch_tile` (`pages/theme.rs`), so it appears
  identically on the global Theme grid and every per-component panel.
- Default lookup: compare/assign against `Atoms::default()` per field (the
  `hex::*` constants).

## 3. The `selection` atom — stronger sidebar highlight

A dedicated atom so the selected-row highlight is independently tunable.

- **`Atoms` gains `selection: Color`**; new `hex::SELECTION` default — an
  accent-tinted fill clearly stronger than `bg_hover` (exact value chosen during
  implementation; tunable).
- **`extended_from_atoms` does *not* route it** (iced's `Extended` has no
  selection slot). Instead it rides the side-channel: the atoms→`iced::Theme`
  build path (`build_theme`, the single builder all theme construction flows
  through) installs the current selection colour into a process-wide
  `RwLock<Color>`, mirroring `fonts::install`. A `theme::selection() -> Color`
  accessor reads it.
- **`sidebar::item_style`** active branch fills with `theme::selection()` instead
  of `background.strong`. (Text colour stays `primary.base`/accent, or is tuned
  alongside.)
- **Round-trips on the bus** as a new palette token `selection` via a new
  `ATOM_BINDINGS` entry (token `"selection"`, fallback `hex::SELECTION`,
  get/set on the new field) — so it persists with the rest of the theme even
  though it has no iced slot.
- **Editable in the UI:** new `AtomField::Selection` variant (+ its get/set) and
  a new row in `atom_grid`'s `rows` table.

Touch points for the new atom (all parallel tables that list atoms):
`Atoms` struct + `Default`, `hex::SELECTION`, `ATOM_BINDINGS`, `AtomField` +
its get/set, `atom_grid` rows, `build_theme` install, `theme::selection()`
accessor, `sidebar::item_style`.

## 4. Per-component atom panels

- **`Page::atoms(self) -> &'static [AtomField]`** — a curated list of the atoms
  each component visibly uses. Authored below; tunable.
- Each component page renders a compact **"Atoms" panel** (below the demo)
  containing the swatches for `page.atoms()`, reusing `swatch_tile` + the
  existing anchored picker and the per-atom reset from §2. No per-panel
  Save/Revert — those are global in the top bar (§1).
- The **Theme page keeps the full grid** (now 11 atoms incl. `Selection`). The
  **Shell page keeps its own `shell-*` editor** (unchanged; no atom panel).
- Same underlying `EditAtom` → `apply_atom` path, so live broadcast + dirty
  tracking + Save/Revert all work uniformly.

### Curated atom lists (best-effort; tune freely)

Derived from each component's actual palette-slot usage (atom → slot map:
`bg`→background.base, `bg_raised`→background.weak, `bg_hover`→background.strong,
`border`→background.stronger + secondary.base.color, `fg`→text,
`fg_muted`→secondary.base.text, `accent`→primary, `success/warning/danger`→
their slots, `selection`→sidebar active fill).

| Page         | Atoms shown |
| ------------ | ----------- |
| Divider      | Border, Bg |
| Split        | Bg, BgRaised, Border |
| Toolbar      | Bg, BgRaised, BgHover, Border, Fg, FgMuted |
| Text         | Fg, FgMuted, Accent, Success, Warning, Danger |
| Button       | Accent, Danger, Bg, BgHover, Border, Fg |
| Badge        | Accent, Success, Warning, Danger, Border, FgMuted |
| Card         | Bg, BgRaised, Border, Fg, Accent |
| Field        | BgRaised, Border, Fg, FgMuted |
| Icon         | Fg, FgMuted, Accent |
| NumberInput  | Bg, Border, Fg, FgMuted, Accent |
| Readable     | Bg, BgRaised, Fg, FgMuted |
| ColorPicker  | Bg, BgRaised, Border, Fg, Accent |
| Popover      | BgRaised, Border, Fg, FgMuted |
| Sidebar      | Bg, BgHover, **Selection**, Fg, FgMuted, Accent |

(Theme = full grid of all 11; Shell = `shell-*` editor, no atom panel.)

## 5. Page-header / description redesign

Today: `column![ heading(label), body("…run-on…").style(muted), demo, code(…) ]`
(e.g. `pages/sidebar.rs:213`). The description is a single muted paragraph with
no hierarchy.

**Replace with a structured header block** — title, a proper-size one-line lead
(not muted-tiny), optional short "when to use" / key points, and the signature as
a styled code chip. Implemented as a shared `page_header(...)` helper so every
page is consistent.

**This section's exact visual treatment is mocked in Paper first** (the user's
open paper.design doc): 2–3 header layouts, pick one, then hand-translate to
iced widgets. Paper is a visual reference only — it can't emit iced code. The
structural intent above is fixed; the typography/spacing is the open variable.

---

## Data flow

```
picker drag ─▶ Msg::EditAtom(field,color) ─▶ apply_atom
                                              ├─ field.set(active.atoms)
                                              ├─ refresh_active_theme  (rebuild iced::Theme; build_theme installs selection())
                                              └─ broadcast_theme       (Topic::Theme → whole desktop, live)
                                                 (no persist; dirty = active != checkpoint)

per-atom ↺  ─▶ Msg::ResetAtom(field)       ─▶ set field = Atoms::default().field ─▶ refresh + broadcast

Save  (top bar) ─▶ checkpoint = active.clone(); persist_active_theme (Topic::CustomTheme); dirty=false
Revert(top bar) ─▶ active = checkpoint.clone(); refresh + broadcast; dirty=false
SelectTheme while dirty ─▶ load chosen theme as active+checkpoint (abandon working set)
```

## Files touched (anticipated)

- `crates/sola-kit/src/theme.rs` — `Atoms.selection` + `Default`; `hex::SELECTION`;
  `ATOM_BINDINGS` entry; `build_theme` selection install + `selection()` accessor
  + process-wide slot.
- `crates/sola-kit/src/storybook/mod.rs` — `AtomField::Selection` (+ get/set);
  `Page::atoms()`; checkpoint field + dirty; `Msg` for Save/Revert/ResetAtom;
  `update` handling; split `apply_atom`; drop immediate persist from
  `apply_shell_color` and the font-apply path so they join the Save gate;
  Save/Revert/indicator in `header`.
- `crates/sola-kit/src/storybook/pages/theme.rs` — new `Selection` row in
  `atom_grid`; per-atom reset on `swatch_tile`; extract a reusable atom-panel.
- `crates/sola-kit/src/storybook/pages/*.rs` — each component page renders its
  `page.atoms()` panel; adopt the new `page_header` helper.
- `crates/sola-kit/src/components/sidebar.rs` — `item_style` reads `selection()`.
- A shared `page_header` helper (location decided in the plan).

## Testing

- `theme.rs`: `selection` round-trips `Atoms` ↔ bus theme (token present, value
  preserved); missing/malformed `selection` token falls back to `hex::SELECTION`;
  `Atoms::default().selection == hex::SELECTION`.
- Edit model: `apply_atom` broadcasts but does not persist; Save persists +
  clears dirty; Revert restores checkpoint; dirty reflects `active != checkpoint`.
- Per-atom reset: resets only the targeted field to default; reset visibility ⇔
  field differs from default.
- `selection()` accessor returns the installed colour after `build_theme`.
- `Page::atoms()` returns the expected set per page (table above).

## Tunable / open

- The curated atom lists (table in §4) — best-effort; expected to be adjusted.
- `hex::SELECTION` exact value and whether selected-row *text* colour is retuned.
- Section 5 typography/spacing — finalised via the Paper mock.
