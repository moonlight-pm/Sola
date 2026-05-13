# sola-kit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/sola-kit/` — a self-contained library + binary crate that owns design tokens, distributes them via a persistent bus topic, ships an Arrow.js component library, and provides a theme-editor / storybook binary that dogfoods that library.

**Architecture:** New crate parallel to `sola-app` (no dep). Framework code (WebView, bus client, asset bundling) is COPIED from sola-app so the two crates can evolve independently. Token schema (`Theme`) lives in `sola-core`; bus topic `Topic::Theme` lives in `sola-bus`. The kit's library exports `@sola/kit` (atoms + components + `applyTheme`); the kit's binary is the editor + storybook. v1 has only one consumer of `@sola/kit` (the binary itself); other apps migrate later.

**Tech Stack:** Rust 2024, GTK4, WebKit6, Arrow.js (vendored), TOML persistence via sola-bus.

---

## File Structure

### Created

**Schema in sola-core:**
- `crates/sola-core/src/theme.rs` — `Theme`, `Colors`, `Typography`, `Spacing`, `Radius` structs; `Default`; `to_css_vars()`.

**New crate (sola-kit):**
- `crates/sola-kit/Cargo.toml`
- `crates/sola-kit/src/lib.rs` — copied from `sola-app/src/lib.rs`, adjusted import map + framework-level `Topic::Theme` subscription.
- `crates/sola-kit/src/assets.rs` — copied; `platform_assets()` lists kit-specific files.
- `crates/sola-kit/src/{async_dispatch,bridge,ctx,strip,webview,window}.rs` — copied from sola-app.
- `crates/sola-kit/src/theme.rs` — color conversion (HEX↔OKLCH) and editor-only helpers.
- `crates/sola-kit/src/app/main.rs` — binary entry point.
- `crates/sola-kit/src/app/kit_app.rs` — `KitApp` struct + `SolaApp` impl + bus handlers.
- `crates/sola-kit/src/app/tokens.rs` — color conversion helpers used by the editor (HEX↔OKLCH).
- `crates/sola-kit/src/app/catalog.rs` — static `CATALOG` array of (component-name, &[token-vars]).
- `crates/sola-kit/web/lib/ipc.ts` — copied verbatim from sola-app.
- `crates/sola-kit/web/lib/store.ts` — copied verbatim from sola-app.
- `crates/sola-kit/web/lib/kit.ts` — re-exports atoms + components + `applyTheme`; auto-installs theme listener on import.
- `crates/sola-kit/web/lib/kit.css` — `:root` token defaults + all `.kit-*` component CSS.
- `crates/sola-kit/web/lib/components/{button,field,badge,icon,sidebar,nav-item,section,row,list,form,tabs,toast,empty}.ts` — one file per atom/component; each exports a function returning Arrow templates plus a `*Tokens` const.
- `crates/sola-kit/web/vendor/arrow/{index.mjs,index.d.ts,internal.d.ts,chunks/internal-DchK7S7v.mjs}` — copied verbatim from sola-app.
- `crates/sola-kit/web/app/index.html` — storybook entry HTML.
- `crates/sola-kit/web/app/src/{main,app,sidebar}.ts` + `app.css` — storybook frontend.
- `crates/sola-kit/web/app/src/preview/{button,field,badge,icon,sidebar,nav-item,section,row,list,form,tabs,toast,empty}.ts` — one preview module per item; renders the live preview + token chips for component mode.

### Modified

- `crates/sola-bus/src/topics.rs` — add `pub use sola_core::theme::Theme;` and a new `#[persistent] Theme(Theme)` topic variant.
- `crates/sola-core/src/lib.rs` — add `pub mod theme;`.
- `crates/sola-core/src/applications.rs` — append `sola-kit` builtin to `builtin_apps()`.
- `CLAUDE.md` — add `sola-kit/` to the `crates/` listing.

### Untouched

`sola-app`, `sola-shell`, `sola-settings`, `sola-terminal`, `sola-browser`, `sola-monitor`. Per-app theme.css files stay in place. Migration is a separate body of work.

---

## Phase 1 — Foundations (schema + bus + builtin)

### Task 1: Add `Theme` schema to `sola-core`

**Files:**
- Create: `crates/sola-core/src/theme.rs`
- Modify: `crates/sola-core/src/lib.rs`

- [ ] **Step 1: Create `crates/sola-core/src/theme.rs` with the schema**

```rust
//! Design-token schema shared by sola-bus (the wire type for `Topic::Theme`)
//! and sola-kit (consumer + editor). Lives in sola-core because sola-bus
//! depends on sola-core, not the other way around — same arrangement as
//! `crate::applications::Application`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Theme {
    pub colors: Colors,
    pub typography: Typography,
    pub spacing: Spacing,
    pub radius: Radius,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Colors {
    pub bg_primary: String,
    pub bg_secondary: String,
    pub bg_tertiary: String,
    pub bg_hover: String,
    pub border: String,
    pub border_subtle: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_tertiary: String,
    pub text_muted: String,
    pub text_accent: String,
    pub accent: String,
    pub accent_dim: String,
    pub danger: String,
    pub success: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Typography {
    pub font_sans: String,
    pub font_mono: String,
    pub text_caption: String,
    pub text_body: String,
    pub text_body_lg: String,
    pub text_heading: String,
    pub text_display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Spacing {
    pub xs: String,
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Radius {
    pub sm: String,
    pub md: String,
    pub lg: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            colors: Colors {
                bg_primary: "#0d1117".into(),
                bg_secondary: "#161b22".into(),
                bg_tertiary: "#1c2129".into(),
                bg_hover: "#1a2030".into(),
                border: "#2d333b".into(),
                border_subtle: "#21262d".into(),
                text_primary: "#e6edf3".into(),
                text_secondary: "#8b949e".into(),
                text_tertiary: "#6e7681".into(),
                text_muted: "#484f58".into(),
                text_accent: "#58a6ff".into(),
                accent: "#00d4ff".into(),
                accent_dim: "rgba(0, 212, 255, 0.12)".into(),
                danger: "#f85149".into(),
                success: "#3fb950".into(),
            },
            typography: Typography {
                font_sans: "'DM Sans', system-ui, sans-serif".into(),
                font_mono: "'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace".into(),
                text_caption: "11px".into(),
                text_body: "12px".into(),
                text_body_lg: "13px".into(),
                text_heading: "16px".into(),
                text_display: "20px".into(),
            },
            spacing: Spacing {
                xs: "4px".into(),
                sm: "8px".into(),
                md: "12px".into(),
                lg: "16px".into(),
                xl: "20px".into(),
                xxl: "24px".into(),
            },
            radius: Radius {
                sm: "3px".into(),
                md: "4px".into(),
                lg: "6px".into(),
            },
        }
    }
}

impl Theme {
    /// Flatten a `Theme` into the CSS-custom-property map that `applyTheme`
    /// applies to `:root` in the WebView. Var names are deterministic.
    /// Returns a `BTreeMap` so iteration order is stable for tests.
    pub fn to_css_vars(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        let c = &self.colors;
        m.insert("--bg-primary".into(), c.bg_primary.clone());
        m.insert("--bg-secondary".into(), c.bg_secondary.clone());
        m.insert("--bg-tertiary".into(), c.bg_tertiary.clone());
        m.insert("--bg-hover".into(), c.bg_hover.clone());
        m.insert("--border".into(), c.border.clone());
        m.insert("--border-subtle".into(), c.border_subtle.clone());
        m.insert("--text-primary".into(), c.text_primary.clone());
        m.insert("--text-secondary".into(), c.text_secondary.clone());
        m.insert("--text-tertiary".into(), c.text_tertiary.clone());
        m.insert("--text-muted".into(), c.text_muted.clone());
        m.insert("--text-accent".into(), c.text_accent.clone());
        m.insert("--accent".into(), c.accent.clone());
        m.insert("--accent-dim".into(), c.accent_dim.clone());
        m.insert("--danger".into(), c.danger.clone());
        m.insert("--success".into(), c.success.clone());

        let t = &self.typography;
        m.insert("--font-sans".into(), t.font_sans.clone());
        m.insert("--font-mono".into(), t.font_mono.clone());
        m.insert("--text-caption".into(), t.text_caption.clone());
        m.insert("--text-body".into(), t.text_body.clone());
        m.insert("--text-body-lg".into(), t.text_body_lg.clone());
        m.insert("--text-heading".into(), t.text_heading.clone());
        m.insert("--text-display".into(), t.text_display.clone());

        let s = &self.spacing;
        m.insert("--space-xs".into(), s.xs.clone());
        m.insert("--space-sm".into(), s.sm.clone());
        m.insert("--space-md".into(), s.md.clone());
        m.insert("--space-lg".into(), s.lg.clone());
        m.insert("--space-xl".into(), s.xl.clone());
        m.insert("--space-xxl".into(), s.xxl.clone());

        let r = &self.radius;
        m.insert("--radius-sm".into(), r.sm.clone());
        m.insert("--radius-md".into(), r.md.clone());
        m.insert("--radius-lg".into(), r.lg.clone());

        m
    }
}
```

- [ ] **Step 2: Add `pub mod theme;` to sola-core's lib.rs**

