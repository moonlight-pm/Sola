# Sidebar component + new theme protocol — design

**Date:** 2026-05-07
**Branch:** `sola-kit-preact`
**Status:** approved (brainstorm); ready to implement

## Overview

Replace the kit's prior single-layer theme implementation (a flat `Theme`
struct + a hardcoded `catalog.rs` listing which atomic CSS vars each
component used) with a two-layer **palette + bindings** protocol, and use
the addition of a new sectioned-nav `<sola-sidebar>` component as the
first concrete consumer.

Two layers of editing are needed because the prior design conflated them:

1. **Atom edit** — change the *value* of a named token (e.g. swap
   `--border-subtle` from `#21262d` to `#2a2f36`).
2. **Binding edit** — change *which* token a component slot consumes
   (e.g. the sidebar's right-edge border now reads from `--border`
   instead of `--border-subtle`).

Each visual slot on a component declares a *selection group*; tokens
self-declare which groups they're eligible for; the editor uses both to
constrain the dropdown of valid choices per slot.

## §1 · Sidebar component anatomy (v1 scope)

Three custom elements composed via JSX slots:

```tsx
<sola-sidebar>
  <sola-sidebar-section label="Components">
    <sola-sidebar-item active>
      <icon slot="leading" name="square"/>
      Button
      <span slot="trailing">3</span>
    </sola-sidebar-item>
    <sola-sidebar-item>Field</sola-sidebar-item>
  </sola-sidebar-section>

  <sola-sidebar-section label="Theme">
    <sola-sidebar-item>Colors</sola-sidebar-item>
  </sola-sidebar-section>
</sola-sidebar>
```

| Element | Required props | Optional props | Slots |
|---|---|---|---|
| `<sola-sidebar>` | — | `width?: string` (default `"220px"`) | default (sections) |
| `<sola-sidebar-section>` | — | `label?: string` | default (items) |
| `<sola-sidebar-item>` | — | `active?: boolean`, `disabled?: boolean` | `leading`, default (label), `trailing` |

**Selection model.** `<sola-sidebar-item>` dispatches a bubbling
`sola-select` `CustomEvent` on click / Enter / Space. Selection state is
**parent-controlled** — the consumer flips `active` on whichever item is
current. The component does not track current selection itself.

**Active visual.** Bg-tinted full-width row plus a 2-px accent stripe at
the leading edge.

**Hover visual.** Item background swaps to `--sola-sidebar-item-bg-hover`.
Text colour and icon colour are *not* swapped on hover — only the
background changes. (Keeps hover quiet and non-distracting; no extra
slots for hover text/icon in v1.)

**Disabled visual.** `disabled` items render at `opacity: 0.4` with
`pointer-events: none`. No themed slots in v1 — opacity is a structural
choice, not a colour decision.

**Width.** Per-instance prop (default `220px`), not themed. Cheap to
promote to a theme slot later if a global "narrow / wide" variant is
ever desired.

**Designed-with-room-for, deferred from v1:**

- `header` / `footer` slots on `<sola-sidebar>`
- Section collapse (carets, open/closed state)
- Resize handle
- Icon-rail collapse mode
- Nested items / tree depth
- Multi-select / checkbox items

All future additions are additive (new slot, new prop, new event) and
won't reshape the v1 API.

## §2 · Theme protocol

Both layers live in `sola-core::theme` and travel together as the
inner type of `sola_bus::Topic::Theme`.

### Types

```rust
pub struct Theme {
    pub palette:    Palette,
    pub components: BTreeMap<String, ComponentBindings>,
}

// ── Layer 1 — flat palette of named tokens ──────────────────────────
pub struct Palette {
    pub tokens: BTreeMap<TokenName, Token>,
}

pub type TokenName = String;          // "bg-secondary", "border-subtle", "space-md"

pub struct Token {
    pub kind:   TokenKind,
    pub value:  String,               // "#161b22" / "12px" / "'DM Sans', system-ui"
    pub groups: Vec<String>,          // ["surface"] or ["accent", "border"]
}

pub enum TokenKind { Color, FontFamily, TextSize, Space, Radius }

// ── Layer 2 — per-component bindings ────────────────────────────────
pub struct ComponentBindings {
    pub slots: BTreeMap<SlotName, Binding>,
}

pub type SlotName = String;           // "bg", "border", "item-stripe"

pub struct Binding {
    pub group: String,                // slot's selection-group constraint
    pub token: TokenName,             // current selection (key into palette.tokens)
}
```

### Resolution

```rust
let binding = &theme.components["sidebar"].slots["border"];
//   binding.group  == "border"
//   binding.token  == "border-subtle"
let token = &theme.palette.tokens[&binding.token];
//   token.kind     == TokenKind::Color
//   token.value    == "#21262d"
//   token.groups   ⊇ {"border"}     (invariant; editor enforces)
```

