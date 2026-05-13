# sola-kit — Design

**Date:** 2026-04-30
**Status:** Design — pending implementation plan

## 1. Motivation

Every Sola app today ships its own copy of `theme.css` with the same ~14 color custom properties, font families, and a sprinkle of overlapping component CSS (`.btn`, `.field`, `.row`, `.badge`, `.section`, `.nav-item`, etc.). Spacing, radius, and type sizes are inlined ad hoc across files. There is no shared component code; identical patterns drift between apps over time.

`sola-kit` is a new crate that:
1. Owns a single source of truth for design tokens (colors, typography, spacing, radius).
2. Distributes tokens at runtime via a persistent bus topic so all apps can update live.
3. Ships a shared component library (Arrow.js templates + CSS) that apps consume through one import path.
4. Provides a binary that is *both* the theme editor and the storybook for the component library — dogfooded by being the sole consumer in v1.

## 2. Scope

**In v1:**
- New crate `crates/sola-kit/` with a self-contained framework, component library, and binary.
- Token data model (`Theme`) + bus topic (`Topic::Theme`).
- Component library:
  - **Atoms:** Button, Field, Badge, Icon
  - **Components:** Sidebar, NavItem, Section, Row, List, Form, Tabs/Tab, Toast, Empty
- Editor + storybook binary (the kit's own frontend), launchable from the Sola launcher as "Theme".

**Out of v1:**
- Migrating any existing app off `sola-app` onto `sola-kit`. Apps continue using `sola-app` and their per-app `theme.css` files until separately ported.
- Renaming `sola-app` → `sola-kit`. The two crates coexist until the last consumer migrates off `sola-app`, at which point `sola-app` is deleted.
- Multiple themes / theme presets / light-vs-dark switching. The model is a single editable theme.
- Components without a current consumer: Dialog, Tooltip, Menu/Popover, Checkbox/Toggle, Chip, address bar, autocomplete dropdown, JSON viewer, drag-handles. Added as their consuming apps appear.

## 3. Architecture

### 3.1 Crate layout

`crates/sola-kit/` is a **library + binary** crate, fully self-contained. It depends on `sola-bus`, `sola-core`, and `sola-assets`. It does **not** depend on `sola-app`.

```
crates/sola-kit/
  Cargo.toml                     # [lib] + [[bin]] name="sola-kit" path="src/app/main.rs"
  src/
    lib.rs                       # framework public API (copied from sola-app/src/lib.rs)
    assets.rs                    # COPIED from sola-app
    async_dispatch.rs            # COPIED
    bridge.rs                    # COPIED
    ctx.rs                       # COPIED
    strip.rs                     # COPIED
    webview.rs                   # COPIED
    window.rs                    # COPIED
    theme.rs                     # NEW — color conversion (HEX↔OKLCH) and other editor-only helpers
    app/                         # binary: the kit's own editor/storybook
      main.rs                    # entry — sola_kit::run::<KitApp>()
      kit_app.rs                 # SolaApp impl + bus handlers
      tokens.rs                  # OKLCH/HEX conversion, persistence helpers
      catalog.rs                 # static catalog of atoms + components for the sidebar
  web/
    lib/                         # JS library shipped to consumers via asset_bundle!
      ipc.ts                     # COPIED from sola-app/web/lib/
      store.ts                   # COPIED
      kit.ts                     # NEW — exports applyTheme + every atom/component
      kit.css                    # NEW — :root token defaults + .kit-* component CSS
      components/
        button.ts, field.ts, badge.ts, icon.ts
        sidebar.ts, nav-item.ts, section.ts, row.ts, list.ts, form.ts, tabs.ts, toast.ts, empty.ts
      vendor/arrow/*             # COPIED from sola-app/web/vendor/
    app/                         # the kit binary's own frontend — only embedded by the binary
      index.html
      src/
        main.ts, app.ts, sidebar.ts, app.css
        preview/*.ts             # one preview module per atom/component
```

**Why lib + binary in one crate:** the binary is the storybook for the very components the library exports. Colocation makes drift impossible — editing a component template and its showcase happen in the same `cargo make build` cycle.

**Public surface boundary:**
- `src/lib.rs` re-exports only framework public types (`SolaApp`, `AppCtx`, `WindowConfig`, `BusRegistry`, `asset_bundle!`, the `theme` module).
- `src/app/` is referenced exclusively from `src/app/main.rs`. Cargo enforces that `lib.rs` cannot reach into it.
- `web/lib/` is included in *every* asset bundle the kit serves (consumers later embed these files from their own `asset_bundle!`).
- `web/app/` is included **only** in the binary's `asset_bundle!`.

**Naming dichotomy** — same word ("app" / "lib") used on both sides:

| Role | Rust | Web |
|---|---|---|
| Library (consumed by other apps) | `src/lib.rs` + framework modules | `web/lib/` |
| App (kit's own binary frontend) | `src/app/main.rs` + sibling modules | `web/app/` |

### 3.2 Touch sites outside sola-kit

1. **`crates/sola-bus/src/topics.rs`** — add a topic variant:
   ```rust
   #[persistent]
   Theme(Theme),
   ```
2. **`crates/sola-core/src/theme.rs`** (new file) — define `Theme`, `Colors`, `Typography`, `Spacing`, `Radius`, and `Default` impls. Lives in `sola-core` because `sola-bus` depends on `sola-core` (same dependency direction as `Application`).
3. **`crates/sola-core/src/applications.rs`** — add the kit to `builtin_apps()`:
   ```rust
   Application {
       app_id: "sola-kit".into(),
       label: "Theme".into(),
       command: "/opt/sola/bin/sola-kit".into(),
       icon: "lucide/palette".into(),
   }
   ```

That is the entire blast radius. `sola-app`, `sola-shell`, `sola-settings`, `sola-terminal`, `sola-browser`, `sola-monitor` are untouched.

### 3.3 Migration path for other apps (future work)

Per-app PRs, each independent. For one app (e.g. `sola-settings`):
1. `Cargo.toml`: swap `sola-app = { path = "../sola-app" }` → `sola-kit = { path = "../sola-kit" }`.
2. `sed -i 's/sola_app::/sola_kit::/g'` across `crates/sola-settings/src/`.
3. Delete `crates/sola-settings/web/src/theme.css`.
4. Update `web/index.html`: replace `<link href="/src/theme.css">` with `<link href="/lib/kit.css">`.
5. Optionally swap inline-CSS classes for `@sola/kit` component imports (incrementally, in the same or follow-up PRs).
6. `cargo make build`; smoke-test.

When the last consumer migrates off `sola-app`, delete `crates/sola-app/`.

**Maintenance discipline during the window:** treat `sola-app` as frozen. Critical fixes only. New framework work lands in `sola-kit`. The longer the migration window, the more this matters — but bounded drift is acceptable because consumers only see one of the two.

## 4. JS API surface (`@sola/kit`)

### 4.1 Single import path

```ts
import {
  applyTheme,
  button, field, badge, icon,
  sidebar, navItem, section, row, list, form, tabs, tab, toast, empty,
} from '@sola/kit';
```

The kit's `inject_import_map` (copied and adjusted from sola-app's) registers four module names served from the kit's `web/lib/`:

```jsonc
{
  "@arrow-js/core": "/vendor/arrow/index.mjs",
  "@sola/ipc":      "/lib/ipc.js",
  "@sola/store":    "/lib/store.js",
  "@sola/kit":      "/lib/kit.js"
}
```

The kit deliberately drops sola-app's `"@sola/theme"` stub — that name dies with the migration; the proper module is `@sola/kit`.

### 4.2 Atoms and components return Arrow templates

```ts
button({ label: 'Save', variant: 'primary', onClick: () => save() })
// returns an Arrow template; embed inside any html`...`
```

Reactive options accept either values or closures:

```ts
field({
  value: () => state.email,
  onInput: v => state.email = v,
  error: () => state.emailError,
})
```

Body-style options accept Arrow templates as slots:

```ts
section({
  title: 'Mail accounts',
  description: 'Configure IMAP/SMTP per account.',
  body: html`${() => state.accounts.map(a => row({ label: a.email, actions: html`...` }))}`,
})
```

### 4.3 Tabs — variants via slot composition

`Tab` exposes `leading` and `trailing` slots so terminal-style numbered tabs and browser-style favicon-with-reload tabs share one implementation:

```ts
interface TabOpts {
  title: string | (() => string);
  active?: boolean | (() => boolean);
  onClick?: () => void;
  onClose?: () => void;
  leading?: TemplatePartial;       // terminal: number; browser: favicon
  trailing?: TemplatePartial;      // browser: reload button
}
```

Common cases get a `variant?: 'numbered' | 'favicon'` shortcut that pre-fills the slots; the slots remain the foundation.

### 4.4 Token-usage metadata (per component)

Each component module exports its token-usage list alongside the template:

```ts
export const buttonTokens = [
  '--accent', '--accent-dim', '--bg-tertiary', '--text-secondary',
  '--radius-sm', '--text-body', '--space-md',
];
export function button(opts: ButtonOpts): TemplatePartial { ... }
```

Both sides hold the catalog explicitly:

- **Rust** — `src/app/catalog.rs` defines a static array used to render the sidebar and to compute the reverse index ("what components use `--accent`?"):
  ```rust
  pub static CATALOG: &[(&str, &[&str])] = &[
      ("button", &["--accent", "--accent-dim", "--bg-tertiary", "--text-secondary",
                   "--radius-sm", "--text-body", "--space-md"]),
      ("field",  &["--bg-primary", "--border-subtle", "--accent",
                   "--text-primary", "--radius-sm", "--text-body", "--space-sm"]),
      // ...one row per atom + component
  ];
  ```
- **TypeScript** — each component module exports its own list (the `*Tokens` const above).

A unit test asserts parity: the Rust `CATALOG` row for `button` lists exactly the same vars as `buttonTokens` in `web/lib/components/button.ts`. Drift fails the test.

Declarative beats DOM-walking: zero runtime cost, local to the component file, lint-able.

### 4.5 CSS layer

A single bundle `web/lib/kit.css` ships both:
- `:root { --bg-primary: ...; --accent: ...; --space-md: ...; ... }` static defaults.
- `.kit-button`, `.kit-field`, `.kit-tab`, etc. component classes.

Apps embed two stylesheets:
```html
<link rel="stylesheet" href="/lib/kit.css">
<link rel="stylesheet" href="/src/app.css">       <!-- optional app-specific overrides -->
```

All component classes are prefixed `kit-` to avoid collision with app-local CSS during migration.

### 4.6 Auto-wired distribution

`@sola/kit` self-installs a bus listener on import: it subscribes to a `'theme'` event from the framework and calls `applyTheme(payload.vars)`. The framework's `lib.rs::run<A>` adds `Topic::Theme` to its automatic subscription set (alongside `Shutdown`, `Windows`, `Copy`, `Paste`, `Evaluate`) and forwards each one to the WebView. No app-side opt-in code; subscribing to live tokens is automatic for any app on the kit.

## 5. Token data model + bus schema

### 5.1 Rust schema

`crates/sola-core/src/theme.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Theme {
    pub colors: Colors,
    pub typography: Typography,
    pub spacing: Spacing,
    pub radius: Radius,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Colors {
    pub bg_primary: String, pub bg_secondary: String, pub bg_tertiary: String, pub bg_hover: String,
    pub border: String, pub border_subtle: String,
    pub text_primary: String, pub text_secondary: String, pub text_tertiary: String,
    pub text_muted: String, pub text_accent: String,
    pub accent: String, pub accent_dim: String,
    pub danger: String, pub success: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Typography {
    pub font_sans: String, pub font_mono: String,
    pub text_caption: String, pub text_body: String, pub text_body_lg: String,
    pub text_heading: String, pub text_display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Spacing { pub xs: String, pub sm: String, pub md: String, pub lg: String, pub xl: String, pub xxl: String }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Radius { pub sm: String, pub md: String, pub lg: String }

impl Theme { pub fn to_css_vars(&self) -> HashMap<String, String> { /* flat --var → value */ } }
impl Default for Theme { /* baked-in dark palette in current use */ }
```

### 5.2 Semantic renames

The migration touches every var name the kit cares about anyway, so we rename a few from appearance-based to intent-based:

| Old | New |
|---|---|
| `--cyan` | `--accent` |
| `--cyan-dim` | `--accent-dim` |
| `--red` | `--danger` |
| `--green` | `--success` |

Default values stay cyan / red / green. Future themes can recolor `--accent` without misnaming.

### 5.3 CSS variable naming

| Group | Names |
|---|---|
| Color | `--bg-primary`, `--bg-secondary`, `--bg-tertiary`, `--bg-hover`, `--border`, `--border-subtle`, `--text-primary`, `--text-secondary`, `--text-tertiary`, `--text-muted`, `--text-accent`, `--accent`, `--accent-dim`, `--danger`, `--success` |
| Typography | `--font-sans`, `--font-mono`, `--text-caption` (11px), `--text-body` (12px), `--text-body-lg` (13px), `--text-heading` (16px), `--text-display` (20px) |
| Spacing | `--space-xs` (4), `--space-sm` (8), `--space-md` (12), `--space-lg` (16), `--space-xl` (20), `--space-xxl` (24) |
| Radius | `--radius-sm` (3), `--radius-md` (4), `--radius-lg` (6) |

### 5.4 Bus topic

```rust
// crates/sola-bus/src/topics.rs
pub use sola_core::theme::Theme;

// in define_topics! { ... }
#[persistent]
Theme(Theme),
```

Persistent — survives bus restart and sola sessions, replays on subscribe. Same pattern as `MailConfig`.

### 5.5 JS payload

The Rust side flattens `Theme` via `to_css_vars()` before sending to the WebView. JS sees a flat var map:

```jsonc
{
  "vars": {
    "--bg-primary": "#0d1117",
    "--accent": "#00d4ff",
    "--font-sans": "DM Sans, system-ui, sans-serif",
    "--text-body": "12px",
    "--space-md": "12px",
    "--radius-sm": "3px"
  }
}
```

### 5.6 Defaults & sync

`Theme::default()` returns the current dark palette already in use across the existing apps:

| Var | Default |
|---|---|
| `--bg-primary` | `#0d1117` |
| `--bg-secondary` | `#161b22` |
| `--bg-tertiary` | `#1c2129` |
| `--bg-hover` | `#1a2030` |
| `--border` | `#2d333b` |
| `--border-subtle` | `#21262d` |
| `--text-primary` | `#e6edf3` |
| `--text-secondary` | `#8b949e` |
| `--text-tertiary` | `#6e7681` |
| `--text-muted` | `#484f58` |
| `--text-accent` | `#58a6ff` |
| `--accent` | `#00d4ff` |
| `--accent-dim` | `rgba(0, 212, 255, 0.12)` |
| `--danger` | `#f85149` |
| `--success` | `#3fb950` |
| `--font-sans` | `'DM Sans', system-ui, sans-serif` |
| `--font-mono` | `'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace` |
| `--text-caption` / `--text-body` / `--text-body-lg` / `--text-heading` / `--text-display` | `11px` / `12px` / `13px` / `16px` / `20px` |
| `--space-xs` / `--space-sm` / `--space-md` / `--space-lg` / `--space-xl` / `--space-xxl` | `4px` / `8px` / `12px` / `16px` / `20px` / `24px` |
| `--radius-sm` / `--radius-md` / `--radius-lg` | `3px` / `4px` / `6px` |

`web/lib/kit.css` declares the same values as static `:root` properties so apps render correctly even before any `Topic::Theme` arrives. A unit test asserts `Theme::default().to_css_vars()` matches values declared in `kit.css` to prevent drift.

### 5.7 Partial edits

`Theme` has no `Option` fields. The editor always emits the full struct. Persistence stays simple; consumers never merge partial overlays.

## 6. Editor / storybook UX

### 6.1 Layout

Two-pane: sidebar nav on the left, work area on the right. Visual mockups in `.superpowers/brainstorm/917866-1777577285/content/layout-v3.html`.

**Sidebar groups:**
- **Tokens** — Colors, Typography, Spacing & radius
- **Atoms** — Button, Field, Badge, Icon
- **Components** — Sidebar, NavItem, Section, Row, List, Form, Tabs, Toast, Empty

When a token is selected, sidebar items that consume it are marked with an indicator (e.g. `●`).

**Sola-kit dogfoods.** The sidebar is `sidebar(...)` + `navItem(...)` from `@sola/kit`; section headers in the work area are `section(...)`; lists are `list(...)`; etc. If a kit component looks wrong, the editor itself looks wrong.

### 6.2 Token mode (when a token is selected)

Work area top: editor strip with the swatch, hex/HSL/OKLCH inputs, lightness slider, Reset/Save buttons, and a "used in N atoms · M components" caption.

Below: a grid of mini-preview tiles, one per consuming component. Each tile labels the variant and shows the component rendered with the live values. Editing the swatch ripples through the grid in real time.

### 6.3 Component mode (when an atom or component is selected)

Work area top: live previews of all variants (e.g. Button shows primary, default, ghost, danger, add side-by-side).

Below: editable token chips — each chip is a swatch + name + value. Click a chip to edit in place. A color chip opens a color picker; spacing/radius opens a numeric stepper; font opens a font picker. Edits propagate live (same lifecycle as token mode).

### 6.4 Cross-highlighting

- **Token mode:** hovering a tile in the grid highlights its tokens at the top of the strip; hovering a sidebar item that uses the current token highlights it in the wall.
- **Component mode:** hovering a chip outlines the parts of the live preview using it. Implemented via `data-uses-token` attributes on preview elements + `[data-token-hover='--accent']` selectors driven by mouseover.

### 6.5 Edit lifecycle

JS holds the full `Theme` as JSON state (mirrors the Rust struct one-for-one). KitApp seeds this on window creation by passing the current `Theme` as `initial_state`. Edits mutate this JS-side struct, then propagate.

1. **User tweaks** — slider/input fires per keystroke. JS updates its local `Theme` struct (e.g. `state.theme.colors.accent = '#00ffaa'`).
2. **Local apply** — JS recomputes the affected CSS var(s) and calls `applyTheme({ '--accent': '#00ffaa' })` immediately, mutating `:root`. Storybook reflows within a frame.
3. **Debounced bus emit** — 300 ms after the last change, JS calls `invoke('theme_set', { theme: state.theme })` with the *full* `Theme` JSON. KitApp deserializes via serde into a `Theme`, replaces its in-memory copy, and calls `ctx.emit(Topic::Theme(theme.clone()))`. The bus persists and broadcasts. KitApp's own subscriber receives it back; idempotent (values already applied locally).
4. **Reset** — JS restores its local state to the default `Theme`, applies all default vars, then sends the default theme on the same path.

Sending the full `Theme` (rather than a diff) keeps the Rust side simple: no inverse mapping from flat CSS-var keys back to struct fields. Matches the pattern used by `mail_save_account` in `sola-settings`.

Debounce on the bus emit (not the local apply): typing feels instantaneous; we don't broadcast 80 messages per slider drag.

### 6.6 State that lives in sola-kit (not on the bus)

- Sidebar selection (which token/atom/component is open) — not persisted; resets to "Colors" overview on launch.
- Optional "unsaved" indicator while the debounce is pending.

### 6.7 Reset semantics

The persistent topic always carries a *full* `Theme`. Even editing one var causes the full struct to be persisted. "Reset" emits `Theme::default()` rather than retracting — simpler reasoning, the topic is always present once the user has touched anything.

## 7. Testing

- **Unit:** `Theme::default().to_css_vars()` matches values in `web/lib/kit.css` (drift guard).
- **Unit:** every `*Tokens` export references CSS vars defined in `kit.css` (drift guard).
- **Unit:** `Theme` round-trips through TOML (matches existing topic round-trip tests in `sola-bus`).
- **Manual:** launch sola-kit, verify storybook renders all atoms + components without console errors; tweak each token category and verify live propagation.
- **Manual:** restart sola-kit; verify the persistent theme is restored.

## 8. Open questions / future work

- **Export / import themes as TOML or JSON.** Useful for sharing; not needed in v1.
- **Multiple named theme presets.** Single editable theme is the v1 scope. Adding presets later means making the existing theme the "active" preset and adding a "themes list" UI; no schema change required for consumers.
- **Light/dark scheme switching.** Out of scope for v1; could be modeled as preset switching per the previous bullet.
- **Lint job** that compares declared `*Tokens` lists against `kit.css` `var()` references, beyond the unit test.
- **Per-component CSS isolation.** Currently all component CSS lives in one `kit.css`. If the kit grows large, splitting per-component is a future optimization (with a small build step to bundle them).

## 9. Summary

`sola-kit` is a self-contained crate that ships a token model, a bus topic, a component library, and the editor/storybook that operates on them. It exists in parallel to `sola-app` until apps migrate off one at a time. v1 is dogfooded — sola-kit is the only consumer of `@sola/kit` until other apps are ported, which de-risks the kit by validating the design end-to-end inside one window before any other app risks regression.