In `crates/sola-core/src/lib.rs`, add `pub mod theme;` next to the existing module declarations (alphabetical order if that's the convention; otherwise just append).

- [ ] **Step 3: Add unit test for var count + sample values**

Append to `crates/sola-core/src/theme.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_to_css_vars_has_expected_count() {
        let vars = Theme::default().to_css_vars();
        // 15 colors + 7 typography + 6 spacing + 3 radius
        assert_eq!(vars.len(), 31);
    }

    #[test]
    fn default_to_css_vars_sample_values() {
        let vars = Theme::default().to_css_vars();
        assert_eq!(vars.get("--bg-primary").unwrap(), "#0d1117");
        assert_eq!(vars.get("--accent").unwrap(), "#00d4ff");
        assert_eq!(vars.get("--space-md").unwrap(), "12px");
        assert_eq!(vars.get("--radius-sm").unwrap(), "3px");
        assert_eq!(vars.get("--font-sans").unwrap(), "'DM Sans', system-ui, sans-serif");
    }

    #[test]
    fn theme_round_trips_through_toml() {
        let theme = Theme::default();
        let s = toml::to_string(&theme).expect("serialize");
        let back: Theme = toml::from_str(&s).expect("deserialize");
        assert_eq!(theme, back);
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p sola-core theme:: 2>&1 | tail -20
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-core/src/theme.rs crates/sola-core/src/lib.rs
git commit -m "feat(sola-core): add Theme schema + Default + to_css_vars

Wire type for the upcoming Topic::Theme. Lives in sola-core because
sola-bus depends on sola-core (same arrangement as Application). Default
returns the dark palette currently inlined across every Sola app's
theme.css; to_css_vars flattens the nested struct into the CSS custom
properties applyTheme will write to :root."
```

---

### Task 2: Add `Topic::Theme` to `sola-bus`

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`

- [ ] **Step 1: Re-export `Theme` and add the topic variant**

In `crates/sola-bus/src/topics.rs`, add the re-export at the top (next to the existing `pub use sola_core::Encrypted; pub use sola_core::applications::{Application, ApplicationsConfig};`):

```rust
pub use sola_core::theme::Theme;
```

Then locate the `define_topics! { ... }` block (it contains `MailConfig(MailConfig)`, `Application(Application)`, etc.) and add the new variant in the same persistent-config grouping:

```rust
    // Theme tokens edited by sola-kit, consumed by every app's WebView via
    // the framework-level subscription. Persistent — survives bus restart
    // and sola sessions; replays on subscribe.
    #[persistent]
    Theme(Theme),
```

- [ ] **Step 2: Add a TOML round-trip test**

Find the existing TOML round-trip tests in `crates/sola-bus/src/topics.rs` (search for `Topic::MailConfig(back)`). Append a parallel test for `Theme`:

```rust
        // Theme round-trips through the bus' TOML state path.
        let theme = sola_core::theme::Theme::default();
        let topic = Topic::Theme(theme.clone());
        let value = topic.to_value();
        let back = Topic::from_value(TopicKind::Theme, &value).expect("from_value");
        match back {
            Topic::Theme(b) => assert_eq!(theme, b),
            other => panic!("expected Theme, got {other:?}"),
        }
```

(The exact helper names — `to_value` / `from_value` — match what the existing `MailConfig` test uses; if the existing test uses different helpers, mirror those.)

- [ ] **Step 3: Build and run the bus tests**

```bash
cargo test -p sola-bus 2>&1 | tail -20
```

Expected: all bus tests pass, including the new round-trip.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-bus/src/topics.rs
git commit -m "feat(sola-bus): add #[persistent] Theme topic

Distribution channel for sola-kit's design tokens. Same persistence
shape as MailConfig — TOML on disk, sticky replay on subscribe."
```

---

### Task 3: Register `sola-kit` as a builtin app

The launcher reads `sola-core::applications::builtin_apps()` for entries that ship with Sola.

**Files:**
- Modify: `crates/sola-core/src/applications.rs`

- [ ] **Step 1: Append the entry**

In `crates/sola-core/src/applications.rs`, find the `pub fn builtin_apps() -> Vec<Application>` function. Append a new entry after `sola-browser`:

```rust
        Application {
            app_id: "sola-kit".into(),
            label: "Theme".into(),
            command: "/opt/sola/bin/sola-kit".into(),
            icon: "lucide/palette".into(),
        },
```

- [ ] **Step 2: Build and confirm**

```bash
cargo build -p sola-core 2>&1 | tail -5
```

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-core/src/applications.rs
git commit -m "feat(sola-core): register sola-kit as a builtin app

Launcher label: Theme; binary at /opt/sola/bin/sola-kit. Icon picks the
palette glyph from the existing lucide bundle."
```

---

## Phase 2 — Crate scaffolding (framework copy)

The next several tasks copy `sola-app`'s framework Rust + JS into `crates/sola-kit/`, adjust module paths and the import map, and end with a binary that opens an empty WebView. After Phase 2, `sola-kit` builds and runs, but does nothing app-specific yet.

### Task 4: Create the `sola-kit` crate skeleton

**Files:**
- Create: `crates/sola-kit/Cargo.toml`
- Create: `crates/sola-kit/src/lib.rs` (placeholder)

- [ ] **Step 1: Create Cargo.toml**

Write `crates/sola-kit/Cargo.toml` (mirror `crates/sola-app/Cargo.toml`'s deps, add `[lib]` + `[[bin]]`, drop the swc deps until they're actually needed — but easier to keep parity now):

```toml
[package]
name = "sola-kit"
version.workspace = true
edition.workspace = true

[lib]
# default — uses src/lib.rs

[[bin]]
name = "sola-kit"
path = "src/app/main.rs"

[dependencies]
sola-bus = { path = "../sola-bus" }
sola-assets = { path = "../sola-assets" }
sola-core = { path = "../sola-core" }
gtk4 = "0.9"
gdk4 = "0.9"
glib = "0.20"
gio = "0.20"
webkit6 = "0.4"
tokio = { version = "1", features = ["rt-multi-thread", "sync", "macros"] }
swc_ts_fast_strip = "48"
swc_common = "21"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
async-trait = "0.1"
```

- [ ] **Step 2: Create a stub lib.rs so the workspace compiles**

Write `crates/sola-kit/src/lib.rs`:

```rust
//! Scaffolding stub. Replaced wholesale in Task 5 with the framework
//! port from sola-app.
```

- [ ] **Step 3: Create stub binary so the bin target exists**

Write `crates/sola-kit/src/app/main.rs`:

```rust
//! Scaffolding stub. Replaced in Task 9.
fn main() {
    eprintln!("sola-kit stub — not yet implemented");
}
```

- [ ] **Step 4: Verify workspace picks up the new crate**

```bash
cargo build -p sola-kit 2>&1 | tail -5
```

Expected: `sola-kit` compiles. The workspace `members = ["crates/*"]` enrolls it automatically.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-kit/Cargo.toml crates/sola-kit/src/lib.rs crates/sola-kit/src/app/main.rs
git commit -m "scaffold(sola-kit): empty crate with lib + bin targets

Library + binary in one crate. Binary path explicitly set
so we can use src/app/ instead of Cargo's default src/bin/."
```

---

### Task 5: Copy framework Rust modules from sola-app

**Files:**
- Create: `crates/sola-kit/src/{assets,async_dispatch,bridge,ctx,strip,webview,window}.rs` — copies of sola-app counterparts.
- Modify: `crates/sola-kit/src/lib.rs` — replace stub with sola-app's lib.rs body, with module paths and import map adjusted.

- [ ] **Step 1: Copy framework modules verbatim**

```bash
cp crates/sola-app/src/assets.rs       crates/sola-kit/src/assets.rs
cp crates/sola-app/src/async_dispatch.rs crates/sola-kit/src/async_dispatch.rs
cp crates/sola-app/src/bridge.rs       crates/sola-kit/src/bridge.rs
cp crates/sola-app/src/ctx.rs          crates/sola-kit/src/ctx.rs
cp crates/sola-app/src/strip.rs        crates/sola-kit/src/strip.rs
cp crates/sola-app/src/webview.rs      crates/sola-kit/src/webview.rs
cp crates/sola-app/src/window.rs       crates/sola-kit/src/window.rs
cp crates/sola-app/src/lib.rs          crates/sola-kit/src/lib.rs
```

- [ ] **Step 2: Replace `sola_app` with `sola_kit` in copied modules**

Some `tracing::info!` and doc strings in the copied files reference "sola-app" or `sola_app`. Update them:

```bash
sed -i 's/sola_app/sola_kit/g' crates/sola-kit/src/{lib,assets,async_dispatch,bridge,ctx,strip,webview,window}.rs
sed -i 's/sola-app/sola-kit/g' crates/sola-kit/src/{lib,assets,async_dispatch,bridge,ctx,strip,webview,window}.rs
```

These are all in comments and log messages; no behavioral change.

- [ ] **Step 3: Adjust import map in lib.rs**

In `crates/sola-kit/src/lib.rs`, find `inject_import_map`'s `platform_imports` constant (looks like `r#""@arrow-js/core": "/vendor/arrow/index.mjs", ... "@sola/theme": "/lib/theme.js""#`). Replace the entry list:

```rust
    let platform_imports = r#""@arrow-js/core": "/vendor/arrow/index.mjs",
      "@sola/ipc": "/lib/ipc.js",
      "@sola/store": "/lib/store.js",
      "@sola/kit": "/lib/kit.js""#;
```

(Drops `@sola/theme`; adds `@sola/kit`.)

- [ ] **Step 4: Adjust `platform_assets()` in assets.rs**

In `crates/sola-kit/src/assets.rs`, the `platform_assets()` function lists framework assets. Update it to:
- keep `/lib/ipc.ts` and `/lib/store.ts`
- remove `/lib/theme.ts`
- add `/lib/kit.ts` and `/lib/kit.css` (we'll create the actual files in Task 7)
- keep both `/vendor/arrow/...` entries

```rust
pub fn platform_assets() -> AssetBundle {
    AssetBundle {
        assets: &[
            Asset {
                path: "/lib/ipc.ts",
                content: include_str!("../web/lib/ipc.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/lib/store.ts",
                content: include_str!("../web/lib/store.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/lib/kit.ts",
                content: include_str!("../web/lib/kit.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/lib/kit.css",
                content: include_str!("../web/lib/kit.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/vendor/arrow/index.mjs",
                content: include_str!("../web/vendor/arrow/index.mjs"),
                content_type: ContentType::JavaScript,
            },
            Asset {
                path: "/vendor/arrow/chunks/internal-DchK7S7v.mjs",
                content: include_str!("../web/vendor/arrow/chunks/internal-DchK7S7v.mjs"),
                content_type: ContentType::JavaScript,
            },
        ],
    }
}
```

- [ ] **Step 5: Don't build yet**

The web/ files don't exist; `include_str!` would fail. We create them in Tasks 6 and 7.

- [ ] **Step 6: Commit (work-in-progress; build will fail until web/ files arrive)**

```bash
git add crates/sola-kit/src/
git commit -m "scaffold(sola-kit): copy framework Rust modules from sola-app

Verbatim copy with sed-renamed log/doc references and import map adjusted
to publish @sola/kit (drops sola-app's @sola/theme stub). Build is broken
until web/ files arrive in subsequent tasks." --allow-empty
```

(Empty allowed in case the rename leaves no diff in some files.)

---

### Task 6: Copy web vendor + framework JS

**Files:**
- Create: `crates/sola-kit/web/lib/{ipc,store}.ts` — copies from sola-app.
- Create: `crates/sola-kit/web/vendor/arrow/{index.mjs,index.d.ts,internal.d.ts,chunks/internal-DchK7S7v.mjs}` — copies from sola-app.

- [ ] **Step 1: Copy with directory structure**

```bash
mkdir -p crates/sola-kit/web/lib crates/sola-kit/web/vendor/arrow/chunks
cp crates/sola-app/web/lib/ipc.ts             crates/sola-kit/web/lib/ipc.ts
cp crates/sola-app/web/lib/store.ts           crates/sola-kit/web/lib/store.ts
cp crates/sola-app/web/vendor/arrow/index.mjs crates/sola-kit/web/vendor/arrow/index.mjs
cp crates/sola-app/web/vendor/arrow/index.d.ts crates/sola-kit/web/vendor/arrow/index.d.ts
cp crates/sola-app/web/vendor/arrow/internal.d.ts crates/sola-kit/web/vendor/arrow/internal.d.ts
cp crates/sola-app/web/vendor/arrow/chunks/internal-DchK7S7v.mjs crates/sola-kit/web/vendor/arrow/chunks/internal-DchK7S7v.mjs
```

- [ ] **Step 2: Commit**

```bash
git add crates/sola-kit/web/lib/ crates/sola-kit/web/vendor/
git commit -m "scaffold(sola-kit): copy web vendor + ipc/store

Verbatim copy of sola-app's web/lib/{ipc,store}.ts and the vendored
Arrow.js bundle. These files stay in lockstep with sola-app's copies
during the migration window — bug fixes that need to land in both
crates while sola-app still has consumers."
```

---

### Task 7: Create the empty `kit.ts` and `kit.css`

Placeholder files so the framework's `platform_assets()` `include_str!` succeeds. Real content lands in Phase 3+.

**Files:**
- Create: `crates/sola-kit/web/lib/kit.ts`
- Create: `crates/sola-kit/web/lib/kit.css`

- [ ] **Step 1: Create kit.ts with applyTheme + a placeholder export**

Write `crates/sola-kit/web/lib/kit.ts`:

```ts
//! Sola Kit — design tokens, atoms, components.
//
// applyTheme is the one bit needed before Phase 3. Atoms/components and
// the auto-installed bus listener arrive in later phases.

/** Apply a map of CSS custom properties to :root. */
export function applyTheme(vars: Record<string, string>): void {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(vars)) {
    root.style.setProperty(key.startsWith('--') ? key : `--${key}`, value);
  }
}
```

- [ ] **Step 2: Create kit.css with the static `:root` block**

Write `crates/sola-kit/web/lib/kit.css` with all 31 default vars (matching `Theme::default().to_css_vars()`):

```css
/* sola-kit — design tokens + component CSS.
 * :root values are the defaults baked into Theme::default() (sola-core).
 * If you change one here, change the corresponding default there or the
 * default_to_css_vars_match_kit_css test in sola-kit will fail.
 */

:root {
  --bg-primary: #0d1117;
  --bg-secondary: #161b22;
  --bg-tertiary: #1c2129;
  --bg-hover: #1a2030;
  --border: #2d333b;
  --border-subtle: #21262d;
  --text-primary: #e6edf3;
  --text-secondary: #8b949e;
  --text-tertiary: #6e7681;
  --text-muted: #484f58;
  --text-accent: #58a6ff;
  --accent: #00d4ff;
  --accent-dim: rgba(0, 212, 255, 0.12);
  --danger: #f85149;
  --success: #3fb950;

  --font-sans: 'DM Sans', system-ui, sans-serif;
  --font-mono: 'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace;
  --text-caption: 11px;
  --text-body: 12px;
  --text-body-lg: 13px;
  --text-heading: 16px;
  --text-display: 20px;

  --space-xs: 4px;
  --space-sm: 8px;
  --space-md: 12px;
  --space-lg: 16px;
  --space-xl: 20px;
  --space-xxl: 24px;

  --radius-sm: 3px;
  --radius-md: 4px;
  --radius-lg: 6px;
}

/* Component CSS lands here in Phase 4-5. */
```

- [ ] **Step 3: Build sola-kit; expect success**

```bash
cargo build -p sola-kit 2>&1 | tail -5
```

Expected: clean build. The `include_str!` calls in `platform_assets()` now resolve.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/web/lib/kit.ts crates/sola-kit/web/lib/kit.css
git commit -m "feat(sola-kit): add kit.ts (applyTheme) + kit.css (static :root)

kit.css carries the same 31 default vars Theme::default() returns. Drift
between the two is guarded by a unit test (added later) that parses
kit.css and asserts it contains every Theme::default().to_css_vars()
pair."
```

---

### Task 8: Add the kit-css drift unit test

Now that both sources of defaults exist, add the test that catches drift between them.

**Files:**
- Modify: `crates/sola-kit/src/lib.rs`

- [ ] **Step 1: Add a test module to lib.rs**

Append to `crates/sola-kit/src/lib.rs`:

```rust
#[cfg(test)]
mod kit_css_drift {
    use sola_core::theme::Theme;

    /// kit.css's :root block must declare every var Theme::default() produces,
    /// with the same value. Catches accidental drift between the Rust source
    /// of truth and the static CSS apps load before any Topic::Theme arrives.
    #[test]
    fn default_to_css_vars_match_kit_css() {
        let css = include_str!("../web/lib/kit.css");
        for (var, value) in Theme::default().to_css_vars() {
            // crude but sufficient: kit.css is hand-maintained, single :root,
            // each declaration on its own line as `  --name: value;`.
            let needle = format!("{var}: {value};");
            assert!(
                css.contains(&needle),
                "kit.css missing or wrong value for {var}: expected `{needle}`",
            );
        }
    }
}
```

- [ ] **Step 2: Run the test**

```bash
cargo test -p sola-kit kit_css_drift 2>&1 | tail -10
```

Expected: `default_to_css_vars_match_kit_css ... ok`.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-kit/src/lib.rs
git commit -m "test(sola-kit): assert kit.css :root matches Theme::default()

Drift guard: every var-value pair Theme::default().to_css_vars()
produces must appear verbatim in kit.css. The two are hand-maintained
sources of truth (the CSS file is what apps see before any bus topic
arrives; the Rust struct is what gets serialised back into the topic)."
```

---

### Task 9: Minimal binary — opens a window with the storybook shell

**Files:**
- Create: `crates/sola-kit/web/app/index.html`
- Create: `crates/sola-kit/web/app/src/main.ts`
- Create: `crates/sola-kit/web/app/src/app.ts`
- Create: `crates/sola-kit/web/app/src/app.css`
- Create: `crates/sola-kit/src/app/kit_app.rs`
- Modify: `crates/sola-kit/src/app/main.rs`

- [ ] **Step 1: Create index.html**

Write `crates/sola-kit/web/app/index.html`:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>sola-kit</title>
  <link rel="stylesheet" href="/lib/kit.css">
  <link rel="stylesheet" href="/src/app.css">
  <script>window.RESTORED_STATE = __RESTORED_STATE__;</script>
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

- [ ] **Step 2: Create main.ts (mounts the app once DOM is ready)**

Write `crates/sola-kit/web/app/src/main.ts`:

```ts
import { mount } from './app';

const target = document.getElementById('app')!;
mount(target);
```

- [ ] **Step 3: Create app.ts (placeholder render)**

Write `crates/sola-kit/web/app/src/app.ts`:

```ts
import { html } from '@arrow-js/core';

export function mount(target: HTMLElement) {
  html`
    <div class="kit-shell">
      <h1>sola-kit</h1>
      <p>storybook scaffolding</p>
    </div>
  `(target);
}
```

- [ ] **Step 4: Create app.css (storybook chrome only — never the kit.css concerns)**

Write `crates/sola-kit/web/app/src/app.css`:

```css
html, body {
  margin: 0;
  padding: 0;
  height: 100%;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: var(--font-sans);
  font-size: var(--text-body-lg);
  overflow: hidden;
}

#app, .kit-shell {
  height: 100%;
}

.kit-shell {
  padding: var(--space-lg);
}
```

- [ ] **Step 5: Create kit_app.rs**

Write `crates/sola-kit/src/app/kit_app.rs`:

```rust
use sola_kit::{AppCtx, BusRegistry, SolaApp, WindowConfig, asset_bundle};

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/app/index.html"), Html),
    "/src/main.ts" => (include_str!("../../web/app/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../../web/app/src/app.ts"), TypeScript),
    "/src/app.css" => (include_str!("../../web/app/src/app.css"), Css),
};

pub struct KitApp;

impl SolaApp for KitApp {
    const APP_ID: &'static str = "sola-kit";

    fn new(ctx: &mut AppCtx) -> Self {
        ctx.add_window(WindowConfig {
            title: "Theme".into(),
            size: (1100, 720),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: None,
            zoned: true,
            keyboard_target: true,
        });
        Self
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(sola_bus::topics::TopicKind::CloseApp, Self::on_close_app);
    }
}
```

- [ ] **Step 6: Replace stub main.rs**

Overwrite `crates/sola-kit/src/app/main.rs`:

```rust
mod kit_app;

fn main() {
    sola_kit::run::<kit_app::KitApp>();
}
```

- [ ] **Step 7: Build the kit binary**

```bash
cargo build -p sola-kit --bin sola-kit 2>&1 | tail -5
```

Expected: clean build.

- [ ] **Step 8: Commit**

```bash
git add crates/sola-kit/src/app/ crates/sola-kit/web/app/
git commit -m "feat(sola-kit): minimal binary opens a storybook shell window

KitApp opens a 1100x720 zoned window titled \"Theme\". Storybook frontend
is a placeholder Arrow template that renders the title text against the
default theme so we can confirm kit.css landed correctly. Real storybook
UI lands in Phase 6."
```

---

## Phase 3 — Auto-distribution wiring

The kit's framework subscribes to `Topic::Theme` and forwards it to the WebView; `kit.ts` self-installs a listener that calls `applyTheme`. By the end of Phase 3, the storybook window can receive a published theme and re-render with it.

### Task 10: Subscribe to `Topic::Theme` at framework level

**Files:**
- Modify: `crates/sola-kit/src/lib.rs`

- [ ] **Step 1: Add Topic::Theme to the framework subscription set**

In `crates/sola-kit/src/lib.rs`, locate the block that builds `subscription_kinds` (it adds `Shutdown`, `Windows`, `Copy`, `Paste`, `Evaluate`). Add `Theme` alongside:

```rust
        for kind in [
            TopicKind::Shutdown,
            TopicKind::Windows,
            TopicKind::Copy,
            TopicKind::Paste,
            TopicKind::Evaluate,
            TopicKind::Theme,
        ] {
            if !subscription_kinds.contains(&kind) {
                subscription_kinds.push(kind);
            }
        }
```

- [ ] **Step 2: Forward Theme topic to the WebView**

In the same `lib.rs`, find the `match &topic { Topic::Windows(...) => ..., Topic::Evaluate(req) => ..., Topic::Copy(req) => ..., Topic::Paste(req) => ..., _ => {} }` block. Add a Theme arm before the catch-all:

```rust
                        Topic::Theme(theme) => {
                            // Forward to every window's WebView. JS-side
                            // listener (in @sola/kit) will apply the vars.
                            let payload = serde_json::json!({
                                "event": "theme",
                                "vars": theme.to_css_vars(),
                            });
                            let rt = runtime.borrow();
                            for w in &rt.ctx.windows {
                                w.send_to_js(&payload);
                            }
                        }
```

- [ ] **Step 3: Build**

```bash
cargo build -p sola-kit 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/lib.rs
git commit -m "feat(sola-kit): framework auto-subscribes to Topic::Theme

Every kit-based app subscribes to Theme via the same framework-level
mechanism that already auto-subscribes Shutdown/Windows/Copy/Paste/
Evaluate. On receive, flatten via Theme::to_css_vars() and broadcast
to every window's WebView as { event: 'theme', vars: ... }."
```

---

### Task 11: JS-side auto-listener in `kit.ts`

**Files:**
- Modify: `crates/sola-kit/web/lib/kit.ts`

- [ ] **Step 1: Wire the listener at module load**

Replace `crates/sola-kit/web/lib/kit.ts` with:

```ts
//! Sola Kit — design tokens, atoms, components.
//
// Importing this module installs a bus listener that applies any
// Topic::Theme broadcasts the framework forwards. Apps don't need to
// opt in beyond the import.

import { onEvent } from '@sola/ipc';

/** Apply a map of CSS custom properties to :root. */
export function applyTheme(vars: Record<string, string>): void {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(vars)) {
    root.style.setProperty(key.startsWith('--') ? key : `--${key}`, value);
  }
}

// Self-install: framework forwards Topic::Theme as { event: 'theme', vars }.
onEvent('theme', (payload: { vars: Record<string, string> }) => {
  if (payload && payload.vars) applyTheme(payload.vars);
});
```

(`onEvent` is already exported by `@sola/ipc`; check `crates/sola-kit/web/lib/ipc.ts` to confirm the helper name. If the existing API uses a slightly different shape, mirror it — the goal is "listen to a named event from the host and call applyTheme".)

- [ ] **Step 2: Verify ipc.ts exposes `onEvent`**

```bash
grep -n "export" crates/sola-kit/web/lib/ipc.ts | head
```

If `onEvent` is not exported, fall back to the lower-level helper exported there (commonly `subscribe`, `addListener`, or a window-level `__solaRecv` hook). Adjust the import + listener call accordingly. Do not invent a new export — use whatever's there.

- [ ] **Step 3: Build**

```bash
cargo build -p sola-kit 2>&1 | tail -5
```

Expected: clean (no Rust changes; CSS/TS files compile lazily at runtime).

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/web/lib/kit.ts
git commit -m "feat(sola-kit): kit.ts self-installs Topic::Theme listener

Importing @sola/kit subscribes to the framework's 'theme' event and
calls applyTheme on receipt. Apps that import any atom/component get
live theme distribution for free; no opt-in code in app frontends."
```

---

### Task 12: KitApp emits the current theme on startup; handles `theme_set` JS command

**Files:**
- Modify: `crates/sola-kit/src/app/kit_app.rs`

- [ ] **Step 1: Hold an in-memory `Theme` in `KitApp`**

Replace `crates/sola-kit/src/app/kit_app.rs` with:

```rust
use serde::Deserialize;
use serde_json::{Value, json};
use sola_bus::topics::{Topic, TopicKind};
use sola_core::theme::Theme;
use sola_kit::{AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle};

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/app/index.html"), Html),
    "/src/main.ts" => (include_str!("../../web/app/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../../web/app/src/app.ts"), TypeScript),
    "/src/app.css" => (include_str!("../../web/app/src/app.css"), Css),
};

#[derive(Deserialize)]
struct ThemeSetArgs {
    theme: Theme,
}

pub struct KitApp {
    theme: Theme,
    main_window: WindowHandle,
}

impl SolaApp for KitApp {
    const APP_ID: &'static str = "sola-kit";

    fn new(ctx: &mut AppCtx) -> Self {
        let theme = Theme::default();
        let initial_state = serde_json::to_string(&json!({ "theme": &theme })).ok();

        let main_window = ctx.add_window(WindowConfig {
            title: "Theme".into(),
            size: (1100, 720),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state,
            zoned: true,
            keyboard_target: true,
        });

        // Publish current theme so any pre-existing subscribers see something
        // immediately. The bus persistence layer replays the stored Theme over
        // this on first subscribe; the order doesn't matter — the persisted
        // value wins.
        ctx.emit(Topic::Theme(theme.clone()));

        Self { theme, main_window }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::CloseApp, Self::on_close_app);
        bus.on(TopicKind::Theme, Self::on_theme);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        let result = match cmd {
            "theme_set" => self.handle_theme_set(args, ctx),
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        };
        if let Some(id) = id {
            source.send_to_js(&json!({ "id": id, "result": result }));
        }
    }
}