### Selection-group vocabulary (v1)

Each group implicitly matches one `TokenKind`:

| Kind | Groups |
|---|---|
| `Color` | `surface`, `border`, `text`, `accent`, `accent-tint`, `status` |
| `FontFamily` | `font-family` |
| `TextSize` | `text-size` |
| `Space` | `space` |
| `Radius` | `radius` |

A token can declare *multiple* groups (e.g. an accent color usable as a
border).

Adding a group later is additive — tokens opt in by tagging, slots opt
in by referencing.

### CSS the renderer sees

The Rust side renders `Theme` to a single `:root { … }` block in two
sections: atoms first, then per-component scoped vars.

```css
:root {
  /* Layer 1 — atoms. One var per palette token, name = key. */
  --bg-secondary:   #161b22;
  --border-subtle:  #21262d;
  --accent:         #00d4ff;
  --accent-dim:     rgba(0, 212, 255, 0.12);
  /* … */

  /* Layer 2 — bindings. One var per slot, name = `--sola-<component>-<slot>`. */
  --sola-page-bg:                 var(--bg-primary);
  --sola-page-text:               var(--text-primary);
  --sola-page-font:               var(--font-sans);
  --sola-page-text-size:          var(--text-body);

  --sola-sidebar-bg:              var(--bg-secondary);
  --sola-sidebar-border:          var(--border-subtle);
  --sola-sidebar-item-bg-active:  var(--accent-dim);
  --sola-sidebar-item-stripe:     var(--accent);
  /* … */
}
```

**Component CSS only ever references scoped vars** (`var(--sola-<component>-<slot>)`).
Atoms are an implementation detail of the `:root` block; a binding
swap is a one-line edit there with no component-CSS change.

## §3 · Sidebar slots, default bindings, seed palette

### `page` (globals)

Applied at `body { … }` so unstyled descendants inherit them.

| Slot | Group | Default token |
|---|---|---|
| `bg` | `surface` | `bg-primary` |
| `text` | `text` | `text-primary` |
| `font` | `font-family` | `font-sans` |
| `text-size` | `text-size` | `text-body` |

### `sidebar`

| Slot | Group | Default token |
|---|---|---|
| `bg` | `surface` | `bg-secondary` |
| `border` | `border` | `border-subtle` |
| `section-label-color` | `text` | `text-secondary` |
| `section-label-size` | `text-size` | `text-caption` |
| `item-text-idle` | `text` | `text-secondary` |
| `item-text-active` | `text` | `text-primary` |
| `item-text-size` | `text-size` | `text-body` |
| `item-icon-idle` | `text` | `text-secondary` |
| `item-icon-active` | `accent` | `accent` |
| `item-bg-hover` | `surface` | `bg-hover` |
| `item-bg-active` | `accent-tint` | `accent-dim` |
| `item-stripe` | `accent` | `accent` |
| `padding-block` | `space` | `space-md` |
| `padding-inline` | `space` | `space-sm` |
| `item-padding-block` | `space` | `space-sm` |
| `item-padding-inline` | `space` | `space-md` |
| `gap` | `space` | `space-xs` |

### Seed palette

Values carry over unchanged from the current `theme.rs`; the new
`groups: Vec<String>` field is the only addition.

**Colors (`TokenKind::Color`):**

| Name | Value | Groups |
|---|---|---|
| `bg-primary` | `#0d1117` | `["surface"]` |
| `bg-secondary` | `#161b22` | `["surface"]` |
| `bg-tertiary` | `#1c2129` | `["surface"]` |
| `bg-hover` | `#1a2030` | `["surface"]` |
| `border` | `#2d333b` | `["border"]` |
| `border-subtle` | `#21262d` | `["border"]` |
| `text-primary` | `#e6edf3` | `["text"]` |
| `text-secondary` | `#8b949e` | `["text"]` |
| `text-tertiary` | `#6e7681` | `["text"]` |
| `text-muted` | `#484f58` | `["text"]` |
| `text-accent` | `#58a6ff` | `["text", "accent"]` |
| `accent` | `#00d4ff` | `["accent"]` |
| `accent-dim` | `rgba(0, 212, 255, 0.12)` | `["accent-tint"]` |
| `danger` | `#f85149` | `["status"]` |
| `success` | `#3fb950` | `["status"]` |

**Typography:**

