# sola-kit Audit & Cleanup — Design

Date: 2026-05-29
Status: Proposal (for review — no code changed in this pass)
Scope: `sola-kit` (core, theme, fonts, components, storybook), with supporting
changes in `sola-core::theme` and the three Iced consumers
(`sola-monitor`, `sola-settings`, `sola-shell`). Legacy gap analysis against
`sola-kit-legacy`.

---

## 1. Why this document

`sola-kit` is the young, Iced-native successor to the CEF/Remix kit. It has
grown quickly over the last dozen commits (editable atoms, font roles, sidebar,
storybook). This is a consolidation pass: find the bugs, inconsistencies, and
drift that have accumulated, identify what's worth pulling forward from
`sola-kit-legacy`, and propose a concrete, prioritized set of changes before the
kit picks up more consumers.

The audit was conducted as a parallel agentic sweep across five surfaces (core
modules, components, storybook, legacy inventory, consumer usage), with the
load-bearing factual claims independently re-verified against source. Line
references are 1-based.

### The headline

The single most important finding is **schema drift between
`sola-core::theme::Palette::seed()` and the token vocabulary `sola-kit` actually
reads.** They were authored independently and no longer agree. Concretely:

| Kit reads (`from_bus_theme`) | Seed provides | Match? |
|---|---|---|
| `bg-primary` | `bg-primary` `#0d1117` | ✅ |
| `bg-secondary` | `bg-secondary` `#161b22` | ✅ |
| `bg-tertiary` (→ kit `bg_hover`) | `bg-tertiary` `#1c2129` | ⚠️ wrong *role*: seed also has a real `bg-hover` `#1a2030` the kit never reads |
| `border` | `border` `#2d333b` | value ≠ kit `hex::BORDER` `#30363d` |
| `text-primary` | `text-primary` `#e6edf3` | value ≠ kit `hex::FG` `#c9d1d9` |
| `text-tertiary` | `text-tertiary` `#6e7681` | ✅ |
| `accent` | `accent` `#00d4ff` | value ≠ kit `hex::ACCENT` `#58a6ff` |
| `success` | `success` `#3fb950` | ✅ |
| `warning` | **(absent)** | ❌ never round-trips |
| `danger` | `danger` `#f85149` | ✅ |
| `font-ui` / `font-ui-medium` / `font-display` / `font-chrome` | **(absent)** | ❌ seed has `font-sans` only |
| `font-mono` | `font-mono` `Iosevka Term Slab` | ✅ |

The consequence: when `sola-shell` boots, it broadcasts
`theme::to_bus_theme()` (= the seed) as the sticky `Topic::Theme`. Every other
kit app then renders an **accent, primary text, hover elevation, and border that
differ from the kit's own compile-time defaults**, and picks up only one of five
font roles. The desktop's actual on-screen palette is the seed's, not the kit's
documented `hex::*` palette — and nobody intended that.

Fixing this drift, and making it un-driftable, is the spine of the proposal.

---

## 2. Findings

Grouped by surface. Severity: **BUG** (correctness), **DRIFT** (two sources of
truth disagree), **CLEANUP**, **GAP** (missing capability), **ARCH**.

### 2.A Theme ⇄ palette schema (highest priority)

- **A1 — DRIFT/BUG: seed palette ≠ kit `hex::*`.** `to_bus_theme()`
  (`theme.rs:241`) returns `BusTheme::default()` (the seed), whose
  `accent`/`text-primary`/`bg-tertiary`/`border` values differ from the kit's
  `hex` constants (`theme.rs:54-69`). The docstring's claim that it "matches
  exactly what `default_theme()` reads back" is false. Shell broadcasts this at
  boot (`sola-shell/src/app.rs:142-147`).
- **A2 — BUG: `warning` cannot round-trip.** Seed (`palette.rs:9-134`) has no
  `warning` token. `from_bus_theme`/`atoms_from_bus_theme` read `warning`
  (`theme.rs:200`, `:446`) → always fall back to `hex::WARNING`.
  `bus_theme_from_atoms` writes 9 of 10 atoms, deliberately omitting `warning`
  (`theme.rs:330-345`). The `Atoms.warning` field, the grid label, and the
  reader exist; the persistence path doesn't.
- **A3 — BUG: hover atom mismatch.** Kit maps its `bg_hover` slot to bus token
  `bg-tertiary`, but the seed defines a distinct `bg-hover` (`#1a2030`) that the
  kit never reads. The kit's hover elevation pulls the wrong seed atom; the real
  hover atom is dead.