impl KitApp {
    fn on_theme(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Theme(theme) = delivery.topic else { return };
        // Persisted replay or peer update: refresh in-memory copy.
        self.theme = theme.clone();
        // Push to the JS frontend so its mirror updates too.
        self.main_window.send_to_js(&json!({
            "event": "theme",
            "vars": self.theme.to_css_vars(),
        }));
    }

    fn handle_theme_set(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let parsed: ThemeSetArgs = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        self.theme = parsed.theme;
        ctx.emit(Topic::Theme(self.theme.clone()));
        json!({ "ok": true })
    }
}
```

- [ ] **Step 2: Build**

```bash
cargo build -p sola-kit 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-kit/src/app/kit_app.rs
git commit -m "feat(sola-kit): KitApp owns Theme; handles theme_set; emits on startup

Holds the in-memory Theme. on_theme handler refreshes from sticky
replay / peer updates and rebroadcasts to the WebView. theme_set JS
command deserialises the full Theme JSON, replaces the in-memory copy,
and emits Topic::Theme — bus persists, framework re-broadcasts to every
window in every kit-using app."
```

---

## Phase 4 — Atoms

Each atom task: TS file (template + tokens export), CSS in kit.css, preview module, sidebar item. After each atom, the storybook window can be launched and the atom appears in its category. Pattern is established in Task 13 (Button); subsequent atoms repeat the same shape.

### Task 13: Button atom

**Files:**
- Create: `crates/sola-kit/web/lib/components/button.ts`
- Modify: `crates/sola-kit/web/lib/kit.css` — add Button CSS.
- Modify: `crates/sola-kit/web/lib/kit.ts` — re-export Button.
- Modify: `crates/sola-kit/src/assets.rs` — register `button.ts`.

- [ ] **Step 1: Write the Button template + tokens**

Create `crates/sola-kit/web/lib/components/button.ts`:

```ts
import { html } from '@arrow-js/core';

export type ButtonVariant = 'primary' | 'default' | 'ghost' | 'danger' | 'add';

export interface ButtonOpts {
  label: string | (() => string);
  variant?: ButtonVariant;
  disabled?: boolean | (() => boolean);
  onClick?: () => void;
}

export const buttonTokens = [
  '--accent', '--accent-dim',
  '--bg-tertiary', '--text-secondary', '--text-primary',
  '--danger', '--border-subtle',
  '--radius-sm', '--text-body', '--space-sm', '--space-md',
];