| Name | Kind | Value | Groups |
|---|---|---|---|
| `font-sans` | `FontFamily` | `'DM Sans', system-ui, sans-serif` | `["font-family"]` |
| `font-mono` | `FontFamily` | `'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace` | `["font-family"]` |
| `text-caption` | `TextSize` | `11px` | `["text-size"]` |
| `text-body` | `TextSize` | `12px` | `["text-size"]` |
| `text-body-lg` | `TextSize` | `13px` | `["text-size"]` |
| `text-heading` | `TextSize` | `16px` | `["text-size"]` |
| `text-display` | `TextSize` | `20px` | `["text-size"]` |

**Spacing (`TokenKind::Space`):**

| Name | Value | Groups |
|---|---|---|
| `space-xs` | `4px` | `["space"]` |
| `space-sm` | `8px` | `["space"]` |
| `space-md` | `12px` | `["space"]` |
| `space-lg` | `16px` | `["space"]` |
| `space-xl` | `20px` | `["space"]` |
| `space-xxl` | `24px` | `["space"]` |

**Radius (`TokenKind::Radius`):**

| Name | Value | Groups |
|---|---|---|
| `radius-sm` | `3px` | `["radius"]` |
| `radius-md` | `4px` | `["radius"]` |
| `radius-lg` | `6px` | `["radius"]` |

Radius tokens are unused by sidebar v1 (active row spans full width with
no radius); they remain in the palette for future components.

## §4 · File-level changes + bus pipeline

### `crates/sola-core/src/theme.rs` — rewrite

Replace `Theme` / `Colors` / `Typography` / `Spacing` / `Radius` with the
types from §2.

`Default::default()` builds the seed: the palette and group tags from
§3, plus `ComponentBindings` for `page` and `sidebar`.

Replace `to_css_vars(&self) -> BTreeMap<String, String>` with
`to_css(&self) -> String` returning the full `:root { … }` block (atoms
first, then scoped vars). Rust is the authoritative renderer; apps
never reimplement the lowering.

Add `validate(&self) -> Result<(), Vec<ValidationError>>` (see §5).

### `crates/sola-bus/src/topics.rs` — keep the topic, change the inner type

`Topic::Theme(Theme)` stays `#[persistent]`. Only the inner type from
`sola-core` changes shape. No bus-protocol-level edits beyond the
implicit re-export.

### Bus → CSS pipeline (sola-kit)

```
[any app]  ctx.emit(Topic::Theme(new_theme))
   │
   ▼
[sola-bus]  persistent topic, broadcast to every subscriber
   │
   ▼
[every kit window]  Rust-side bus handler converts `Theme → to_css()`
                    once, then sends the CSS string to the renderer via
                    `__solaRecv` as `{ event: "theme", css: "<root>{…}" }`
   │
   ▼
[renderer]  index.tsx's `on("theme", …)` does
            `themeSheet.replaceSync(msg.css)`  (already exists)
```

The renderer never sees the structured `Theme` for plain *application*.
It only sees CSS. The structured `Theme` is for *editing*, and editing
is a kit-only concern (theme editor mutates `Theme` → emits
`Topic::Theme` → the cycle above).

**Wiring gap to close.** The renderer-side subscriber
(`index.tsx` `on("theme", …)`) already exists and is correct. The
Rust-side handler that *produces* the `{ event: "theme", css: … }`
payload does **not** yet exist — `KitApp::on_theme` currently only
updates the in-memory copy, with a comment claiming "the framework's
bus loop is responsible" that does not match reality. The
implementation closes this gap by pushing `theme.to_css()` to every
kit-managed window via `WindowHandle::send_to_js`. The implementer
chooses where this lives:

- **Framework default** in `crates/sola-kit/src/lib.rs` (preferred —
  every kit-based app gets it for free; storybook overrides
  `on_theme` only for the in-memory mirror), or
- **App-level** in `KitApp::on_theme` (simpler diff; revisit when a
  second kit-based app appears).

Either is acceptable for this spec; the contract is that on every
`Topic::Theme` delivery, every kit-managed window receives a fresh
`{ event: "theme", css: <to_css output> }`.

### `crates/sola-kit/src/app/catalog.rs` — delete

No replacement. `crates/sola-kit/src/app/app.rs` drops:

- the `use super::catalog::{CATALOG, Group};`
- the `catalog_json` building block
- the `"catalog": catalog_json` key in `initial_state`

The storybook discovers what it needs from the live theme directly via
the bindings table (future theme-editor work).

### Sidebar component code — kit-shipped framework primitive

Lives in `crates/sola-kit/web/lib/components/sidebar.ts` (one file,
three custom elements: `<sola-sidebar>`, `<sola-sidebar-section>`,
`<sola-sidebar-item>`). Sibling stylesheet
`crates/sola-kit/web/lib/components/sidebar.css` uses
`var(--sola-sidebar-*)` exclusively, plus inherited `var(--sola-page-*)`
where appropriate.