- **A4 — DRIFT: font-token vocabulary.** Kit reads `font-ui`/`font-ui-medium`/
  `font-display`/`font-chrome`/`font-mono`; seed defines `font-sans`/`font-mono`.
  Only `font-mono` overlaps, so a seeded theme can't drive four of five roles.
- **A5 — CLEANUP: atom↔token table written three times, already inconsistent.**
  The (field ↔ token-name ↔ fallback) mapping is hand-coded in `from_bus_theme`
  (`:194-203`), `bus_theme_from_atoms` (`:332-342`), and `atoms_from_bus_theme`
  (`:438-447`) — and they already disagree on `warning`. This is *why* A2
  happened and why it'll recur.
- **A6 — CLEANUP: duplicated Theme construction.** `from_bus_theme`
  (`:178-228`), `iced_theme_from_atoms` (`:294-323`), and `sola_extended`
  (`:93-127`) each build the same `Extended { background … }` block from a
  different color source — three copies of the layout.
- **A7 — ARCH/BUG: hidden font side-effect.** `from_bus_theme` calls
  `fonts::install(...)` (`:181`) inside what reads as a pure converter. All three
  consumers rely on this implicitly; any non-update caller (a preview) would
  mutate global font state. Confirmed by both the core and consumer audits.
- **A8 — CLEANUP: conversion-function sprawl.** Eleven public conversion fns
  (`default_theme`, `from_bus_theme`, `to_bus_theme`, `iced_theme_from_atoms`,
  `bus_theme_from_atoms`, `bus_theme_with_fonts`, `bus_theme_from`,
  `fonts_from_bus_theme`, `atoms_from_bus_theme`, `font_selection_from_bus_theme`,
  `sola_extended`) with no single canonical direction. `Atoms` is *documented* as
  the hub but `from_bus_theme`/`to_bus_theme`/`sola_extended` all bypass it.

### 2.B Bus / app scaffolding

- **B1 — BUG: polling thread leak + cross-subscription race.** `bus_stream`
  (`app.rs:259-272`) spawns a detached OS thread per subscription that only exits
  when its channel send fails. Two live `bus_subscription`s would race the single
  process receiver; a dropped one keeps draining until its next failed send. 8ms
  busy-poll.
- **B2 — BUG: poison panic kills the bus stream silently.**
  `bus().lock().expect("bus poisoned")` (`app.rs:261`) — if any thread panics
  holding the bus mutex, the poller panics, the stream closes, and the app stops
  receiving bus events with nothing logged (violates the "never lose output"
  rule). `fonts::current`/`install` handle poisoning gracefully; this doesn't.
- **B3 — ARCH: poll instead of using the notify fd.** The bus client exposes a
  clonable notify fd (`try_clone_notify`); iced's async runtime could register it
  as a readable source and drop both the 8ms poll and the thread.
- **B4 — CLEANUP/INCONSISTENCY: dead `App` trait + placeholder `run`.**
  `trait App { const APP_ID }` (`app.rs:33`) is implemented by **no** consumer;
  all three use a bare module const and shadow the name with their own struct.
  `run::<A>()` (`app.rs:227`) just forwards to `startup`. The trait's only
  consumer is the unused placeholder — exactly the speculative abstraction
  CLAUDE.md forbids.
- **B5 — BUG: connect outcome swallowed.** `BusSetup::install`
  (`app.rs:134-135`) discards `connect_blocking`'s result and proceeds; the
  app-menu emit is `let _ =`. A failed bus connect is undiagnosable.

### 2.C Consumer duplication (monitor / settings / shell)

- **C1 — HOIST: three near-identical `main()` builders.**
  `startup → BusSetup → iced builder + font loop` is copy-pasted across
  `sola-monitor/src/main.rs:50-71`, `sola-settings/src/main.rs:33-51`,
  `sola-shell/src/main.rs:20-41`. Monitor and settings are byte-identical apart
  from label strings. The precondition `run()`'s own doc named ("a second app to
  compare against") is now met three times.
- **C2 — HOIST: `Topic::Theme → self.theme` handled three ways.** Inline in
  monitor (`main.rs:233-237`) and settings (`main.rs:114-117`), via a method in
  shell (`app/bus.rs:51-54`). Same logic, three shapes.