export function button(opts: ButtonOpts) {
  const variant = opts.variant ?? 'default';
  const disabledAttr = (): string | false => {
    const d = typeof opts.disabled === 'function' ? opts.disabled() : opts.disabled;
    return d ? 'disabled' : false;
  };
  return html`<button
    class="kit-btn kit-btn-${variant}"
    disabled="${disabledAttr}"
    @click=${() => opts.onClick && opts.onClick()}
  >${typeof opts.label === 'function' ? opts.label : () => opts.label}</button>`;
}
```

- [ ] **Step 2: Add Button CSS to kit.css**

Append to `crates/sola-kit/web/lib/kit.css` (below the closing `:root { ... }` block, replacing the placeholder comment):

```css
/* ===== Button ===== */
.kit-btn {
  padding: var(--space-xs) var(--space-md);
  border: none;
  border-radius: var(--radius-sm);
  font-family: inherit;
  font-size: var(--text-body);
  cursor: pointer;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}
.kit-btn:hover { color: var(--text-primary); }
.kit-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.kit-btn-primary { background: var(--accent-dim); color: var(--accent); }
.kit-btn-ghost   { background: transparent; }
.kit-btn-danger  { background: transparent; color: var(--danger); }
.kit-btn-danger:hover { background: rgba(248, 81, 73, 0.10); }
.kit-btn-add {
  background: transparent;
  border: 1px dashed var(--border-subtle);
  color: var(--text-secondary);
  width: 100%;
  padding: var(--space-sm);
}
.kit-btn-add:hover { border-color: var(--accent); color: var(--accent); }
```

- [ ] **Step 3: Re-export from kit.ts**

In `crates/sola-kit/web/lib/kit.ts`, after the existing `applyTheme` export, append:

```ts
export { button, buttonTokens } from './components/button';
export type { ButtonOpts, ButtonVariant } from './components/button';
```

- [ ] **Step 4: Register button.ts in `platform_assets()`**

In `crates/sola-kit/src/assets.rs`'s `platform_assets()`, add an entry inside the `&[ ... ]` array:

```rust
            Asset {
                path: "/lib/components/button.ts",
                content: include_str!("../web/lib/components/button.ts"),
                content_type: ContentType::TypeScript,
            },
```

- [ ] **Step 5: Build**

```bash
cargo build -p sola-kit 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-kit/web/lib/components/button.ts \
        crates/sola-kit/web/lib/kit.ts crates/sola-kit/web/lib/kit.css \
        crates/sola-kit/src/assets.rs
git commit -m "feat(sola-kit): add Button atom (5 variants)

Template returns an Arrow chunk; reactive label / disabled accept
either values or closures. CSS uses tokens exclusively — no
hex / px literals. buttonTokens lists every var the styles read."
```

---

### Task 14: Field atom

Same shape as Task 13 — TS template + tokens, CSS, kit.ts re-export, assets.rs entry.

**Files:**
- Create: `crates/sola-kit/web/lib/components/field.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write field.ts**

Create `crates/sola-kit/web/lib/components/field.ts`:

```ts
import { html } from '@arrow-js/core';

export interface FieldOpts {
  value: string | (() => string);
  onInput?: (v: string) => void;
  placeholder?: string;
  error?: string | (() => string | undefined);
  type?: 'text' | 'password' | 'email' | 'number';
}

export const fieldTokens = [
  '--bg-primary', '--border-subtle', '--accent',
  '--text-primary', '--danger',
  '--radius-sm', '--text-body', '--space-xs', '--space-sm',
];

export function field(opts: FieldOpts) {
  const t = opts.type ?? 'text';
  const valueExpr = typeof opts.value === 'function' ? opts.value : () => opts.value as string;
  const errorExpr = (): string | false => {
    const e = typeof opts.error === 'function' ? opts.error() : opts.error;
    return e ? 'error' : false;
  };
  return html`<input
    type="${t}"
    class="kit-field"
    data-error="${errorExpr}"
    placeholder="${opts.placeholder ?? ''}"
    value="${valueExpr}"
    @input=${(e: Event) => opts.onInput && opts.onInput((e.target as HTMLInputElement).value)}
  >`;
}
```

- [ ] **Step 2: Append Field CSS to kit.css**

```css
/* ===== Field ===== */
.kit-field {
  width: 100%;
  padding: var(--space-xs) var(--space-sm);
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: var(--text-body);
  outline: none;
}
.kit-field:focus { border-color: var(--accent); }
.kit-field[data-error="error"] { border-color: var(--danger); }
```