Both files are added to `platform_assets()` in
`crates/sola-kit/src/assets.rs` so the kit serves them at
`/lib/components/sidebar.ts` and `/lib/components/sidebar.css` for any
app under `app://`.

The kit does **not** manage a shared importmap (`AppCtx::add_window`
documents that each `index.html` declares its own importmap — the kit
makes no JS-framework assumption). Each consuming app adds the entry
itself:

- The storybook's `crates/sola-kit/web/index.html` adds
  `"@sola/sidebar": "/lib/components/sidebar.ts"` to its existing
  importmap and a matching `<link rel="stylesheet"
  href="/lib/components/sidebar.css">` (per kit convention — no
  JS-side CSS imports).
- The storybook's `crates/sola-kit/web/tsconfig.json` adds
  `"@sola/sidebar": ["./lib/components/sidebar.ts"]` under `paths` so
  the LSP resolves the import in the editor.
- Other apps replicate the same two entries when they adopt the
  component.

After the importmap entry, apps just write `import "@sola/sidebar"` and
the three custom elements register themselves.

### Storybook (`crates/sola-kit/web/components/Main.tsx`) — uses the new sidebar

The current ping-counter spike is replaced with a layout that renders
`<sola-sidebar>` on the left and a content pane on the right. Sidebar
content uses label-only items (the storybook's preference; the rich
slots remain available to other apps).

Initial sections in the storybook sidebar are minimal — placeholder
"Components" and "Theme" sections, since this design itself ships only
the sidebar component. Subsequent component PRs add their own items.

## §5 · Deferred / invariants / tests

### Deferred

See §1 for sidebar-side deferred items. Theme-side deferred:

- **Theme editor UI** in storybook — needed soon, but a separate spec.
  The pipeline above is shaped to support it: editor mutates `Theme`,
  calls `validate()`, emits `Topic::Theme(theme)`, every webview re-skins.
- **Other kit components** (button, field, badge, …) — each adds its
  own entry to `Theme.components` with new slots and groups when
  introduced. The protocol is open for additions.

### Invariants enforced by `Theme::validate(&self) -> Result<(), Vec<ValidationError>>`

Every editor-side mutation path round-trips through `validate` before
publishing to the bus.

1. Every `Binding.token` exists in `Palette.tokens`.
2. `palette.tokens[binding.token].groups` contains `binding.group`.
3. Every `Token.kind` is consistent with the group's expected kind
   (groups are colors-only, sizing-only, font-only, etc., never mixed —
   enforced via a static map of group → kind in `theme.rs`).
4. Token names are unique (enforced by `BTreeMap` keying).
5. Every component referenced in CSS emission exists in
   `Theme.components` (the seed includes `page` and `sidebar`).

`Theme::default()` is asserted valid by a test.

### Rust tests (`sola-core::theme`)

- `default_validates_clean`
- `default_to_css_is_stable` — golden snapshot of the rendered `:root { … }` block
- `theme_round_trips_through_toml`
- `validate_rejects_dangling_token` — mutate a binding to point at a
  nonexistent token; expect error
- `validate_rejects_group_mismatch` — point a `border` slot at a
  `surface`-only token; expect error

### Web-side tests / smoke

- Mounting the three-element JSX from §1 produces the expected DOM
  shape (custom elements registered, slots populated).
- Setting `active` on an item flips its visual state (CSS-only, no
  Rust involvement).
- Clicking an item dispatches a bubbling `sola-select` `CustomEvent`.

## Implementation order

1. **`sola-core::theme`** — type rewrite, `Default::default()` seed,
   `to_css(&self)`, `validate(&self)`, all tests. No dependents touched
   yet; `cargo build` in isolation.
2. **`sola-bus::topics`** — verify `Topic::Theme` still compiles
   against the new inner type. Likely zero-line edit beyond a re-export
   refresh.
3. **`sola-kit` Rust side** — adapt the Rust-side bus handler that
   produces the `__solaRecv` `{ event: "theme", css: ... }` payload to
   call the new `to_css()`. Delete `catalog.rs` and `catalog_json`
   plumbing in `app.rs`.
4. **Sidebar TS + CSS** — add `web/lib/components/sidebar.{ts,css}`,
   register in `platform_assets()`, add import-map entry.
5. **Storybook `Main.tsx`** — replace the spike with a real layout
   using the new sidebar.
6. **Build** with `cargo make build sola-kit`. Do **not** install or
   run; the user reviews the diff before either.

## Open questions

None at draft time. If discoveries during implementation reveal new
ambiguity, the implementer surfaces them before pressing on.