- **C3 — BUG/INCONSISTENCY: quit handling reinvented; monitor ignores
  `CloseApp`.** Settings exits on both `MenuAction("quit")` and
  `CloseApp(self)` (`main.rs:119-131`); monitor handles only `MenuAction("quit")`
  (`main.rs:240-249`) — so monitor won't close when sent `CloseApp`, a latent
  bug. `"quit"`/`"exit"` are magic strings in four files.
- **C4 — INCONSISTENCY: default font source.** monitor/settings seed
  `default_font(fonts::ui())`; shell pins `default_font(INTER)`
  (`sola-shell/src/main.rs:37`), bypassing the themeable role table.
- **C5 — positive: bus draining is already uniform** — all three use
  `bus_subscription()` (no manual `try_recv`). Keep.

### 2.D Components library

Overall the widget set is small and **largely theme-clean**: nearly every color
comes from `theme.extended_palette()` and every font from a `fonts::*` role.
The weaknesses are consistency and a couple of real bugs.

- **D1 — BUG: hardcoded popover shadow.** `popover.rs:38`
  `Color::from_rgba(0,0,0,0.35)` — the lone non-palette color, not marked as an
  escape hatch (the convention `mod.rs:11-13` calls out).
- **D2 — BUG: dead sidebar border.** `sidebar.rs:120-124` sets a border `color`
  with `width: 0.0` — never drawn. Either `Border::default()` or a real width.
- **D3 — BUG/perf: `icon` re-reads SVG from disk every frame.** `icon.rs:25,40`
  call `read_svg(name)` per `view()`; the doc admits it and tells callers to
  cache, but ships no cached variant, so the happy path does per-frame disk I/O.
- **D4 — BUG (doc drift): badge/toolbar docs say "condensed-bold" but code uses
  `fonts::ui_medium()`** (`badge.rs:23/29`, `toolbar.rs:18/30`). No
  `fonts::condensed()` accessor exists, though a `CONDENSED_BOLD` constant does.
- **D5 — INCONSISTENCY: return types.** `card`/`popover` return `Container`
  (chainable); `badge`/`sidebar`/`swatch`/`field` return `Element` (not). `field`
  even builds a `Container` then `.into()`s it away, forcing `width(Fill)`.
- **D6 — INCONSISTENCY: `Element` theme param.** Some signatures write
  `Element<'a, Message>`, others `Element<'a, Message, Theme>`. Pick one.
- **D7 — INCONSISTENCY: `mod.rs` re-exports are arbitrary.** `card`, `badge`,
  etc. are re-exported as free fns, but `text::*`, `button::*`, and
  `text_input::style` (the most-used) are not — callers write `components::card`
  but `components::text::body`.
- **D8 — DUPLICATION: button-style skeleton.** The four-state `button::Style`
  match is rebuilt in `button.rs` (primary/secondary/ghost/danger),
  `sidebar::item_style`, and `toolbar::style`; `primary`/`danger` differ only by
  palette tier.
- **D9 — DUPLICATION: hairline border + radius.** `Border { color:
  background.stronger, width: 1.0, radius }` recurs in card/popover/swatch/
  text_input.
- **D10 — CLEANUP: scattered magic numbers.** Radii (4/6/8/999) and padding/
  spacing literals are per-file, despite `mod.rs` claiming a deliberate spacing
  convention. No shared scale.
- **D11 — CLEANUP: minor.** `flex_spacer` is orphaned/unexported
  (`sidebar.rs:163`); `swatch` uses `text("")` instead of `Space::new()`
  (`swatch.rs:24`); `icon` default tint (`background.base.text`) won't match
  muted caption text next to it.

### 2.E Storybook

- **E1 — COVERAGE: `icon` has no page.** Real gap — `icon`/`icon_colored` are
  public and exported, never dogfooded. (`swatch` and `text_input` are folded
  into the Theme and Field pages respectively — acceptable, but undocumented.)
- **E2 — EDITOR: atom grid is read-only; only accent is editable, via 5 fixed
  presets.** `theme.rs` page (`pages/theme.rs:81-120,153-186`) renders 9 atoms as
  display-only swatches; there's no color input even for accent. The intro copy
  ("live editor for the kit's atoms") oversells it.
- **E3 — propagation is otherwise correct.** Edits flow local theme rebuild →
  `fonts::install` → `Topic::Theme` (live) → `Topic::CustomTheme` (persist). The
  loop is well-built; A1/A2 are the leaks in it.