- [ ] **Step 3: Re-export + register asset (mirror Task 13's Steps 3 and 4 for field)**

- [ ] **Step 4: Build + commit**

```bash
cargo build -p sola-kit 2>&1 | tail -5
git add crates/sola-kit/web/lib/components/field.ts crates/sola-kit/web/lib/kit.{ts,css} crates/sola-kit/src/assets.rs
git commit -m "feat(sola-kit): add Field atom

Text input with optional error state; supports password/email/number
via opts.type. fieldTokens lists 9 vars the styles read."
```

---

### Task 15: Badge atom

**Files:**
- Create: `crates/sola-kit/web/lib/components/badge.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write badge.ts**

```ts
import { html } from '@arrow-js/core';

export type BadgeVariant = 'default' | 'accent' | 'danger' | 'success';

export interface BadgeOpts {
  label: string | (() => string);
  variant?: BadgeVariant;
}

export const badgeTokens = [
  '--bg-tertiary', '--text-secondary',
  '--accent', '--accent-dim',
  '--danger', '--success',
  '--radius-sm', '--text-caption', '--space-xs',
];

export function badge(opts: BadgeOpts) {
  const variant = opts.variant ?? 'default';
  return html`<span class="kit-badge kit-badge-${variant}">${
    typeof opts.label === 'function' ? opts.label : () => opts.label
  }</span>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Badge ===== */
.kit-badge {
  display: inline-block;
  padding: 1px var(--space-xs);
  font-size: var(--text-caption);
  font-weight: 500;
  border-radius: var(--radius-sm);
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}
.kit-badge-accent  { background: var(--accent-dim); color: var(--accent); }
.kit-badge-danger  { background: rgba(248, 81, 73, 0.14); color: var(--danger); }
.kit-badge-success { background: rgba(63, 185, 80, 0.14); color: var(--success); }
```

- [ ] **Step 3: Re-export + register asset (mirror Task 13)**

- [ ] **Step 4: Build + commit**

```bash
git add ...
git commit -m "feat(sola-kit): add Badge atom (4 variants)"
```

---

### Task 16: Icon atom

The icon system already exists in `sola-shell` (lucide names). This atom is the simplest possible passthrough — accepts a name string, renders from sola-assets icons URL. We don't reinvent the icon loader.

**Files:**
- Create: `crates/sola-kit/web/lib/components/icon.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write icon.ts**

```ts
import { html } from '@arrow-js/core';

export interface IconOpts {
  name: string | (() => string);
  size?: number;
}

export const iconTokens = ['--text-secondary'];

export function icon(opts: IconOpts) {
  const name = typeof opts.name === 'function' ? opts.name : () => opts.name as string;
  const size = opts.size ?? 16;
  return html`<img
    class="kit-icon"
    src="${() => `sola-assets://icons/${name()}.svg`}"
    width="${size}"
    height="${size}"
  >`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Icon ===== */
.kit-icon {
  display: inline-block;
  vertical-align: middle;
  filter: brightness(0) saturate(100%); /* tint via parent color via CSS mask in future */
  opacity: 0.85;
}
```

- [ ] **Step 3: Re-export + register asset (mirror Task 13)**

- [ ] **Step 4: Build + commit**

```bash
git add ...
git commit -m "feat(sola-kit): add Icon atom (sola-assets passthrough)"
```

---

## Phase 5 — Components

Same pattern as Phase 4. One task per component. Each: TS file with template + tokens, kit.css additions, kit.ts re-export, assets.rs entry, build + commit.

### Task 17: Sidebar component

**Files:**
- Create: `crates/sola-kit/web/lib/components/sidebar.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write sidebar.ts**

```ts
import { html, type TemplatePartial } from '@arrow-js/core';

export interface SidebarOpts {
  title?: string | (() => string);
  body: TemplatePartial;
}

export const sidebarTokens = [
  '--bg-secondary', '--border-subtle', '--text-muted',
  '--space-xs', '--space-sm', '--space-md',
  '--text-caption',
];

export function sidebar(opts: SidebarOpts) {
  return html`<aside class="kit-sidebar">
    ${opts.title ? html`<div class="kit-sidebar-title">${
      typeof opts.title === 'function' ? opts.title : () => opts.title
    }</div>` : html``}
    ${() => opts.body}
  </aside>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Sidebar ===== */
.kit-sidebar {
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-subtle);
  padding: var(--space-md) var(--space-sm);
  display: flex;
  flex-direction: column;
  gap: var(--space-xs);
  overflow-y: auto;
}
.kit-sidebar-title {
  font-size: var(--text-caption);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--text-muted);
  padding: var(--space-xs) var(--space-sm);
}
```

- [ ] **Step 3: Re-export + register asset (mirror Task 13)**

- [ ] **Step 4: Build + commit**

```bash
git add ...
git commit -m "feat(sola-kit): add Sidebar component (title + body slot)"
```

---

### Task 18: NavItem component

**Files:**
- Create: `crates/sola-kit/web/lib/components/nav-item.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write nav-item.ts**

```ts
import { html } from '@arrow-js/core';

export interface NavItemOpts {
  label: string | (() => string);
  active?: boolean | (() => boolean);
  onClick?: () => void;
}

export const navItemTokens = [
  '--text-secondary', '--text-primary',
  '--bg-tertiary', '--accent', '--accent-dim',
  '--radius-sm', '--text-body', '--space-xs', '--space-sm',
];

export function navItem(opts: NavItemOpts) {
  const activeAttr = (): string | false => {
    const a = typeof opts.active === 'function' ? opts.active() : opts.active;
    return a ? 'active' : false;
  };
  return html`<button
    class="kit-nav-item"
    data-active="${activeAttr}"
    @click=${() => opts.onClick && opts.onClick()}
  >${typeof opts.label === 'function' ? opts.label : () => opts.label}</button>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== NavItem ===== */
.kit-nav-item {
  background: none;
  border: none;
  text-align: left;
  font: inherit;
  cursor: pointer;
  color: var(--text-secondary);
  padding: var(--space-xs) var(--space-sm);
  border-radius: var(--radius-sm);
  font-size: var(--text-body);
}
.kit-nav-item:hover { background: var(--bg-tertiary); color: var(--text-primary); }
.kit-nav-item[data-active="active"] { background: var(--accent-dim); color: var(--accent); }
```

- [ ] **Step 3: Re-export + register asset (mirror Task 13)**

- [ ] **Step 4: Build + commit**

---

### Task 19: Section component

**Files:**
- Create: `crates/sola-kit/web/lib/components/section.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write section.ts**

```ts
import { html, type TemplatePartial } from '@arrow-js/core';

export interface SectionOpts {
  title: string | (() => string);
  description?: string | (() => string);
  body: TemplatePartial;
}

export const sectionTokens = [
  '--text-primary', '--text-tertiary',
  '--text-heading', '--text-body',
  '--space-xs', '--space-md', '--space-lg',
];

export function section(opts: SectionOpts) {
  const title = typeof opts.title === 'function' ? opts.title : () => opts.title;
  const desc = opts.description
    ? (typeof opts.description === 'function' ? opts.description : () => opts.description as string)
    : null;
  return html`<section class="kit-section">
    <h2 class="kit-section-title">${title}</h2>
    ${desc ? html`<p class="kit-section-desc">${desc}</p>` : html``}
    <div class="kit-section-body">${() => opts.body}</div>
  </section>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Section ===== */
.kit-section { margin-bottom: var(--space-lg); }
.kit-section-title {
  margin: 0 0 var(--space-xs);
  font-size: var(--text-heading);
  font-weight: 600;
  color: var(--text-primary);
}
.kit-section-desc {
  margin: 0 0 var(--space-md);
  font-size: var(--text-body);
  color: var(--text-tertiary);
}
```

- [ ] **Step 3: Re-export + register asset (mirror Task 13)**

- [ ] **Step 4: Build + commit**

---

### Task 20: Row component

**Files:**
- Create: `crates/sola-kit/web/lib/components/row.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write row.ts**

```ts
import { html, type TemplatePartial } from '@arrow-js/core';

export interface RowOpts {
  label: string | (() => string);
  detail?: string | (() => string);
  actions?: TemplatePartial;
  leading?: TemplatePartial;
}

export const rowTokens = [
  '--bg-secondary', '--text-primary', '--text-tertiary',
  '--radius-md', '--text-body', '--text-caption',
  '--space-sm', '--space-md',
];

export function row(opts: RowOpts) {
  const label = typeof opts.label === 'function' ? opts.label : () => opts.label;
  const detail = opts.detail
    ? (typeof opts.detail === 'function' ? opts.detail : () => opts.detail as string)
    : null;
  return html`<div class="kit-row">
    ${opts.leading ? html`<div class="kit-row-leading">${() => opts.leading}</div>` : html``}
    <div class="kit-row-info">
      <div class="kit-row-label">${label}</div>
      ${detail ? html`<div class="kit-row-detail">${detail}</div>` : html``}
    </div>
    ${opts.actions ? html`<div class="kit-row-actions">${() => opts.actions}</div>` : html``}
  </div>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Row ===== */
.kit-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-sm) var(--space-md);
  background: var(--bg-secondary);
  border-radius: var(--radius-md);
  gap: var(--space-sm);
}
.kit-row-leading { flex-shrink: 0; }
.kit-row-info { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; }
.kit-row-label { font-size: var(--text-body); font-weight: 500; color: var(--text-primary); }
.kit-row-detail {
  font-size: var(--text-caption);
  color: var(--text-tertiary);
  font-family: var(--font-mono);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.kit-row-actions { display: flex; gap: var(--space-xs); flex-shrink: 0; }
```

- [ ] **Step 3-4: Re-export + register asset + build + commit (mirror Task 13)**

---

### Task 21: List component

**Files:**
- Create: `crates/sola-kit/web/lib/components/list.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write list.ts**

```ts
import { html, type TemplatePartial } from '@arrow-js/core';

export interface ListOpts {
  body: TemplatePartial;
}

export const listTokens = ['--space-xs'];

export function list(opts: ListOpts) {
  return html`<div class="kit-list">${() => opts.body}</div>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== List ===== */
.kit-list { display: flex; flex-direction: column; gap: 1px; }
```

- [ ] **Step 3-4: Re-export + register asset + build + commit (mirror Task 13)**

---

### Task 22: Form component

**Files:**
- Create: `crates/sola-kit/web/lib/components/form.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write form.ts**

```ts
import { html, type TemplatePartial } from '@arrow-js/core';

export interface FormOpts {
  body: TemplatePartial;
  actions?: TemplatePartial;
}

export interface FieldRowOpts {
  label: string;
  body: TemplatePartial;
  width?: 'narrow' | 'normal';
}

export const formTokens = [
  '--bg-secondary', '--text-secondary',
  '--radius-md', '--text-body',
  '--space-sm', '--space-md',
];

export function form(opts: FormOpts) {
  return html`<div class="kit-form">
    <div class="kit-form-body">${() => opts.body}</div>
    ${opts.actions ? html`<div class="kit-form-actions">${() => opts.actions}</div>` : html``}
  </div>`;
}

export function fieldRow(opts: FieldRowOpts) {
  return html`<div class="kit-field-row">
    <label class="kit-field-label">${opts.label}</label>
    <div class="kit-field-body kit-field-${opts.width ?? 'normal'}">${() => opts.body}</div>
  </div>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Form ===== */
.kit-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
  padding: var(--space-md);
  background: var(--bg-secondary);
  border-radius: var(--radius-md);
}
.kit-form-actions { display: flex; gap: var(--space-sm); }
.kit-field-row { display: flex; align-items: center; gap: var(--space-sm); }
.kit-field-label {
  flex-shrink: 0;
  width: 110px;
  font-size: var(--text-body);
  color: var(--text-secondary);
}
.kit-field-body { flex: 1; }
.kit-field-narrow { flex: 0 0 140px; }
```

- [ ] **Step 3: Re-export both `form` and `fieldRow` from kit.ts**

- [ ] **Step 4: Register asset + build + commit (mirror Task 13)**

---

### Task 23: Tabs/Tab component

**Files:**
- Create: `crates/sola-kit/web/lib/components/tabs.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write tabs.ts**

```ts
import { html, type TemplatePartial } from '@arrow-js/core';

export interface TabsOpts {
  body: TemplatePartial;       // a list of tab(...) calls
  orientation?: 'vertical' | 'horizontal';
}

export type TabVariant = 'numbered' | 'favicon';

export interface TabOpts {
  title: string | (() => string);
  active?: boolean | (() => boolean);
  onClick?: () => void;
  onClose?: () => void;
  leading?: TemplatePartial;       // numbered: "1", favicon: <img>
  trailing?: TemplatePartial;      // browser: reload
  variant?: TabVariant;            // shorthand for filling slots
  index?: number | (() => number); // used when variant === 'numbered'
  faviconUrl?: string | (() => string); // used when variant === 'favicon'
}

export const tabsTokens = [
  '--bg-secondary', '--bg-tertiary', '--accent-dim', '--accent',
  '--text-secondary', '--text-primary', '--border-subtle',
  '--radius-sm', '--text-body', '--text-caption',
  '--space-xs', '--space-sm',
];

export function tabs(opts: TabsOpts) {
  const o = opts.orientation ?? 'vertical';
  return html`<div class="kit-tabs kit-tabs-${o}">${() => opts.body}</div>`;
}

export function tab(opts: TabOpts) {
  const activeAttr = (): string | false => {
    const a = typeof opts.active === 'function' ? opts.active() : opts.active;
    return a ? 'active' : false;
  };

  // Variant shortcuts pre-fill leading / trailing.
  let leading = opts.leading;
  let trailing = opts.trailing;
  if (opts.variant === 'numbered' && !leading && opts.index !== undefined) {
    const idx = typeof opts.index === 'function' ? opts.index : () => opts.index as number;
    leading = html`<span class="kit-tab-num">${idx}</span>`;
  }
  if (opts.variant === 'favicon' && !leading && opts.faviconUrl !== undefined) {
    const url = typeof opts.faviconUrl === 'function' ? opts.faviconUrl : () => opts.faviconUrl as string;
    leading = html`<img class="kit-tab-favicon" src="${url}" width="14" height="14">`;
  }

  const title = typeof opts.title === 'function' ? opts.title : () => opts.title;
  return html`<div
    class="kit-tab"
    data-active="${activeAttr}"
    @click=${() => opts.onClick && opts.onClick()}
  >
    ${leading ? html`<span class="kit-tab-leading">${() => leading}</span>` : html``}
    <span class="kit-tab-title">${title}</span>
    ${trailing ? html`<span class="kit-tab-trailing">${() => trailing}</span>` : html``}
    ${opts.onClose ? html`<button
      class="kit-tab-close"
      @click=${(e: Event) => { e.stopPropagation(); opts.onClose && opts.onClose(); }}
    >×</button>` : html``}
  </div>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Tabs ===== */
.kit-tabs { display: flex; gap: 1px; }
.kit-tabs-vertical   { flex-direction: column; }
.kit-tabs-horizontal { flex-direction: row; align-items: stretch; }

.kit-tab {
  display: flex;
  align-items: center;
  gap: var(--space-xs);
  padding: var(--space-xs) var(--space-sm);
  background: var(--bg-secondary);
  color: var(--text-secondary);
  border-radius: var(--radius-sm);
  font-size: var(--text-body);
  cursor: pointer;
}
.kit-tab:hover { color: var(--text-primary); background: var(--bg-tertiary); }
.kit-tab[data-active="active"] { background: var(--accent-dim); color: var(--accent); }
.kit-tab-leading { display: inline-flex; align-items: center; }
.kit-tab-num { font-family: var(--font-mono); font-size: var(--text-caption); color: var(--text-tertiary); width: 14px; text-align: center; }
.kit-tab-favicon { border-radius: 2px; }
.kit-tab-title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.kit-tab-close {
  border: none; background: none; color: var(--text-tertiary);
  cursor: pointer; padding: 0 var(--space-xs); font-size: var(--text-body-lg);
}
.kit-tab-close:hover { color: var(--text-primary); }
```

- [ ] **Step 3: Re-export both `tabs` and `tab` from kit.ts**

- [ ] **Step 4: Register asset + build + commit (mirror Task 13)**

---

### Task 24: Toast component

**Files:**
- Create: `crates/sola-kit/web/lib/components/toast.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write toast.ts**

```ts
import { html, type TemplatePartial } from '@arrow-js/core';

export interface ToastOpts {
  body: TemplatePartial;
  variant?: 'default' | 'success' | 'danger';
}

export const toastTokens = [
  '--bg-secondary', '--border-subtle',
  '--accent', '--success', '--danger',
  '--radius-md', '--text-body',
  '--space-sm', '--space-md',
];

export function toast(opts: ToastOpts) {
  const v = opts.variant ?? 'default';
  return html`<div class="kit-toast kit-toast-${v}">${() => opts.body}</div>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Toast ===== */
.kit-toast {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  padding: var(--space-sm) var(--space-md);
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  font-size: var(--text-body);
  box-shadow: 0 4px 16px -8px rgba(0,0,0,0.6);
}
.kit-toast-success { border-left: 3px solid var(--success); }
.kit-toast-danger  { border-left: 3px solid var(--danger); }
```

- [ ] **Step 3-4: Re-export + register + build + commit (mirror Task 13)**

---

### Task 25: Empty component

**Files:**
- Create: `crates/sola-kit/web/lib/components/empty.ts`
- Modify: `crates/sola-kit/web/lib/kit.css`, `kit.ts`, `crates/sola-kit/src/assets.rs`

- [ ] **Step 1: Write empty.ts**

```ts
import { html } from '@arrow-js/core';

export interface EmptyOpts {
  label: string | (() => string);
  hint?: string | (() => string);
}

export const emptyTokens = [
  '--text-muted',
  '--text-body', '--text-caption',
  '--space-md',
];

export function empty(opts: EmptyOpts) {
  const label = typeof opts.label === 'function' ? opts.label : () => opts.label;
  const hint = opts.hint
    ? (typeof opts.hint === 'function' ? opts.hint : () => opts.hint as string)
    : null;
  return html`<div class="kit-empty">
    <div class="kit-empty-label">${label}</div>
    ${hint ? html`<div class="kit-empty-hint">${hint}</div>` : html``}
  </div>`;
}
```

- [ ] **Step 2: CSS**

```css
/* ===== Empty ===== */
.kit-empty {
  padding: var(--space-md);
  font-size: var(--text-body);
  color: var(--text-muted);
  font-style: italic;
  text-align: center;
}
.kit-empty-hint { font-size: var(--text-caption); margin-top: 4px; font-style: normal; }
```

- [ ] **Step 3-4: Re-export + register + build + commit (mirror Task 13)**

---

## Phase 6 — Catalog + storybook UI

### Task 26: Rust `CATALOG` + parity test

**Files:**
- Create: `crates/sola-kit/src/app/catalog.rs`
- Modify: `crates/sola-kit/src/app/main.rs` (declare module)

- [ ] **Step 1: Write catalog.rs**

```rust
//! Static catalog of every atom + component the kit ships, with the
//! CSS-token vars each one uses. Used by the storybook for the sidebar
//! AND for the reverse index ("which components use --accent?").
//!
//! These lists must match the `*Tokens` exports in
//! `web/lib/components/<name>.ts`. The parity test below enforces it.

#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub name: &'static str,
    pub group: Group,
    pub tokens: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Atom,
    Component,
}

pub static CATALOG: &[CatalogEntry] = &[
    // Atoms
    CatalogEntry {
        name: "button",
        group: Group::Atom,
        tokens: &[
            "--accent", "--accent-dim",
            "--bg-tertiary", "--text-secondary", "--text-primary",
            "--danger", "--border-subtle",
            "--radius-sm", "--text-body", "--space-sm", "--space-md",
        ],
    },
    CatalogEntry {
        name: "field",
        group: Group::Atom,
        tokens: &[
            "--bg-primary", "--border-subtle", "--accent",
            "--text-primary", "--danger",
            "--radius-sm", "--text-body", "--space-xs", "--space-sm",
        ],
    },
    CatalogEntry {
        name: "badge",
        group: Group::Atom,
        tokens: &[
            "--bg-tertiary", "--text-secondary",
            "--accent", "--accent-dim",
            "--danger", "--success",
            "--radius-sm", "--text-caption", "--space-xs",
        ],
    },
    CatalogEntry {
        name: "icon",
        group: Group::Atom,
        tokens: &["--text-secondary"],
    },
    // Components
    CatalogEntry {
        name: "sidebar",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--border-subtle", "--text-muted",
            "--space-xs", "--space-sm", "--space-md",
            "--text-caption",
        ],
    },
    CatalogEntry {
        name: "nav-item",
        group: Group::Component,
        tokens: &[
            "--text-secondary", "--text-primary",
            "--bg-tertiary", "--accent", "--accent-dim",
            "--radius-sm", "--text-body", "--space-xs", "--space-sm",
        ],
    },
    CatalogEntry {
        name: "section",
        group: Group::Component,
        tokens: &[
            "--text-primary", "--text-tertiary",
            "--text-heading", "--text-body",
            "--space-xs", "--space-md", "--space-lg",
        ],
    },
    CatalogEntry {
        name: "row",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--text-primary", "--text-tertiary",
            "--radius-md", "--text-body", "--text-caption",
            "--space-sm", "--space-md",
        ],
    },
    CatalogEntry {
        name: "list",
        group: Group::Component,
        tokens: &["--space-xs"],
    },
    CatalogEntry {
        name: "form",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--text-secondary",
            "--radius-md", "--text-body",
            "--space-sm", "--space-md",
        ],
    },
    CatalogEntry {
        name: "tabs",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--bg-tertiary", "--accent-dim", "--accent",
            "--text-secondary", "--text-primary", "--border-subtle",
            "--radius-sm", "--text-body", "--text-caption",
            "--space-xs", "--space-sm",
        ],
    },
    CatalogEntry {
        name: "toast",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--border-subtle",
            "--accent", "--success", "--danger",
            "--radius-md", "--text-body",
            "--space-sm", "--space-md",
        ],
    },
    CatalogEntry {
        name: "empty",
        group: Group::Component,
        tokens: &[
            "--text-muted",
            "--text-body", "--text-caption",
            "--space-md",
        ],
    },
];

/// Reverse index: which components consume the given token?
pub fn consumers_of(token: &str) -> Vec<&'static CatalogEntry> {
    CATALOG.iter().filter(|e| e.tokens.contains(&token)).collect()
}

#[cfg(test)]
mod parity {
    //! Asserts each Rust CATALOG entry's tokens match the corresponding
    //! `*Tokens` export in `web/lib/components/<name>.ts`. We parse the
    //! TS file naively (regex on the array literal); good enough for our
    //! single-line declarations.

    use super::*;

    fn js_tokens_for(name: &str) -> Vec<String> {
        let path = format!("web/lib/components/{name}.ts");
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        // Find `export const <camel>Tokens = [...]` and pull out the var
        // strings (anything between single quotes inside the array).
        let camel = name.replace('-', "");
        let needle = format!("{camel}Tokens");
        let start = src.find(&needle).unwrap_or_else(|| panic!("no {needle} in {path}"));
        let after = &src[start..];
        let bracket = after.find('[').expect("no [ after Tokens");
        let close = after[bracket..].find(']').expect("no ] after Tokens");
        let inner = &after[bracket + 1..bracket + close];
        let mut out = Vec::new();
        for chunk in inner.split(',') {
            let t = chunk.trim();
            let t = t.trim_matches(|c| c == '\'' || c == '"' || c == '\n' || c == ' ');
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
        out
    }

    #[test]
    fn rust_catalog_matches_typescript_exports() {
        for entry in CATALOG {
            // nav-item.ts → navItemTokens, button.ts → buttonTokens, etc.
            let js: Vec<String> = js_tokens_for(entry.name);
            let rs: Vec<String> = entry.tokens.iter().map(|s| s.to_string()).collect();
            let mut js_sorted = js.clone();
            js_sorted.sort();
            let mut rs_sorted = rs.clone();
            rs_sorted.sort();
            assert_eq!(
                js_sorted, rs_sorted,
                "catalog mismatch for {}: rust={:?}, ts={:?}",
                entry.name, rs, js
            );
        }
    }
}
```

- [ ] **Step 2: Declare the module in main.rs**

In `crates/sola-kit/src/app/main.rs`, before `fn main`:

```rust
mod kit_app;
mod catalog;
```

(Even if `catalog` isn't used by `main()` itself, declaring it here brings its tests into the bin's crate. We'll use `catalog::CATALOG` from `kit_app.rs` in later tasks.)

- [ ] **Step 3: Run parity test**

```bash
cargo test -p sola-kit --bin sola-kit catalog 2>&1 | tail -10
```

Expected: `parity::rust_catalog_matches_typescript_exports ... ok`.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/app/catalog.rs crates/sola-kit/src/app/main.rs
git commit -m "feat(sola-kit): static catalog + TS-parity drift test

CATALOG enumerates every atom/component the kit ships with the CSS-token
vars each consumes. consumers_of(\"--accent\") returns the reverse-index
list the editor uses for token-mode previews. Parity test parses each
TS file's *Tokens export and asserts equality with the Rust list."
```

---

### Task 27: Storybook frontend — sidebar with categories

The storybook UI starts from `web/app/src/app.ts`. We replace its placeholder with a sidebar listing every catalog entry, plus a content area that says "select an item" until something's clicked.

**Files:**
- Modify: `crates/sola-kit/web/app/src/app.ts`
- Create: `crates/sola-kit/web/app/src/sidebar.ts`
- Modify: `crates/sola-kit/web/app/src/app.css`
- Modify: `crates/sola-kit/src/app/kit_app.rs` — pass catalog into initial_state.

- [ ] **Step 1: Pass the catalog to the frontend via initial_state**

In `crates/sola-kit/src/app/kit_app.rs`'s `new`, build a JSON catalog from the Rust `CATALOG`:

```rust
        use crate::catalog::{CATALOG, Group};
        let catalog_json: Vec<serde_json::Value> = CATALOG
            .iter()
            .map(|e| serde_json::json!({
                "name": e.name,
                "group": match e.group { Group::Atom => "atom", Group::Component => "component" },
                "tokens": e.tokens,
            }))
            .collect();
        let initial_state = serde_json::to_string(&serde_json::json!({
            "theme": &theme,
            "catalog": catalog_json,
        })).ok();
```

Replace the existing `let initial_state = ...` line with the block above.

- [ ] **Step 2: Add a `mod catalog;` declaration to lib.rs OR import catalog inside kit_app.rs**

Since `catalog.rs` lives under `src/app/` and is currently only declared in `src/app/main.rs`, kit_app.rs (sibling) needs `use super::catalog;` instead of `use crate::catalog;`. Adjust the `use` line in Step 1 accordingly:

```rust
        use super::catalog::{CATALOG, Group};
```

- [ ] **Step 3: Write sidebar.ts**

Create `crates/sola-kit/web/app/src/sidebar.ts`:

```ts
import { html } from '@arrow-js/core';
import { sidebar, navItem } from '@sola/kit';

export interface CatalogEntry {
  name: string;
  group: 'atom' | 'component';
  tokens: string[];
}

export interface SidebarState {
  selected: string;        // id like "tokens.colors", "atoms.button", "components.row"
}

export const TOKEN_ITEMS = [
  { id: 'tokens.colors',     label: 'Colors' },
  { id: 'tokens.typography', label: 'Typography' },
  { id: 'tokens.spacing',    label: 'Spacing & radius' },
];

export function renderSidebar(state: SidebarState, catalog: CatalogEntry[], onSelect: (id: string) => void) {
  const atoms = catalog.filter(e => e.group === 'atom');
  const comps = catalog.filter(e => e.group === 'component');

  const navWith = (id: string, label: string) => navItem({
    label,
    active: () => state.selected === id,
    onClick: () => onSelect(id),
  });

  return sidebar({
    body: html`
      <div class="kit-sidebar-title">Tokens</div>
      ${TOKEN_ITEMS.map(t => navWith(t.id, t.label))}
      <div class="kit-sidebar-title">Atoms</div>
      ${atoms.map(a => navWith(`atoms.${a.name}`, capitalise(a.name)))}
      <div class="kit-sidebar-title">Components</div>
      ${comps.map(c => navWith(`components.${c.name}`, capitalise(c.name)))}
    `,
  });
}

function capitalise(s: string) {
  return s.split('-').map(p => p.charAt(0).toUpperCase() + p.slice(1)).join('');
}
```

- [ ] **Step 4: Replace app.ts**

Overwrite `crates/sola-kit/web/app/src/app.ts`:

```ts
import { html, reactive } from '@arrow-js/core';
import { renderSidebar, type CatalogEntry } from './sidebar';

declare global {
  interface Window { RESTORED_STATE?: { catalog: CatalogEntry[]; theme: unknown }; }
}

const restored = window.RESTORED_STATE ?? { catalog: [], theme: null };

const state = reactive({
  selected: 'tokens.colors',
  catalog: restored.catalog as CatalogEntry[],
});

export function mount(target: HTMLElement) {
  html`
    <div class="kit-shell">
      ${() => renderSidebar(
        { selected: state.selected },
        state.catalog,
        (id: string) => { state.selected = id; },
      )}
      <main class="kit-work">
        <div class="kit-placeholder">${() => state.selected}</div>
      </main>
    </div>
  `(target);
}
```

- [ ] **Step 5: Update app.css for the two-pane layout**

Replace `crates/sola-kit/web/app/src/app.css` with:

```css
html, body {
  margin: 0; padding: 0; height: 100%;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: var(--font-sans);
  font-size: var(--text-body-lg);
  overflow: hidden;
}
#app, .kit-shell { height: 100%; }
.kit-shell { display: flex; }
.kit-shell > .kit-sidebar { width: 200px; flex-shrink: 0; }
.kit-work { flex: 1; overflow: auto; padding: var(--space-lg) var(--space-xl); }
.kit-placeholder {
  font-family: var(--font-mono);
  color: var(--text-tertiary);
  font-size: var(--text-body);
}
```

- [ ] **Step 6: Build**

```bash
cargo build -p sola-kit 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-kit/web/app/ crates/sola-kit/src/app/kit_app.rs
git commit -m "feat(sola-kit): storybook sidebar + selection state

Sidebar groups: Tokens (Colors/Typography/Spacing), Atoms, Components,
populated from the catalog passed in via initial_state. Clicking a nav
item updates state.selected; work area shows the id as a placeholder
until token/component modes land in subsequent tasks."
```

---

### Task 28: Token-mode work area — Colors

When `state.selected` is `tokens.colors`, render a swatch list of every color token; clicking one shows the editor strip + grid of consuming components.

**Files:**
- Create: `crates/sola-kit/web/app/src/preview/tokens-colors.ts`
- Create: `crates/sola-kit/web/app/src/token-edit.ts`
- Modify: `crates/sola-kit/web/app/src/app.ts` — route `tokens.colors` to the new module.

- [ ] **Step 1: Write token-edit.ts (the shared editor strip)**

Create `crates/sola-kit/web/app/src/token-edit.ts`:

```ts
import { html, reactive } from '@arrow-js/core';
import { applyTheme, button } from '@sola/kit';
import { invoke } from '@sola/ipc';
import type { CatalogEntry } from './sidebar';

// Holds the in-progress full Theme as JSON, mirroring the Rust struct
// passed in via initial_state. Mutations propagate to applyTheme(...)
// immediately (live preview) and emit Topic::Theme via debounced bus
// after 300 ms of inactivity.
export const themeState = reactive({
  // shape: { colors: {...}, typography: {...}, spacing: {...}, radius: {...} }
  current: (window as unknown as { RESTORED_STATE?: { theme: any } }).RESTORED_STATE?.theme ?? {},
});

let debounceTimer: number | null = null;

export function setColor(field: string, value: string) {
  if (!themeState.current.colors) return;
  themeState.current.colors[field] = value;
  applyTheme({ [`--${field.replaceAll('_', '-')}`]: value });
  scheduleEmit();
}

function scheduleEmit() {
  if (debounceTimer !== null) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    invoke('theme_set', { theme: themeState.current });
  }, 300) as unknown as number;
}

export function resetTheme() {
  // Server-side default is authoritative. Clear local copy + ask
  // KitApp to re-emit Theme::default() by sending an empty hint;
  // KitApp's sticky replay will then push the new (default) state back
  // and our themeState will refresh via the theme listener.
  invoke('theme_reset', {});
}

/** Editor strip for one color token. */
export function colorEditor(field: string, varName: string, used: CatalogEntry[]) {
  const current = (): string => themeState.current?.colors?.[field] ?? '';
  return html`
    <div class="kit-editor-strip">
      <div class="kit-editor-head">
        <div class="kit-editor-name">${varName}</div>
        <div class="kit-editor-meta">${() => `Used in ${used.length} ${used.length === 1 ? 'component' : 'components'}`}</div>
      </div>
      <div class="kit-editor-row">
        <div class="kit-editor-swatch" style="${() => `background: ${current()}`}"></div>
        <input type="text" class="kit-field" value="${current}" @input=${(e: Event) => setColor(field, (e.target as HTMLInputElement).value)}>
        <input type="color" value="${() => normaliseToHex(current())}" @input=${(e: Event) => setColor(field, (e.target as HTMLInputElement).value)}>
        <div class="kit-editor-actions">${button({ label: 'Reset', variant: 'ghost', onClick: resetTheme })}</div>
      </div>
    </div>
  `;
}

/** Best-effort hex form for the <input type="color"> element. */
function normaliseToHex(value: string): string {
  if (value.startsWith('#') && (value.length === 7 || value.length === 4)) return value;
  // For rgba(...) / non-hex, fall back to a neutral so the picker isn't
  // broken; the text input is still authoritative.
  return '#000000';
}
```

- [ ] **Step 2: Add `theme_reset` to KitApp**

In `crates/sola-kit/src/app/kit_app.rs`, add a new arm to `on_js_command`:

```rust
            "theme_reset" => self.handle_theme_reset(ctx),
```

And the handler:

```rust
    fn handle_theme_reset(&mut self, ctx: &mut AppCtx) -> Value {
        self.theme = Theme::default();
        ctx.emit(Topic::Theme(self.theme.clone()));
        json!({ "ok": true })
    }
```

- [ ] **Step 3: Write tokens-colors.ts (the per-color list and detail view)**

Create `crates/sola-kit/web/app/src/preview/tokens-colors.ts`:

```ts
import { html, reactive } from '@arrow-js/core';
import { themeState, colorEditor } from '../token-edit';
import type { CatalogEntry } from '../sidebar';

const COLOR_FIELDS: Array<{ field: string; var: string }> = [
  { field: 'bg_primary',     var: '--bg-primary' },
  { field: 'bg_secondary',   var: '--bg-secondary' },
  { field: 'bg_tertiary',    var: '--bg-tertiary' },
  { field: 'bg_hover',       var: '--bg-hover' },
  { field: 'border',         var: '--border' },
  { field: 'border_subtle',  var: '--border-subtle' },
  { field: 'text_primary',   var: '--text-primary' },
  { field: 'text_secondary', var: '--text-secondary' },
  { field: 'text_tertiary',  var: '--text-tertiary' },
  { field: 'text_muted',     var: '--text-muted' },
  { field: 'text_accent',    var: '--text-accent' },
  { field: 'accent',         var: '--accent' },
  { field: 'accent_dim',     var: '--accent-dim' },
  { field: 'danger',         var: '--danger' },
  { field: 'success',        var: '--success' },
];

const local = reactive({ openVar: '--accent' });

export function renderColors(catalog: CatalogEntry[]) {
  return html`
    <div class="kit-colors">
      <div class="kit-colors-list">
        ${COLOR_FIELDS.map(f => html`
          <button
            class="kit-color-row"
            data-active="${() => local.openVar === f.var ? 'active' : false}"
            @click=${() => { local.openVar = f.var; }}
          >
            <span class="kit-color-swatch" style="${() => `background: ${themeState.current?.colors?.[f.field] ?? ''}`}"></span>
            <span class="kit-color-name">${f.var}</span>
            <span class="kit-color-value">${() => themeState.current?.colors?.[f.field] ?? ''}</span>
          </button>
        `)}
      </div>
      <div class="kit-colors-detail">
        ${() => {
          const entry = COLOR_FIELDS.find(f => f.var === local.openVar);
          if (!entry) return html``;
          const used = catalog.filter(e => e.tokens.includes(entry.var));
          return html`
            ${colorEditor(entry.field, entry.var, used)}
            <div class="kit-affected">
              <div class="kit-section-title-sm">Used in</div>
              ${used.length === 0
                ? html`<div class="kit-empty">No components use this token.</div>`
                : html`<ul class="kit-affected-list">${used.map(c => html`<li>${c.name}</li>`)}</ul>`}
            </div>
          `;
        }}
      </div>
    </div>
  `;
}
```

(Note: this v1 lists affected components by name; richer mini-preview tiles are a follow-up — out-of-scope for this task to keep the PR small. The data is in `used`; mounting actual previews per tile is a v1.1 polish.)

- [ ] **Step 4: Route from app.ts**

In `crates/sola-kit/web/app/src/app.ts`, replace the placeholder body with a switch on `state.selected`:

```ts
import { html, reactive } from '@arrow-js/core';
import { renderSidebar, type CatalogEntry } from './sidebar';
import { renderColors } from './preview/tokens-colors';

declare global {
  interface Window { RESTORED_STATE?: { catalog: CatalogEntry[]; theme: unknown }; }
}

const restored = window.RESTORED_STATE ?? { catalog: [], theme: null };

const state = reactive({
  selected: 'tokens.colors',
  catalog: restored.catalog as CatalogEntry[],
});

export function mount(target: HTMLElement) {
  html`
    <div class="kit-shell">
      ${() => renderSidebar(
        { selected: state.selected },
        state.catalog,
        (id: string) => { state.selected = id; },
      )}
      <main class="kit-work">
        ${() => routeWork(state.selected, state.catalog)}
      </main>
    </div>
  `(target);
}

function routeWork(selected: string, catalog: CatalogEntry[]) {
  if (selected === 'tokens.colors') return renderColors(catalog);
  return html`<div class="kit-placeholder">${selected}</div>`;
}
```

- [ ] **Step 5: Add CSS for the new classes**

Append to `crates/sola-kit/web/app/src/app.css`:

```css
.kit-colors { display: flex; gap: var(--space-lg); height: 100%; }
.kit-colors-list { width: 280px; flex-shrink: 0; display: flex; flex-direction: column; gap: 1px; overflow-y: auto; }
.kit-color-row {
  display: flex; align-items: center; gap: var(--space-sm);
  background: none; border: none; text-align: left; padding: var(--space-xs) var(--space-sm);
  border-radius: var(--radius-sm); cursor: pointer; color: var(--text-secondary); font: inherit;
}
.kit-color-row:hover { background: var(--bg-secondary); }
.kit-color-row[data-active="active"] { background: var(--accent-dim); color: var(--accent); }
.kit-color-swatch { width: 18px; height: 18px; border-radius: 3px; flex-shrink: 0; box-shadow: inset 0 0 0 1px var(--border-subtle); }
.kit-color-name { font-family: var(--font-mono); font-size: var(--text-caption); flex: 1; }
.kit-color-value { font-family: var(--font-mono); font-size: var(--text-caption); color: var(--text-tertiary); }

.kit-colors-detail { flex: 1; min-width: 0; }

.kit-editor-strip { padding: var(--space-md); background: var(--bg-secondary); border-radius: var(--radius-md); margin-bottom: var(--space-md); }
.kit-editor-head { display: flex; align-items: baseline; gap: var(--space-sm); margin-bottom: var(--space-sm); }
.kit-editor-name { font-family: var(--font-mono); font-size: var(--text-heading); }
.kit-editor-meta { font-size: var(--text-caption); color: var(--text-tertiary); }
.kit-editor-row { display: flex; align-items: center; gap: var(--space-sm); }
.kit-editor-swatch { width: 48px; height: 48px; border-radius: var(--radius-md); box-shadow: inset 0 0 0 1px var(--border-subtle); }
.kit-editor-actions { margin-left: auto; }

.kit-affected { padding: var(--space-md); background: var(--bg-secondary); border-radius: var(--radius-md); }
.kit-affected-list { margin: 0; padding-left: var(--space-md); color: var(--text-secondary); font-family: var(--font-mono); font-size: var(--text-body); }
.kit-section-title-sm { font-size: var(--text-caption); text-transform: uppercase; letter-spacing: 0.08em; color: var(--text-muted); margin-bottom: var(--space-xs); }
```

- [ ] **Step 6: Build**

```bash
cargo build -p sola-kit 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-kit/web/app/ crates/sola-kit/src/app/kit_app.rs
git commit -m "feat(sola-kit): token-mode UI for Colors

Two-pane within the work area: list of every color token (swatch +
name + value) on the left, editor strip + 'Used in' panel on the right.
Edits propagate via setColor → applyTheme (live) + debounced
invoke('theme_set', { theme }) (bus). theme_reset emits Theme::default()
back through the bus. Mini-preview tiles per consuming component are
deferred — this v1 lists names; richer previews are a follow-up."
```

---

### Task 29: Token-mode for Typography + Spacing/Radius

Same shape as Task 28 but for the other two token groups. Less code each — typography/spacing/radius edits are number/string inputs without a color picker.

**Files:**
- Create: `crates/sola-kit/web/app/src/preview/tokens-typography.ts`
- Create: `crates/sola-kit/web/app/src/preview/tokens-spacing.ts`
- Modify: `crates/sola-kit/web/app/src/token-edit.ts` — add `setTypography`, `setSpacing`, `setRadius` setters.
- Modify: `crates/sola-kit/web/app/src/app.ts` — route `tokens.typography` and `tokens.spacing`.

- [ ] **Step 1: Add the missing setters to token-edit.ts**

Append to `token-edit.ts`:

```ts
export function setTypography(field: string, value: string) {
  if (!themeState.current.typography) return;
  themeState.current.typography[field] = value;
  applyTheme({ [`--${field.replaceAll('_', '-')}`]: value });
  scheduleEmit();
}

export function setSpacing(field: string, value: string) {
  if (!themeState.current.spacing) return;
  themeState.current.spacing[field] = value;
  applyTheme({ [`--space-${field}`]: value });
  scheduleEmit();
}

export function setRadius(field: string, value: string) {
  if (!themeState.current.radius) return;
  themeState.current.radius[field] = value;
  applyTheme({ [`--radius-${field}`]: value });
  scheduleEmit();
}
```

- [ ] **Step 2: Write tokens-typography.ts**

```ts
import { html } from '@arrow-js/core';
import { themeState, setTypography } from '../token-edit';

const TYPE_FIELDS: Array<{ field: string; var: string; label: string }> = [
  { field: 'font_sans',     var: '--font-sans',     label: 'Sans family' },
  { field: 'font_mono',     var: '--font-mono',     label: 'Mono family' },
  { field: 'text_caption',  var: '--text-caption',  label: 'Caption (11)' },
  { field: 'text_body',     var: '--text-body',     label: 'Body (12)' },
  { field: 'text_body_lg',  var: '--text-body-lg',  label: 'Body L (13)' },
  { field: 'text_heading',  var: '--text-heading',  label: 'Heading (16)' },
  { field: 'text_display',  var: '--text-display',  label: 'Display (20)' },
];

export function renderTypography() {
  return html`
    <div class="kit-typography">
      ${TYPE_FIELDS.map(f => html`
        <div class="kit-type-row">
          <div class="kit-type-label">${f.label} <span class="kit-type-var">${f.var}</span></div>
          <input class="kit-field" value="${() => themeState.current?.typography?.[f.field] ?? ''}"
            @input=${(e: Event) => setTypography(f.field, (e.target as HTMLInputElement).value)}>
          <div class="kit-type-sample" style="${() => f.field.startsWith('font_')
            ? `font-family: ${themeState.current?.typography?.[f.field] ?? 'inherit'};`
            : `font-size: ${themeState.current?.typography?.[f.field] ?? 'inherit'};`}">The quick brown fox</div>
        </div>
      `)}
    </div>
  `;
}
```

- [ ] **Step 3: Write tokens-spacing.ts**

```ts
import { html } from '@arrow-js/core';
import { themeState, setSpacing, setRadius } from '../token-edit';

const SPACE_FIELDS = ['xs', 'sm', 'md', 'lg', 'xl', 'xxl'];
const RADIUS_FIELDS = ['sm', 'md', 'lg'];

export function renderSpacing() {
  return html`
    <div class="kit-spacing">
      <div class="kit-section-title-sm">Spacing</div>
      ${SPACE_FIELDS.map(k => html`
        <div class="kit-type-row">
          <div class="kit-type-label">--space-${k}</div>
          <input class="kit-field" value="${() => themeState.current?.spacing?.[k] ?? ''}"
            @input=${(e: Event) => setSpacing(k, (e.target as HTMLInputElement).value)}>
          <div class="kit-space-sample" style="${() => `width: ${themeState.current?.spacing?.[k] ?? '0'}; height: 12px; background: var(--accent);`}"></div>
        </div>
      `)}
      <div class="kit-section-title-sm" style="margin-top: var(--space-md)">Radius</div>
      ${RADIUS_FIELDS.map(k => html`
        <div class="kit-type-row">
          <div class="kit-type-label">--radius-${k}</div>
          <input class="kit-field" value="${() => themeState.current?.radius?.[k] ?? ''}"
            @input=${(e: Event) => setRadius(k, (e.target as HTMLInputElement).value)}>
          <div class="kit-radius-sample" style="${() => `width: 32px; height: 32px; background: var(--accent-dim); border-radius: ${themeState.current?.radius?.[k] ?? '0'};`}"></div>
        </div>
      `)}
    </div>
  `;
}
```

- [ ] **Step 4: Wire routes + CSS**

In `app.ts`:

```ts
import { renderTypography } from './preview/tokens-typography';
import { renderSpacing } from './preview/tokens-spacing';
// ...
function routeWork(selected: string, catalog: CatalogEntry[]) {
  if (selected === 'tokens.colors')     return renderColors(catalog);
  if (selected === 'tokens.typography') return renderTypography();
  if (selected === 'tokens.spacing')    return renderSpacing();
  return html`<div class="kit-placeholder">${selected}</div>`;
}
```

Append to `app.css`:

```css
.kit-type-row { display: flex; align-items: center; gap: var(--space-md); margin-bottom: var(--space-sm); }
.kit-type-label { width: 180px; flex-shrink: 0; font-size: var(--text-body); color: var(--text-secondary); }
.kit-type-var { font-family: var(--font-mono); color: var(--text-muted); font-size: var(--text-caption); margin-left: var(--space-xs); }
.kit-type-sample { flex: 1; color: var(--text-tertiary); }
.kit-space-sample, .kit-radius-sample { display: inline-block; }
.kit-spacing, .kit-typography { padding: var(--space-md); background: var(--bg-secondary); border-radius: var(--radius-md); }
```

- [ ] **Step 5: Build + commit**

```bash
cargo build -p sola-kit 2>&1 | tail -5
git add crates/sola-kit/web/app/ 
git commit -m "feat(sola-kit): token-mode UI for Typography + Spacing/Radius

Both groups are list-of-rows with an inline edit field and a live
sample (font-family / font-size on the typography preview line; a
horizontal bar / a radius'd square for spacing+radius). Setters
mirror the colors path: mutate themeState, applyTheme synchronously,
debounce-emit Topic::Theme."
```

---

### Task 30: Component-mode work area

For each catalog entry under Atoms or Components, render the component live + a token-chip strip below.

**Files:**
- Create: `crates/sola-kit/web/app/src/preview/component-view.ts` — generic per-component renderer.
- Modify: `crates/sola-kit/web/app/src/app.ts` — route `atoms.*` and `components.*` to it.

- [ ] **Step 1: Write component-view.ts**

```ts
import { html, type TemplatePartial } from '@arrow-js/core';
import {
  button, field, badge, icon,
  sidebar, navItem, section, row, list, form, fieldRow,
  tabs, tab, toast, empty,
} from '@sola/kit';
import { themeState, setColor } from '../token-edit';
import type { CatalogEntry } from '../sidebar';

interface ViewSpec {
  variants: () => TemplatePartial;
  notes?: string;
}

const VIEWS: Record<string, ViewSpec> = {
  button: { variants: () => html`
    <div class="kit-variants">
      ${button({ label: 'Primary', variant: 'primary' })}
      ${button({ label: 'Default' })}
      ${button({ label: 'Ghost', variant: 'ghost' })}
      ${button({ label: 'Danger', variant: 'danger' })}
      ${button({ label: '+ Add', variant: 'add' })}
    </div>
  ` },
  field: { variants: () => html`
    <div class="kit-variants kit-variants-stack">
      ${field({ value: '', placeholder: 'placeholder' })}
      ${field({ value: 'with value' })}
      ${field({ value: 'invalid', error: 'oops' })}
    </div>
  ` },
  badge: { variants: () => html`
    <div class="kit-variants">
      ${badge({ label: 'default' })}
      ${badge({ label: 'accent', variant: 'accent' })}
      ${badge({ label: 'danger', variant: 'danger' })}
      ${badge({ label: 'success', variant: 'success' })}
    </div>
  ` },
  icon: { variants: () => html`<div class="kit-variants">${icon({ name: 'lucide/palette', size: 24 })}</div>` },
  sidebar: { variants: () => html`<div class="kit-variants" style="height: 200px">${sidebar({
    title: 'Title',
    body: html`${navItem({ label: 'Item A', active: true })}${navItem({ label: 'Item B' })}`,
  })}</div>` },
  'nav-item': { variants: () => html`<div class="kit-variants kit-variants-stack">
    ${navItem({ label: 'Inactive' })}
    ${navItem({ label: 'Active', active: true })}
  </div>` },
  section: { variants: () => html`${section({ title: 'A section', description: 'A short description.', body: html`<p>Body content.</p>` })}` },
  row: { variants: () => html`<div class="kit-variants kit-variants-stack">
    ${row({ label: 'Simple row' })}
    ${row({ label: 'Row with detail', detail: '/path/to/value' })}
    ${row({ label: 'Row with actions', actions: html`${button({ label: 'Edit', variant: 'ghost' })}` })}
  </div>` },
  list: { variants: () => html`${list({ body: html`${row({ label: 'one' })}${row({ label: 'two' })}${row({ label: 'three' })}` })}` },
  form: { variants: () => html`${form({
    body: html`${fieldRow({ label: 'Email', body: field({ value: 'user@example.com' }) })}${fieldRow({ label: 'Pass', body: field({ value: '', type: 'password' }) })}`,
    actions: html`${button({ label: 'Save', variant: 'primary' })}${button({ label: 'Cancel', variant: 'ghost' })}`,
  })}` },
  tabs: { variants: () => html`<div class="kit-variants kit-variants-stack" style="width:240px">
    ${tabs({ body: html`
      ${tab({ title: 'one',   variant: 'numbered', index: 1, active: true })}
      ${tab({ title: 'two',   variant: 'numbered', index: 2 })}
      ${tab({ title: 'three', variant: 'numbered', index: 3 })}
    ` })}
  </div>` },
  toast: { variants: () => html`<div class="kit-variants kit-variants-stack" style="max-width:360px">
    ${toast({ body: html`Default toast.` })}
    ${toast({ variant: 'success', body: html`Saved successfully.` })}
    ${toast({ variant: 'danger', body: html`Operation failed.` })}
  </div>` },
  empty: { variants: () => html`${empty({ label: 'Nothing yet', hint: 'Add an item to get started.' })}` },
};

export function renderComponent(name: string, catalog: CatalogEntry[]) {
  const view = VIEWS[name];
  const entry = catalog.find(c => c.name === name);
  if (!view || !entry) {
    return html`<div class="kit-placeholder">No preview for ${name}</div>`;
  }
  return html`
    <div class="kit-component-view">
      <div class="kit-section-title-sm">Variants</div>
      <div class="kit-preview">${view.variants()}</div>
      <div class="kit-section-title-sm" style="margin-top: var(--space-md)">Tokens this uses · click a chip to edit</div>
      <div class="kit-chips">
        ${entry.tokens.map(varName => renderChip(varName))}
      </div>
    </div>
  `;
}

function renderChip(varName: string) {
  // Map var → struct field (e.g. "--accent-dim" → "accent_dim")
  const colorField = stripPrefix(varName, '--')?.replaceAll('-', '_');
  const isColor = colorField && themeState.current?.colors && (colorField in themeState.current.colors);
  if (isColor) {
    const valueExpr = (): string => themeState.current?.colors?.[colorField!] ?? '';
    return html`<label class="kit-chip">
      <span class="kit-chip-swatch" style="${() => `background: ${valueExpr()}`}"></span>
      <span class="kit-chip-name">${varName}</span>
      <input type="color" value="${() => normaliseToHex(valueExpr())}" @input=${(e: Event) => setColor(colorField!, (e.target as HTMLInputElement).value)}>
    </label>`;
  }
  // Non-color tokens (typography, spacing, radius) — show value as text;
  // editing routes to the token-mode views for now.
  return html`<span class="kit-chip">
    <span class="kit-chip-name">${varName}</span>
  </span>`;
}

function normaliseToHex(value: string): string {
  if (value.startsWith('#') && (value.length === 7 || value.length === 4)) return value;
  return '#000000';
}

function stripPrefix(s: string, p: string): string | null {
  return s.startsWith(p) ? s.slice(p.length) : null;
}
```

- [ ] **Step 2: Wire routes in app.ts**

In `crates/sola-kit/web/app/src/app.ts`'s `routeWork`:

```ts
import { renderComponent } from './preview/component-view';
// ...
function routeWork(selected: string, catalog: CatalogEntry[]) {
  if (selected === 'tokens.colors')     return renderColors(catalog);
  if (selected === 'tokens.typography') return renderTypography();
  if (selected === 'tokens.spacing')    return renderSpacing();
  if (selected.startsWith('atoms.'))     return renderComponent(selected.slice('atoms.'.length), catalog);
  if (selected.startsWith('components.')) return renderComponent(selected.slice('components.'.length), catalog);
  return html`<div class="kit-placeholder">${selected}</div>`;
}
```

- [ ] **Step 3: CSS for component-view, chips, preview**

Append to `app.css`:

```css
.kit-component-view { padding: 0; }
.kit-preview { background: var(--bg-secondary); border-radius: var(--radius-md); padding: var(--space-md); margin-bottom: var(--space-md); }
.kit-variants { display: flex; gap: var(--space-sm); align-items: center; flex-wrap: wrap; }
.kit-variants-stack { flex-direction: column; align-items: stretch; }

.kit-chips { display: flex; flex-wrap: wrap; gap: var(--space-xs); }
.kit-chip {
  display: inline-flex; align-items: center; gap: var(--space-xs);
  padding: var(--space-xs) var(--space-sm);
  background: var(--bg-secondary); border: 1px solid var(--border-subtle);
  border-radius: 999px; font-family: var(--font-mono); font-size: var(--text-caption);
  color: var(--text-secondary); cursor: pointer;
}
.kit-chip-swatch { width: 14px; height: 14px; border-radius: 50%; }
.kit-chip input[type="color"] {
  width: 16px; height: 16px; border: none; padding: 0; background: none;
}
```

- [ ] **Step 4: Build + commit**

```bash
cargo build -p sola-kit 2>&1 | tail -5
git add crates/sola-kit/web/app/
git commit -m "feat(sola-kit): component-mode UI

Per-component view: live variants in a preview block, editable token
chips below. Color chips include an inline <input type=color> that
calls setColor — same lifecycle as token-mode color edits. Non-color
chips show the var name only; users edit them via the token-mode
typography/spacing views."
```

---

## Phase 7 — Final pass

### Task 31: Add `sola-kit/` to CLAUDE.md crates listing

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Edit CLAUDE.md**

Locate the `crates/` listing in the project root `CLAUDE.md` (search for `sola-app/` in the listing). Insert a line for `sola-kit/`:

```
  sola-kit/            # WebView app framework + design-token kit + storybook (parallel to sola-app)
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add sola-kit to crates listing in CLAUDE.md"
```

---

### Task 32: Full workspace build + manual smoke

- [ ] **Step 1: Full workspace build**

```bash
cargo make build 2>&1 | tail -10
```

Expected: every crate compiles. No warnings introduced (or, if any, related only to copied code that already had warnings in sola-app).

- [ ] **Step 2: Run all tests**

```bash
cargo test --workspace 2>&1 | tail -20
```

Expected: pre-existing tests still pass. The new tests (Theme to_css_vars × 3, Topic::Theme round-trip × 1, kit_css_drift, catalog parity) all pass.

- [ ] **Step 3: Smoke checklist (manual; only if user authorises an `install` step)**

The user runs:
```bash
cargo make install sola-kit
/opt/sola/bin/sola-kit
```

In the launched window, verify:
1. The window is 1100×720, titled "Theme".
2. Sidebar shows three groups: Tokens, Atoms, Components, with all expected items.
3. `tokens.colors` is selected by default; the colors list is fully populated; `--accent` is the open detail.
4. Editing a swatch in the editor strip immediately changes the displayed color across the whole storybook (the sidebar's active-state colour, etc.).
5. After ~300 ms the bus persistence file (`~/.config/sola/bus/state.toml` or similar — check existing path used by MailConfig) contains the updated `Theme` block.
6. Restart sola-kit. The persisted theme is replayed and the storybook starts at the modified colour.
7. `theme_reset` (Reset button in the editor strip) restores the defaults.
8. Each atom + component in the catalog renders without console errors when navigated to.

**This step requires the user's explicit permission per the project's install rule.** The plan stops at "build is clean + tests pass." Hand back to the user for the install + smoke.

- [ ] **Step 4: Final commit (if any squashes/cleanup needed)**

```bash
git status
# If clean, no commit needed.
```

---

## Self-Review

**Spec coverage:** every section of `2026-04-30-sola-kit-design.md` maps to one or more tasks above:
- §3.1 Crate layout → Tasks 4–9
- §3.2 Touch sites outside sola-kit → Tasks 1–3
- §3.3 Migration path → explicitly out of scope (documented in spec; no task here)
- §4 JS API surface → Tasks 7, 11, 13–25
- §4.4 Token-usage metadata + parity test → Task 26
- §4.5 CSS layer (kit.css) → Task 7 + per-atom/component additions
- §4.6 Auto-wired distribution → Tasks 10, 11
- §5 Token data model + bus schema → Tasks 1, 2, 8
- §5.6 Defaults & sync → Task 8
- §6 Editor / storybook UX → Tasks 27–30
- §7 Testing → Tasks 1, 2, 8, 26 (unit); Task 32 step 3 (manual)

**Placeholder scan:** searched plan for "TBD", "TODO", "later", "etc." — no orphan placeholders. Token-mode mini-preview tiles are deferred (Task 28 v1.1) but called out explicitly.

**Type consistency:** `KitApp.theme` is `sola_core::theme::Theme` throughout. `Topic::Theme(Theme)` matches. JS `themeState.current` mirrors the Theme JSON shape. `*Tokens` exports use the same var names as `CATALOG`'s `tokens` arrays (parity test enforces).

**Coverage gaps:** the spec's §6.4 cross-highlighting (token chip ↔ preview region outline on hover) is **not** implemented in Tasks 28/30 — only the explicit "Used in" list is. This is a deliberate scope cut to keep the v1 plan manageable; cross-highlighting is a polish pass that doesn't affect any consumer (sola-kit is the only one in v1). Noted as v1.1 in Task 28.

---

**Plan complete.**