- **E4 — ARCH: adding a page touches 6–9 hand-threaded sites** (`mod`, enum,
  `ALL`, `label`, `section`, dispatch, +`Msg`/`State`/`update` for stateful), and
  forgetting `Page::ALL` silently drops the page with no error.
- **E5 — CLEANUP: page reimplements `color_hex` instead of using the kit's
  `theme::color_to_hex`** (`pages/theme.rs:188-200`); `resync_active_theme`
  matches presets by lossy `BusTheme` equality (`mod.rs:470-484`) — brittle once
  more atoms become editable (depends on A2/A5).

### 2.F Legacy gaps worth porting

From the `sola-kit-legacy` inventory, separating genuine gaps from web-only
concepts that correctly dissolve under Iced.

- **F1 — GAP: ColorPicker.** Legacy had a full HSLA + alpha + hex + clipboard
  editor (`web/lib/components/color-picker.tsx`). The Iced theme page offers 5
  accent presets and nothing else. `swatch` already reserves an `onChange` seam.
  This is the obvious unblock for E2.
- **F2 — GAP: system font enumeration.** Legacy enumerated installed families via
  `fc-list` (`src/app/app.rs::enumerate_fonts`). The kit ships a hardcoded
  `INSTALLED_FAMILIES` (`fonts.rs:180`), so a font picker can only offer shipped
  families. A `fontdb`/`fc-list`-backed enumeration is needed for "pick any
  installed font."
- **F3 — GAP: NumberInput** (unit-aware stepper) — prerequisite for ever editing
  space/radius/text-size tokens in the Iced kit.
- **F4 — GAP: Container** (max-width readable column) — small layout primitive
  the kit punted on; apps hand-roll it.
- **F5 — GAP: Button `confirm` two-stage mode** (arm → confirm within ~2s for
  destructive actions) — cheap; the storybook's own "Delete theme" is the case.
- **Explicitly NOT porting:** the per-component `ComponentBindings`/slot→token
  model + `BindingsEditor` + categories (the Iced port deliberately collapsed
  this into one Rust atoms→palette mapping); and all web-only infra (IPC bridge,
  swc transform, importmap, `__solaInitial`, `Root` wrapper, per-window
  `root_component`). One thing to *note*, not port: legacy gave apps default
  clipboard copy/paste menu handlers for free; Iced apps now wire their own.

---

## 3. Proposed changes

Organized as workstreams. Each is independently shippable; ordering reflects
dependency and risk, not a schedule.

### W1 — Unify the theme schema (fixes A1–A8, unblocks E2/E5/F1)

The fix for the keystone problem is **one source of truth for the atom set,
table-driven in both directions, with `sola-core`'s seed and `sola-kit`'s
vocabulary reconciled.**

1. **Reconcile the catalog (per §5 decisions).** Make
   `sola-core::Palette::seed()` and `sola-kit` agree on names *and* values, with
   the **seed as canonical** for colours: update `sola-kit::theme::hex::*` to the
   seed values (`accent #00d4ff`, `text #e6edf3`, `border #2d333b`,
   `bg-hover #1a2030`, `bg-tertiary #1c2129`). Read the hover elevation from the
   seed's real `bg-hover` token, not `bg-tertiary` (fixes A3). Add a `warning`
   token (`#d29922`) to the seed (fixes A2). Rename the seed's font tokens to the
   role vocabulary `font-ui`/`font-ui-medium`/`font-display`/`font-chrome`/
   `font-mono` with the kit's role defaults (fixes A4).
2. **Single mapping table.** Define the atom↔token↔fallback mapping once (a
   `const &[(AtomField, &str, &str)]` or a small struct array) that both
   `atoms_from_bus_theme` and `bus_theme_from_atoms` consult. Eliminates A5; makes
   A2-class drift structurally impossible. Add a unit test asserting
   writelist-keys ⊇ readlist-keys.
3. **`Atoms` becomes the real hub.** Collapse to three primitives:
   `BusTheme ⇄ Atoms` (lossless, table-driven) and `Atoms → iced::Theme` (one
   `build_theme`) and `Atoms → Fonts`. Rewrite `from_bus_theme`,
   `to_bus_theme`, `sola_extended`, `iced_theme_from_atoms` as thin compositions
   of those — kills A6, A8. `to_bus_theme()` becomes
   `bus_theme_from_atoms(&Atoms::default())`, fixing A1.
4. **Make the font side-effect explicit (A7) — split (per §5 Q3).** Rename
   `from_bus_theme` → pure `theme_from_bus(&BusTheme) -> Theme` (no font install).
   Callers and the W3 `apply_theme_update` helper call
   `fonts::install(fonts_from_bus_theme(bus))` explicitly alongside it.

### W2 — Bus subscription correctness (fixes B1–B3, B5)

1. Tie the poller's lifetime to the stream: gate the loop on a
   `Weak`/`AtomicBool` dropped with the subscription, so a dropped subscription
   stops draining (B1). Enforce single-subscription-per-process with a `OnceLock`
   guard mirroring `BUS`, or document + assert it.
2. Recover from lock poisoning instead of `expect` (B2) — `into_inner()` on
   `PoisonError`, log-and-continue.
3. Log the `connect_blocking` outcome and the app-menu emit result (B5).
4. **Stretch (B3):** replace the 8ms poll with the bus notify fd registered as an
   async source. Higher risk; do after W1/W2.1 land. Track separately.

### W3 — Consumer hoisting: helpers only (fixes C1–C4, B4; per §5 Q4)

Decision: ship the pure helpers, **delete** the dead `App` trait + placeholder
`run()`, and defer the generic single-window wrapper. Each helper is independent
and low-risk.

1. **Delete `App` + `run()` (B4).** No consumer implements `App`; `run()` only
   forwards to `startup`. Remove both from `app.rs` and the `lib.rs` re-exports.
   Consumers keep their bare `const APP_ID` (already the reality).
2. **`apply_theme_update(&Message, &mut Theme) -> bool`** (C2) — consumes a
   `Topic::Theme` delivery: rebuilds `*dst` via `theme_from_bus` *and* calls
   `fonts::install(...)` (the explicit pairing from W1.4). monitor/settings each
   drop to one call; shell calls it inside `on_theme`.
3. **`app::is_self_quit(&Message, app_id) -> bool` + shared `QUIT_ACTION_ID`**
   (C3) — handles `MenuAction{quit}` and `CloseApp(self)`, fixing monitor's
   ignored `CloseApp` and removing the magic string. Shell opts out (maps
   quit→Shutdown deliberately).
4. **`fonts::register(builder)` combinator** folding `load_all` into the iced
   builder (two thin shims: `application` + `daemon`). Resolve the `default_font`
   source (C4): seed from `fonts::current().ui` everywhere, or document shell's
   `INTER` pin.

Deferred (not now): the `SolaApplication` trait + generic `run::<A>()` that would
own `startup` + `BusSetup` + builder + fonts for single-window apps. Revisit when
the associated-type cost is clearly outweighed — the helpers above already remove
most of the per-app duplication.

### W4 — Components consistency (fixes D1–D11)

Small, mechanical, low-risk. Batch them:

1. Conventions doc + apply: return-type rule (container-shaped → `Container`,
   leaf → `Element`), single `Element<…, Theme>` spelling, `mod.rs` re-export
   policy (re-export the common `text`/`button` fns too, or none) — D5/D6/D7.
2. Shared style helpers: `filled(base, strong, weak, status) -> button::Style`
   and `hairline(palette, radius) -> Border`; a named radius/space scale
   (`RADIUS_SM/MD/LG/PILL`) — D8/D9/D10.
3. Bug fixes: mark-as-escape-hatch or theme-derive the popover shadow (D1), fix the dead sidebar border
   (D2), add a cached `icon` handle (D3), correct the badge/toolbar docs or add a
   `fonts::condensed()` accessor (D4), `Space::new()` in swatch, drop/export
   `flex_spacer` (D11).

### W5 — Storybook + editor (fixes E1, E2, E4, E5; consumes F1)

1. Add a `pages/icon.rs`; document `swatch`/`text_input` as intentionally folded
   (E1).
2. Add a `Page::ALL` exhaustiveness test now (E4); defer a `Page` trait/registry
   until the page count justifies it (kit's no-speculative-abstraction rule).
3. Replace `color_hex` with `theme::color_to_hex`; key `resync_active_theme` on
   preset name not value-equality (E5).
4. **Color picker (F1):** build an Iced color-editor component (`components/`),
   wire it to the atom grid via `swatch`'s reserved `onChange`, making all atoms
   editable (E2). Depends on W1 (lossless round-trip) to persist correctly.

### W6 — Legacy gaps (F2–F5), as-needed

Pull these when a real consumer needs them, not speculatively:
- **F5 (button confirm)** — smallest; the storybook delete button is a consumer
  today. Do alongside W4.
- **F2 (system font enumeration via `fontdb`)** — do when a settings/theme font
  picker over the full system set is actually wanted; until then
  `INSTALLED_FAMILIES` is honest about what's shipped.
- **F3 (NumberInput)** / **F4 (Container)** — defer until token-size editing or a
  reading-column layout is needed.

---

## 4. Prioritization

| Workstream | Fixes | Risk | Depends on | Notes |
|---|---|---|---|---|
| **W1** Theme schema unification | A1–A8 | Med (touches sola-core + 3 apps' look) | — | Keystone. Needs decision Q1. |
| **W2** Bus correctness | B1,B2,B5 | Low | — | B3 (notify fd) is a separate stretch. |
| **W3** Consumer hoisting (helpers only) | C1–C4,B4 | Low | W1.4 (for #2) | Generic `run()` deferred; pure helpers. |
| **W4** Component consistency | D1–D11 | Low | — | Mechanical; batch. |
| **W5** Storybook + color picker | E1,E2,E4,E5,F1 | Low–Med | W1 | Color picker is the visible win. |
| **W6** Legacy gaps | F2–F5 | Low | as-needed | Pull on demand; F5 with W4. |

Suggested order: **W2 + W4** first (low-risk, independent, immediate quality),
then **W1** (the keystone, after Q1 is decided), then **W3** and **W5** (both
lean on W1's clean theme surface), then **W6** on demand.

---

## 5. Decisions (resolved 2026-05-30)

- **Q1 — Canonical palette: the SEED (cyan).** The seed's values win
  (`accent #00d4ff`, `text-primary #e6edf3`, `border #2d333b`,
  `bg-tertiary #1c2129`, `bg-hover #1a2030`); this is what the desktop already
  renders. W1 updates `sola-kit::theme::hex::*` to match the seed, not the other
  way around. `default_theme()` will then equal the seeded `Topic::Theme`, so the
  brief pre-replay frame and the steady state agree.
- **Q2 — Add `warning`.** Add a `warning` token to `Palette::seed()` (value
  `#d29922`, the kit's existing `hex::WARNING`) and include it in the
  `bus_theme_from_atoms` writelist so it round-trips and becomes editable.
- **Q3 — Split the font side-effect.** `from_bus_theme` becomes a pure
  colour converter (renamed `theme_from_bus`); callers (and the W3 theme-update
  helper) call `fonts::install(fonts_from_bus_theme(bus))` explicitly alongside.
- **Q4 — Helpers only.** Ship `apply_theme_update`, `is_self_quit` +
  `QUIT_ACTION_ID`, and `fonts::register`. **Delete** the dead `App` trait and
  the placeholder `run()` (no consumer implements them). Defer the generic
  `SolaApplication` + single-window `run()` until a clear win justifies the
  associated-type-heavy trait.

### Sub-decision carried into W1 (font-token vocabulary, A4)

Q1 settled colours but not the font-token drift. Resolution: the kit's font-role
*design* (SF Pro family default + JetBrains Mono mono, recently added) is the
intended look, so the seed's font tokens are **renamed to the role vocabulary**
(`font-ui`, `font-ui-medium`, `font-display`, `font-chrome`, `font-mono`) and
their default values set to the kit's role defaults — replacing the seed's older
`font-sans = DejaVu Sans` / `font-mono = Iosevka Term Slab` placeholders. Flag if
the DejaVu/Iosevka defaults were actually wanted; otherwise this is the plan.

---

## 6. Appendix — what's healthy (don't touch)

- The `startup` / `BusSetup` / `window_settings` / `load_all` decomposition is
  clean; `main.rs` composes them thinly. Only `App`/`run` is the weak seam.
- Bus draining is already uniform across consumers (`bus_subscription`).
- Components are theme-clean modulo D1; the font-role indirection
  (`fonts::ui()` etc.) is a good abstraction that the components honor.
- The storybook's edit→install→broadcast→persist loop is correctly wired; the
  defects are in the theme schema it rides on, not the loop.
- The deliberate omissions from legacy (per-component bindings model, all web
  infra) are correct calls and should stay omitted.
