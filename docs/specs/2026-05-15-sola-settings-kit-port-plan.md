# sola-settings → sola-kit port — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/specs/2026-05-15-sola-settings-kit-port-design.md`

**Goal:** Replace `sola-settings`'s GTK4/WebKit6 stack with `sola-kit` (CEF/Remix v3) in place, redesign UI flow to adopt kit primitives, and extend the kit where needed.

**Architecture:** Three tiny independent kit additions land first (Button `confirm` prop, `Badge` component). Then the settings crate is rewritten: `Cargo.toml` swap → new Rust scaffold (`main.rs` + `app.rs` + `procfs.rs`) → new Remix v3 frontend (`web/main.tsx` + panels) → asset bundle wired up. Each port task lands on master as its own commit.

**Tech Stack:**
- Rust: `sola-kit`, `sola-bus`, `sola-core`, `serde`, `serde_json`, `tracing`
- Frontend: Remix v3 (`@remix-run/ui`, vendored in sola-kit), TypeScript, kit components (`@sola/*`)
- Build: `cargo make build`

**Notes for the executor:**
- This codebase has no UI test infrastructure. The "test" for kit additions is `cargo test -p sola-kit --lib` which runs theme snapshot tests; updating those snapshots in lockstep with code is part of every kit-additions task. For settings tasks, `cargo make build` is the verification step. The user will smoke-test manually.
- **NEVER run `cargo make install` (or any install variant).** Only `cargo make build` / `cargo build` / `cargo test`.
- All work goes on master directly (per user's instruction for this task).
- Plan tasks **must run in order** — Task 3+ depend on Tasks 1–2; Tasks 4+ depend on Task 3; etc.

---

## File Structure (post-port)

```
crates/sola-kit/                                # 2 component additions
  web/lib/components/
    badge.tsx                                   # NEW
    badge.css                                   # NEW
    button.tsx                                  # MODIFIED — confirm prop
    button.css                                  # MODIFIED — confirm pulse styling
  src/
    assets.rs                                   # MODIFIED — register badge
    lib.rs                                      # MODIFIED — importmap entry
    theme.rs                                    # MODIFIED — snapshot includes badge
    components/
      badge.rs                                  # NEW
      button.rs                                 # MODIFIED — (optional, no slot changes)
      mod.rs                                    # MODIFIED — register badge
    categories.rs                               # MODIFIED — badge handled in for_component
  web/app/showcases/
    badge.tsx                                   # NEW — showcase page
    index.ts                                    # MODIFIED — register badge showcase

crates/sola-settings/
  Cargo.toml                                    # REWRITTEN
  src/
    main.rs                                     # REWRITTEN
    app.rs                                      # NEW
    procfs.rs                                   # NEW
  web/
    main.tsx                                    # NEW
    panels/
      applications.tsx                          # NEW
      mail.tsx                                  # NEW
    DELETED: index.html
    DELETED: src/main.ts
    DELETED: src/app.ts
    DELETED: src/theme.css
```

---

## Task 1: Kit — `Button` `confirm` prop

**Files:**
- Modify: `crates/sola-kit/web/lib/components/button.tsx`
- Modify: `crates/sola-kit/web/lib/components/button.css`
- Modify: `crates/sola-kit/web/app/showcases/button.tsx` (add a confirm example row)

**Behavior:**
- Idle: button renders normally.
- Click 1: label swaps to "Click again to confirm" (or `confirmLabel` if provided), variant flips to `danger` for the duration, the original `variant` is restored after timeout or second click.
- Click 2 within 2000 ms: fires `onPress`, resets to idle.
- 2000 ms of inactivity after click 1: resets to idle without firing.
- The visual change is variant-only (no extra DOM/class). No CSS changes needed; we already have `.sola-button-danger`.

- [ ] **Step 1: Read the current Button factory** to confirm where the variant is resolved at render time.

```bash
cat crates/sola-kit/web/lib/components/button.tsx
```

- [ ] **Step 2: Modify `button.tsx` — add the `confirm` props and arm/disarm state**

Replace the file contents with this updated version (only the prop interface, the factory body, and the imports change — the class assembly stays identical):

```tsx
// Button — kit-shipped Remix v3 component.
//
// Single factory `Button` rendering a real `<button>` element so the
// browser handles keyboard activation (Enter/Space → click), focus,
// and the disabled-removed-from-tab-order semantics for free.
//
// Variant + state styling lives entirely in `button.css`; this file
// only assembles class names. CSS references only `--sola-button-*`
// scoped vars, never atoms — the theme protocol owns the look.
//
// Slots are named props (Remix v3 idiom): `leading` / `trailing` for
// adornments around the label; the label itself is default-slot
// `children`.
//
// `confirm` mode — two-stage destructive action. First click swaps
// the visible variant to `danger` and the label to `confirmLabel`;
// a second click within 2 s commits and fires `onPress`. 2 s of
// inactivity rolls back to idle silently. The disarm timer is
// component-owned (`setTimeout`) and cleared on every interaction.

import { type Handle, type RemixNode } from "@remix-run/ui";
import { on } from "@sola/kit";

export type ButtonVariant = "default" | "primary" | "ghost" | "danger";

export interface ButtonProps {
  variant?: ButtonVariant;
  disabled?: boolean;
  type?: "button" | "submit" | "reset";
  onPress?: () => void;
  leading?: RemixNode;
  trailing?: RemixNode;
  children?: RemixNode;
  /**
   * Two-stage confirmation pattern. When `true`, the first click
   * arms the button (variant flips to danger, label swaps to
   * `confirmLabel`); the next click within 2 s fires `onPress`.
   * 2 s of inactivity disarms silently.
   */
  confirm?: boolean;
  /** Label shown while armed. Defaults to "Click again to confirm". */
  confirmLabel?: string;
}

const CONFIRM_TIMEOUT_MS = 2000;

export function Button(handle: Handle<ButtonProps>) {
  let armed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const disarm = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (armed) {
      armed = false;
      handle.update();
    }
  };

  const handleClick = () => {
    if (handle.props.disabled) return;
    if (handle.props.confirm) {
      if (!armed) {
        armed = true;
        timer = setTimeout(() => {
          armed = false;
          timer = null;
          handle.update();
        }, CONFIRM_TIMEOUT_MS);
        handle.update();
        return;
      }
      // armed → commit
      disarm();
      handle.props.onPress?.();
      return;
    }
    handle.props.onPress?.();
  };

  return () => {
    const {
      variant,
      disabled,
      type,
      leading,
      trailing,
      children,
      confirm,
      confirmLabel,
    } = handle.props;
    const v: ButtonVariant = armed && confirm ? "danger" : variant ?? "default";

    const classes = [
      "sola-button",
      `sola-button-${v}`,
      disabled ? "is-disabled" : "",
      armed && confirm ? "is-armed" : "",
    ]
      .filter(Boolean)
      .join(" ");

    const labelContent = armed && confirm
      ? (confirmLabel ?? "Click again to confirm")
      : children;

    return (
      <button
        class={classes}
        type={type ?? "button"}
        disabled={disabled ? true : false}
        mix={[on("click", handleClick)]}
      >
        {leading
          ? <span class="sola-button-leading">{leading}</span>
          : null}
        <span class="sola-button-label">{labelContent}</span>
        {trailing
          ? <span class="sola-button-trailing">{trailing}</span>
          : null}
      </button>
    );
  };
}
```

- [ ] **Step 3: Add a `.is-armed` style hint in `button.css`**

Append to `crates/sola-kit/web/lib/components/button.css`:

```css
/* armed (confirm mode) — subtle outline so the danger fill alone
   isn't the only signal that the button has changed state. */

.sola-button.is-armed {
  outline: 1px solid var(--sola-button-danger-bg);
  outline-offset: 2px;
}
```

- [ ] **Step 4: Update the Button showcase to exercise `confirm`**

Open `crates/sola-kit/web/app/showcases/button.tsx`, find the last variant block (`danger`), and add a new Stack section after it inside the live-preview Card:

```tsx
          <Stack gap="xs">
            <Text kind="label">confirm</Text>
            <Stack direction="row" gap="md" align="center">
              <Button variant="danger" confirm onPress={onPress}>
                Delete
              </Button>
              <Button
                variant="ghost"
                confirm
                confirmLabel="Tap again to discard"
                onPress={onPress}
              >
                Discard changes
              </Button>
            </Stack>
          </Stack>
```

(Place it immediately before the closing `</Stack>` of the "Live preview" Card's body Stack.)

- [ ] **Step 5: Build + test kit**

```bash
cargo test -p sola-kit --lib
```

Expected: `test result: ok. 3 passed; 0 failed`. No theme snapshot drift (no bindings changed).

- [ ] **Step 6: Build the workspace**

```bash
cargo make build
```

Expected: clean build.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-kit/web/lib/components/button.tsx crates/sola-kit/web/lib/components/button.css crates/sola-kit/web/app/showcases/button.tsx
git commit -m "$(cat <<'EOF'
feat(sola-kit): Button \`confirm\` prop for two-stage destructive actions

First click arms the button (label → \`confirmLabel\`, variant → danger);
second click within 2 s fires onPress; 2 s of inactivity disarms.
Adds an outline-based \`.is-armed\` hint so the variant flip isn't the
only state signal. Showcase exercises both \`confirm\` and
\`confirmLabel\` overrides.
EOF
)"
```

---

## Task 2: Kit — `Badge` component

**Files:**
- Create: `crates/sola-kit/web/lib/components/badge.tsx`
- Create: `crates/sola-kit/web/lib/components/badge.css`
- Create: `crates/sola-kit/src/components/badge.rs`
- Create: `crates/sola-kit/web/app/showcases/badge.tsx`
- Modify: `crates/sola-kit/src/components/mod.rs`
- Modify: `crates/sola-kit/src/assets.rs`
- Modify: `crates/sola-kit/src/lib.rs`
- Modify: `crates/sola-kit/src/categories.rs`
- Modify: `crates/sola-kit/src/theme.rs` (snapshot test)
- Modify: `crates/sola-kit/web/app/showcases/index.ts`

- [ ] **Step 1: Create `web/lib/components/badge.tsx`**

```tsx
// Badge — small pill displaying status text alongside other
// content. `kind` chooses semantic color (background + foreground
// scoped vars); shape (radius, padding, text size) is shared.
//
// Used for inline status indicators (e.g. "not found" next to a
// configured application, "unread" count next to a mailbox). Not
// for free-form labels — use `<Text kind="label">` for those.

import { type Handle, type RemixNode } from "@remix-run/ui";

export type BadgeKind =
  | "neutral"
  | "info"
  | "success"
  | "warning"
  | "danger";

export interface BadgeProps {
  /** Semantic color tone. Defaults to "neutral". */
  kind?: BadgeKind;
  children?: RemixNode;
}

export function Badge(handle: Handle<BadgeProps>) {
  return () => {
    const k: BadgeKind = handle.props.kind ?? "neutral";
    return (
      <span class={`sola-badge sola-badge-${k}`}>
        {handle.props.children}
      </span>
    );
  };
}
```

- [ ] **Step 2: Create `web/lib/components/badge.css`**

```css
/* Badge — small pill with kind-driven color. Shape slots are
   shared across kinds; only bg/text vary. */

.sola-badge {
  display: inline-flex;
  align-items: center;
  padding-block: var(--sola-badge-padding-block);
  padding-inline: var(--sola-badge-padding-inline);
  border-radius: var(--sola-badge-radius);
  font-size: var(--sola-badge-text-size);
  line-height: 1;
  white-space: nowrap;
  flex: 0 0 auto;
}

.sola-badge-neutral {
  background: var(--sola-badge-neutral-bg);
  color: var(--sola-badge-neutral-text);
}

.sola-badge-info {
  background: var(--sola-badge-info-bg);
  color: var(--sola-badge-info-text);
}

.sola-badge-success {
  background: var(--sola-badge-success-bg);
  color: var(--sola-badge-success-text);
}

.sola-badge-warning {
  background: var(--sola-badge-warning-bg);
  color: var(--sola-badge-warning-text);
}

.sola-badge-danger {
  background: var(--sola-badge-danger-bg);
  color: var(--sola-badge-danger-text);
}
```

- [ ] **Step 3: Create `crates/sola-kit/src/components/badge.rs`**

```rust
//! `badge` component bindings + editor categories. The Tsx and
//! CSS siblings live at `web/lib/components/badge.{tsx,css}` and
//! reference only `--sola-badge-*` scoped vars. Shape slots
//! (radius, padding, text size) are shared across kinds; bg/text
//! vary per kind.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Shape (kind-agnostic).
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-sm"));
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-caption"));
    // Neutral kind — subtle surface tint.
    comp.slots.insert("neutral-bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("neutral-text".into(), Binding::new("text", "text-secondary"));
    // Info kind — accent-tinted.
    comp.slots.insert("info-bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("info-text".into(), Binding::new("text", "text-accent"));
    // Success kind — saturated status fill.
    comp.slots.insert("success-bg".into(), Binding::new("status", "success"));
    comp.slots.insert("success-text".into(), Binding::new("text", "text-primary"));
    // Warning kind — uses danger for visibility (no dedicated warning atom yet).
    comp.slots.insert("warning-bg".into(), Binding::new("status", "danger"));
    comp.slots.insert("warning-text".into(), Binding::new("text", "text-primary"));
    // Danger kind — saturated status fill.
    comp.slots.insert("danger-bg".into(), Binding::new("status", "danger"));
    comp.slots.insert("danger-text".into(), Binding::new("text", "text-primary"));
    comp
}

pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "shape",
            "Shape",
            vec![
                SlotEntry::new("radius", "Corner radius"),
                SlotEntry::new("padding-block", "Padding (vertical)"),
                SlotEntry::new("padding-inline", "Padding (horizontal)"),
                SlotEntry::new("text-size", "Label size"),
            ],
        )
        .with_description("Geometry shared by every kind."),
        Category::new(
            "neutral",
            "Neutral kind",
            vec![
                SlotEntry::new("neutral-bg", "Background"),
                SlotEntry::new("neutral-text", "Label"),
            ],
        )
        .with_description("Default unobtrusive variant."),
        Category::new(
            "info",
            "Info kind",
            vec![
                SlotEntry::new("info-bg", "Background"),
                SlotEntry::new("info-text", "Label"),
            ],
        )
        .with_description("Accent-tinted variant for neutral informational status."),
        Category::new(
            "success",
            "Success kind",
            vec![
                SlotEntry::new("success-bg", "Background"),
                SlotEntry::new("success-text", "Label"),
            ],
        )
        .with_description("Positive status (saved, validated, online)."),
        Category::new(
            "warning",
            "Warning kind",
            vec![
                SlotEntry::new("warning-bg", "Background"),
                SlotEntry::new("warning-text", "Label"),
            ],
        )
        .with_description("Caution status (missing, deprecated, attention needed)."),
        Category::new(
            "danger",
            "Danger kind",
            vec![
                SlotEntry::new("danger-bg", "Background"),
                SlotEntry::new("danger-text", "Label"),
            ],
        )
        .with_description("Error status (broken, unavailable, critical)."),
    ]
}
```

Verify the `sola_core::theme::Binding` group names used here (`"surface"`, `"text"`, `"status"`, `"radius"`, `"space"`, `"text-size"`) — these MUST match the groups already declared in the palette. Look at `crates/sola-core/src/theme.rs` or another kit `bindings.rs` file (`button.rs`, `card.rs`) to confirm.

- [ ] **Step 4: Register the module in `crates/sola-kit/src/components/mod.rs`**

Open the file. Add `pub mod badge;` to the module list (alphabetical — between `pub mod button;` and the prior entry). In `all_bindings()`, insert `map.insert("badge".into(), badge::bindings());` in alphabetical position (before `"button"`).

- [ ] **Step 5: Add the Tsx + CSS to `assets.rs`**

In `crates/sola-kit/src/assets.rs`, find the `platform_assets()` function and add (in alphabetical order — just before the `button.tsx` block):

```rust
            Asset {
                path: "/lib/components/badge.tsx",
                content: include_bytes!("../web/lib/components/badge.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/badge.css",
                content: include_bytes!("../web/lib/components/badge.css"),
                content_type: ContentType::Css,
            },
```

- [ ] **Step 6: Add the importmap entry in `crates/sola-kit/src/lib.rs::build_importmap`**

Find the importmap format-string and add a new entry. Place it alphabetically — before `"@sola/button"`:

```rust
      "@sola/badge":               "/lib/components/badge.tsx",
```

- [ ] **Step 7: Add `badge` to the `for_component` match in `crates/sola-kit/src/categories.rs`**

Add `"badge" => crate::components::badge::categories(),` to the match arm in `for_component`, in alphabetical order (between `"button"` (none in current list — `"button"` is the lowest) — actually it should come **first**, before `"button"`).

- [ ] **Step 8: Update the theme snapshot in `crates/sola-kit/src/theme.rs`**

Open the file. Locate the `expected = "..."` string in the `kit_default_theme_to_css_is_stable` test. Right after the atoms block (right before `/* button */`), insert:

```text
  /* badge */
  --sola-badge-danger-bg: var(--danger);
  --sola-badge-danger-text: var(--text-primary);
  --sola-badge-info-bg: var(--bg-tertiary);
  --sola-badge-info-text: var(--text-accent);
  --sola-badge-neutral-bg: var(--bg-tertiary);
  --sola-badge-neutral-text: var(--text-secondary);
  --sola-badge-padding-block: var(--space-xs);
  --sola-badge-padding-inline: var(--space-sm);
  --sola-badge-radius: var(--radius-sm);
  --sola-badge-success-bg: var(--success);
  --sola-badge-success-text: var(--text-primary);
  --sola-badge-text-size: var(--text-caption);
  --sola-badge-warning-bg: var(--danger);
  --sola-badge-warning-text: var(--text-primary);

```

(Note: `Theme::to_css` emits sections alphabetized by component name, and within a section the slots are alphabetized — `danger-bg` before `danger-text` before `info-bg` etc. The exact ordering above matches that contract.)

- [ ] **Step 9: Create the Badge showcase at `crates/sola-kit/web/app/showcases/badge.tsx`**

```tsx
// Badge showcase — one row per kind with idle samples + a context
// example showing a Badge inline with surrounding Text. Bindings
// editor below for live theming.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Badge, type BadgeKind } from "@sola/badge";
import { Card } from "@sola/card";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

const KINDS: BadgeKind[] = [
  "neutral",
  "info",
  "success",
  "warning",
  "danger",
];

export function BadgeShowcase(handle: Handle) {
  return () => (
    <Stack gap="xxl">
      <Card
        label="Live preview"
        description="Each row is one kind. The context example shows a Badge inline with surrounding text."
      >
        <Stack gap="lg">
          {KINDS.map((k) => (
            <Stack gap="xs">
              <Text kind="label">{k}</Text>
              <Stack direction="row" gap="md" align="center">
                <Badge kind={k}>{k}</Badge>
                <Badge kind={k}>longer label text</Badge>
              </Stack>
            </Stack>
          ))}
          <Stack gap="xs">
            <Text kind="label">in context</Text>
            <Stack direction="row" gap="sm" align="center">
              <Text>Firefox</Text>
              <Badge kind="warning">not found</Badge>
            </Stack>
          </Stack>
        </Stack>
      </Card>
      <BindingsEditor component="badge" />
    </Stack>
  );
}
```

Note: import path is `@sola/bindings-editor` (the master branch still has bindings-editor at `web/lib/components/`; the showcase-chrome branch's move hasn't been merged).

- [ ] **Step 10: Register the showcase in `crates/sola-kit/web/app/showcases/index.ts`**

Add `import { BadgeShowcase } from "./badge.tsx";` near the other showcase imports, then add an entry to the `showcases` array — alphabetical within the "Components" section, before `Button`:

```ts
  { id: "badge", label: "Badge", section: "Components", component: BadgeShowcase },
```

- [ ] **Step 11: Run the kit unit tests**

```bash
cargo test -p sola-kit --lib
```

Expected: `test result: ok. 3 passed; 0 failed`. The `kit_default_theme_to_css_is_stable` test will fail if the snapshot doesn't exactly match. If it does fail, the test output shows the diff — fix the snapshot to match the actual rendered CSS.

- [ ] **Step 12: Build the workspace**

```bash
cargo make build
```

Expected: clean build.

- [ ] **Step 13: Commit**

```bash
git add crates/sola-kit/web/lib/components/badge.tsx crates/sola-kit/web/lib/components/badge.css crates/sola-kit/src/components/badge.rs crates/sola-kit/src/components/mod.rs crates/sola-kit/src/assets.rs crates/sola-kit/src/lib.rs crates/sola-kit/src/categories.rs crates/sola-kit/src/theme.rs crates/sola-kit/web/app/showcases/badge.tsx crates/sola-kit/web/app/showcases/index.ts
git commit -m "$(cat <<'EOF'
feat(sola-kit): Badge component

Small pill for inline status display. Five kinds (neutral / info /
success / warning / danger), shape slots shared, kind-specific
bg + text. Wired into theme snapshot, importmap, asset bundle,
categories. Showcase exercises each kind + an in-context example
with text wrap.
EOF
)"
```

---

## Task 3: Settings — Rust scaffold

Goal: get `crates/sola-settings/` building against `sola-kit` with a minimal empty window. After this task the binary builds (`cargo make build -- sola-settings`) and runs, opening an empty "Settings" window. UI logic comes in later tasks.

**Files:**
- Modify: `crates/sola-settings/Cargo.toml`
- Modify: `crates/sola-settings/src/main.rs`
- Create: `crates/sola-settings/src/app.rs`
- Create: `crates/sola-settings/src/procfs.rs`
- Create: `crates/sola-settings/web/main.tsx`
- DELETE: `crates/sola-settings/web/index.html`
- DELETE: `crates/sola-settings/web/src/main.ts`
- DELETE: `crates/sola-settings/web/src/app.ts`  ← **save its contents for reference in Tasks 4 + 5**
- DELETE: `crates/sola-settings/web/src/theme.css`

- [ ] **Step 1: Snapshot the legacy `web/src/app.ts` to a temporary reference location**

The frontend logic in `app.ts` is the ground truth for what the panels in Tasks 4 + 5 must reproduce. Copy it somewhere outside the crate so the deletion doesn't destroy the reference:

```bash
cp crates/sola-settings/web/src/app.ts /tmp/sola-settings-app-ts.reference
cp crates/sola-settings/src/main.rs /tmp/sola-settings-main-rs.reference
```

These files will be consulted when porting in Tasks 4 + 5.

- [ ] **Step 2: Rewrite `crates/sola-settings/Cargo.toml`**

```toml
[package]
name = "sola-settings"
version.workspace = true
edition.workspace = true

[[bin]]
name = "sola-settings"
path = "src/main.rs"

[dependencies]
sola-kit = { path = "../sola-kit" }
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
include_dir = "0.7"
```

(Drop `sola-app` and `gtk4`. Add `include_dir` for the `web/` directory mount.)

- [ ] **Step 3: Create `crates/sola-settings/src/procfs.rs`**

Lift the `/proc`/PATH helpers from the legacy main.rs verbatim, as a free-standing module. Open `/tmp/sola-settings-main-rs.reference` and copy lines 387–552 (`suggest_command`, `resolve_from_app_id`, `resolve_binary_for_pid`, `is_multi_arg_launcher`, `cmdline_positional`) and the `is_system_app` helper. Adjust imports — only `std::path::Path` and `sola_core::applications::{command_exists, is_builtin, resolve_in_path}` are needed.

The contents:

```rust
//! /proc + PATH-based binary resolution for the "running but not
//! configured" candidate list. Pure leaf module — no bus, no UI,
//! no app state. Lifted from the legacy main.rs unchanged; the
//! original docstrings remain authoritative for behaviour.

use std::path::Path;

use sola_core::applications::{command_exists, is_builtin, resolve_in_path};

/// App IDs that are part of Sola itself and should never appear as
/// "running, not configured" candidates.
pub fn is_system_app(app_id: &str) -> bool {
    app_id == "sola-shell" || is_builtin(app_id)
}

/// Best-effort suggestion of a launch command for a window we just
/// noticed. See module docstring on the legacy version for the full
/// rationale; behaviour preserved exactly.
pub fn suggest_command(app_id: &str, pid: Option<u32>) -> Option<String> {
    if let Some(path) = resolve_from_app_id(app_id) {
        return Some(path);
    }
    pid.and_then(resolve_binary_for_pid)
}

fn resolve_from_app_id(app_id: &str) -> Option<String> {
    let trimmed = app_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut tried: Vec<String> = Vec::new();
    let try_name = |name: &str, tried: &mut Vec<String>| -> Option<String> {
        if name.is_empty() || tried.iter().any(|t| t == name) {
            return None;
        }
        tried.push(name.to_string());
        resolve_in_path(name).map(|p| p.to_string_lossy().into_owned())
    };

    if let Some(hit) = try_name(&trimmed.to_ascii_lowercase(), &mut tried) {
        return Some(hit);
    }
    let segments: Vec<&str> = trimmed.split('.').collect();
    if segments.len() > 1 {
        let last = segments[segments.len() - 1].to_ascii_lowercase();
        if let Some(hit) = try_name(&last, &mut tried) {
            return Some(hit);
        }
        let second = segments[segments.len() - 2].to_ascii_lowercase();
        if let Some(hit) = try_name(&second, &mut tried) {
            return Some(hit);
        }
    }
    None
}

fn resolve_binary_for_pid(pid: u32) -> Option<String> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    let cleaned = exe.map(|p| {
        let s = p.to_string_lossy().into_owned();
        s.strip_suffix(" (deleted)")
            .map(str::to_string)
            .unwrap_or(s)
    });

    let file_name = cleaned.as_deref().and_then(|c| {
        Path::new(c)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    });

    let need_cmdline = file_name.as_deref().is_none_or(is_multi_arg_launcher);
    if need_cmdline {
        return cmdline_positional(pid);
    }
    cleaned
}

fn is_multi_arg_launcher(name: &str) -> bool {
    matches!(
        name,
        "bwrap"
            | "flatpak-spawn"
            | "flatpak"
            | "AppRun"
            | "snap"
            | "snap-confine"
            | "electron"
    )
}

fn cmdline_positional(pid: u32) -> Option<String> {
    let data = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let parts: Vec<&[u8]> = data
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let mut take = 1;
    for arg in &parts[1..] {
        if arg.first() == Some(&b'-') {
            break;
        }
        take += 1;
    }
    let joined: Vec<String> = parts[..take]
        .iter()
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    Some(joined.join(" "))
}

/// True when `cmd` doesn't resolve on PATH. Thin wrapper over
/// `command_exists` for inversion-of-intent at call sites.
pub fn command_missing(cmd: &str) -> bool {
    !command_exists(cmd)
}
```

- [ ] **Step 4: Create the minimal `crates/sola-settings/src/app.rs`**

Just enough `SolaApp` to open one empty window. Full bus handlers / JS dispatch come in Task 4.

```rust
//! Settings app entrypoint. Owns one window backed by `web/main.tsx`.
//! State + bus handlers will land in Task 4.

use sola_kit::{
    AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};

static APP_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web");

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    @dir "/" => &APP_DIR,
};

pub struct SettingsApp {
    main_window: WindowHandle,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let main_window = ctx.add_window(WindowConfig {
            title: "Settings".into(),
            size: (900, 620),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            zoned: true,
            keyboard_target: true,
        });
        tracing::info!("sola-settings ready (kit)");
        Self { main_window }
    }

    fn register_bus(&mut self, _bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {}
}
```

- [ ] **Step 5: Replace `crates/sola-settings/src/main.rs`**

```rust
mod app;
mod procfs;

use std::process::ExitCode;

use sola_kit::SolaApp;

fn main() -> ExitCode {
    // Subprocess gate — CEF re-execs this binary as renderer/GPU/util/zygote.
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess(app::SettingsApp::APP_ID) {
        return code;
    }
    sola_kit::run::<app::SettingsApp>();
    ExitCode::SUCCESS
}
```

- [ ] **Step 6: Delete legacy frontend files**

```bash
rm crates/sola-settings/web/index.html crates/sola-settings/web/src/main.ts crates/sola-settings/web/src/app.ts crates/sola-settings/web/src/theme.css
rmdir crates/sola-settings/web/src
```

- [ ] **Step 7: Create the minimal `crates/sola-settings/web/main.tsx`**

```tsx
// Settings frontend root. Task 3 scaffold: just a Root with a
// placeholder. The full Split/Sidebar/Container/panels structure
// lands in Tasks 4-7.

import { type Handle } from "@remix-run/ui";
import { Root } from "@sola/root";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

export function Main(_handle: Handle) {
  return () => (
    <Root>
      <Stack gap="md" align="center" justify="center" fill>
        <Text kind="display">Settings</Text>
        <Text tone="muted">scaffold</Text>
      </Stack>
    </Root>
  );
}
```

- [ ] **Step 8: Build**

```bash
cargo make build
```

Expected: clean build. The new sola-settings binary links against sola-kit.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor(sola-settings): scaffold the sola-kit port

Cargo.toml swap: sola-app + gtk4 → sola-kit + include_dir. Legacy
web/{index.html, src/} deleted in favour of an include_dir-mounted
web/main.tsx. main.rs split into subprocess gate (main.rs) +
SettingsApp impl (app.rs) + /proc helpers (procfs.rs).

This is the minimum buildable kit-side scaffold. Bus handlers, JS
commands, and the Applications/Mail panels arrive in follow-up
commits.
EOF
)"
```

---

## Task 4: Settings — Rust bus handlers + JS commands

Goal: port all the application + mail logic from `/tmp/sola-settings-main-rs.reference` into `crates/sola-settings/src/app.rs`. The `state_payload` shape must match what the JS panels (Tasks 5 + 6) will consume.

**Files:**
- Modify: `crates/sola-settings/src/app.rs`

- [ ] **Step 1: Replace `crates/sola-settings/src/app.rs`**

Open `/tmp/sola-settings-main-rs.reference` (the legacy main.rs you saved in Task 3) for line-by-line guidance on the bus topic plumbing. The Rust API rewrites against `sola_kit::*` instead of `sola_app::*` but the bus surface is identical (CloseApp, Windows, MenuAction, MailConfig, Application).

Below is the full file. Compare each handler against the legacy original to verify behaviour parity.

```rust
//! Settings app — kit-side implementation.
//!
//! One window, two sticky-replayed topics it owns (`Application` +
//! `MailConfig`), bus-driven state push via `__solaRecv`. The
//! "running but not configured" candidates list is derived from
//! the `Windows` topic on every state push.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Value, json};
use sola_bus::topics::{
    AppMenuPayload, ApplicationsConfig, MailConfig, MailRule, MailRuleCondition,
    MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
    Window as BusWindow,
};
use sola_core::Encrypted;
use sola_core::KeyCode;
use sola_core::applications::Application;
use sola_kit::{
    AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};

use crate::procfs;

static APP_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web");

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    @dir "/" => &APP_DIR,
};

// ---------- JS command argument types ----------

#[derive(Deserialize)]
struct AddArgs {
    app_id: String,
    label: String,
    command: String,
    icon: String,
}

#[derive(Deserialize)]
struct UpdateArgs {
    old_app_id: String,
    app_id: String,
    label: String,
    command: String,
    icon: String,
}

#[derive(Deserialize)]
struct RemoveArgs {
    app_id: String,
}

#[derive(Deserialize)]
struct MailAccountArgs {
    email: String,
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct MailRuleArgs {
    name: String,
    action: String,
    #[serde(default)]
    dest: Option<String>,
    conditions: Vec<MailRuleCondition>,
}

#[derive(Deserialize)]
struct MailUpdateRuleArgs {
    index: usize,
    name: String,
    action: String,
    #[serde(default)]
    dest: Option<String>,
    conditions: Vec<MailRuleCondition>,
}

#[derive(Deserialize)]
struct MailRemoveRuleArgs {
    index: usize,
}

// ---------- App state ----------

pub struct SettingsApp {
    applications: ApplicationsConfig,
    mail: MailConfig,
    main_window: WindowHandle,
    running: Vec<BusWindow>,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let applications = ApplicationsConfig::default();
        let mail = MailConfig::default();

        let main_window = ctx.add_window(WindowConfig {
            title: "Settings".into(),
            size: (900, 620),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            zoned: true,
            keyboard_target: true,
        });

        ctx.emit(Topic::SetAppMenu(AppMenuPayload {
            app_id: Self::APP_ID.into(),
            menus: vec![
                MenuDefinition {
                    label: "Settings".into(),
                    items: vec![MenuItem::Action {
                        id: "quit".into(),
                        label: "Quit Settings".into(),
                        shortcut: Some(KeyCode::Q.meta()),
                        disabled: false,
                        checked: false,
                    }],
                },
                MenuDefinition {
                    label: "Edit".into(),
                    items: vec![
                        MenuItem::Action {
                            id: "cut".into(),
                            label: "Cut".into(),
                            shortcut: Some(KeyCode::X.meta()),
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Action {
                            id: "copy".into(),
                            label: "Copy".into(),
                            shortcut: Some(KeyCode::C.meta()),
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Action {
                            id: "paste".into(),
                            label: "Paste".into(),
                            shortcut: Some(KeyCode::V.meta()),
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Divider,
                        MenuItem::Action {
                            id: "select_all".into(),
                            label: "Select All".into(),
                            shortcut: Some(KeyCode::A.meta()),
                            disabled: false,
                            checked: false,
                        },
                    ],
                },
            ],
        }));

        tracing::info!("sola-settings ready (kit)");

        Self {
            applications,
            mail,
            main_window,
            running: Vec::new(),
        }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::CloseApp, Self::on_close_app);
        bus.on(TopicKind::Windows, Self::on_windows);
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
        bus.on(TopicKind::MailConfig, Self::on_mail_config);
        bus.on(TopicKind::Application, Self::on_application);
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
            "applications_add" => self.handle_add(args, ctx),
            "applications_update" => self.handle_update(args, ctx),
            "applications_remove" => self.handle_remove(args, ctx),
            "mail_save_account" => self.handle_mail_save_account(args, ctx),
            "mail_add_rule" => self.handle_mail_add_rule(args, ctx),
            "mail_update_rule" => self.handle_mail_update_rule(args, ctx),
            "mail_remove_rule" => self.handle_mail_remove_rule(args, ctx),
            _ => {
                tracing::warn!(cmd, "unknown command");
                json!({ "error": format!("unknown command: {cmd}") })
            }
        };

        if let Some(id) = id {
            source.send_to_js(&json!({ "id": id, "result": result }));
        }
    }
}

impl SettingsApp {
    fn on_close_app(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        if let Topic::CloseApp(app_id) = delivery.topic
            && app_id == Self::APP_ID
        {
            std::process::exit(0);
        }
    }

    fn on_menu_action(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = delivery.topic
            && app_id == Self::APP_ID
        {
            match action_id.as_str() {
                "quit" => std::process::exit(0),
                "cut" => self.main_window.cut(),
                "copy" => self.main_window.copy(),
                "paste" => self.main_window.paste(),
                "select_all" => self.main_window.select_all(),
                _ => {}
            }
        }
    }

    fn on_windows(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Windows(windows) = delivery.topic else {
            return;
        };
        self.running = windows.clone();
        self.push_state();
    }

    fn on_mail_config(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::MailConfig(cfg) = delivery.topic else {
            return;
        };
        self.mail = cfg.clone();
        self.push_state();
    }

    fn on_application(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Application(app) = delivery.topic else {
            return;
        };
        if delivery.retracted {
            self.applications.remove(&app.app_id);
        } else {
            self.applications.remove(&app.app_id);
            self.applications.apps.push(app.clone());
        }
        self.push_state();
    }

    // ---- Applications handlers ----

    fn handle_add(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: AddArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        let mut new_app = Application {
            app_id: args.app_id,
            label: args.label,
            command: args.command,
            icon: args.icon,
        };
        new_app.normalize();
        if let Err(e) = self.applications.add(new_app.clone()) {
            return json!({ "error": e.to_string() });
        }
        ctx.emit(Topic::Application(new_app));
        self.current_state()
    }

    fn handle_update(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: UpdateArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        let mut new_app = Application {
            app_id: args.app_id,
            label: args.label,
            command: args.command,
            icon: args.icon,
        };
        new_app.normalize();
        let old_app_id = args.old_app_id;
        let id_changed = old_app_id != new_app.app_id;
        let prev = self.applications.get(&old_app_id).cloned();
        if let Err(e) = self.applications.update(&old_app_id, new_app.clone()) {
            return json!({ "error": e.to_string() });
        }
        if id_changed
            && let Some(old) = prev
        {
            ctx.retract(Topic::Application(old));
        }
        ctx.emit(Topic::Application(new_app));
        self.current_state()
    }

    fn handle_remove(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: RemoveArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if let Some(removed) = self.applications.get(&args.app_id).cloned() {
            self.applications.remove(&args.app_id);
            ctx.retract(Topic::Application(removed));
        }
        self.current_state()
    }

    // ---- Mail handlers ----

    fn handle_mail_save_account(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: MailAccountArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        self.mail.email = args.email;
        self.mail.imap_host = args.imap_host;
        self.mail.imap_port = args.imap_port;
        self.mail.smtp_host = args.smtp_host;
        self.mail.smtp_port = args.smtp_port;
        self.mail.username = args.username;
        self.mail.password = Encrypted(args.password);
        ctx.emit(Topic::MailConfig(self.mail.clone()));
        self.current_state()
    }

    fn handle_mail_add_rule(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: MailRuleArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if let Err(e) = validate_rule(&args.name, &args.conditions) {
            return json!({ "error": e });
        }
        let dest = normalize_dest(&args.action, args.dest.as_deref());
        self.mail.rules.push(MailRule {
            name: args.name,
            action: args.action,
            dest,
            conditions: args.conditions,
        });
        ctx.emit(Topic::MailConfig(self.mail.clone()));
        self.current_state()
    }

    fn handle_mail_update_rule(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: MailUpdateRuleArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if args.index >= self.mail.rules.len() {
            return json!({ "error": format!("rule index {} out of range", args.index) });
        }
        if let Err(e) = validate_rule(&args.name, &args.conditions) {
            return json!({ "error": e });
        }
        let dest = normalize_dest(&args.action, args.dest.as_deref());
        self.mail.rules[args.index] = MailRule {
            name: args.name,
            action: args.action,
            dest,
            conditions: args.conditions,
        };
        ctx.emit(Topic::MailConfig(self.mail.clone()));
        self.current_state()
    }

    fn handle_mail_remove_rule(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: MailRemoveRuleArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if args.index < self.mail.rules.len() {
            self.mail.rules.remove(args.index);
            ctx.emit(Topic::MailConfig(self.mail.clone()));
        }
        self.current_state()
    }

    // ---- State plumbing ----

    fn current_state(&self) -> Value {
        state_payload(&self.applications, &self.running, &self.mail)
    }

    /// Push the latest state to the JS frontend as a `state` event.
    fn push_state(&self) {
        let mut payload = self.current_state();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("event".into(), json!("state"));
        }
        self.main_window.send_to_js(&payload);
    }
}

/// Shared validation for `mail_add_rule` and `mail_update_rule`.
fn validate_rule(
    name: &str,
    conditions: &[MailRuleCondition],
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("rule name is required".into());
    }
    if conditions.is_empty() {
        return Err("at least one condition is required".into());
    }
    Ok(())
}

fn normalize_dest(action: &str, dest: Option<&str>) -> Option<String> {
    if action != "move" {
        return None;
    }
    dest.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn mail_for_js(mail: &MailConfig) -> Value {
    json!({
        "email": mail.email,
        "imap_host": mail.imap_host,
        "imap_port": mail.imap_port,
        "smtp_host": mail.smtp_host,
        "smtp_port": mail.smtp_port,
        "username": mail.username,
        "password": mail.password.0,
        "rules": mail.rules,
    })
}

fn state_payload(
    cfg: &ApplicationsConfig,
    running: &[BusWindow],
    mail: &MailConfig,
) -> Value {
    let missing: Vec<&str> = cfg
        .apps
        .iter()
        .filter(|a| procfs::command_missing(&a.command))
        .map(|a| a.app_id.as_str())
        .collect();

    let configured: HashSet<&str> = cfg.apps.iter().map(|a| a.app_id.as_str()).collect();
    let mut seen = HashSet::new();
    let candidates: Vec<Value> = running
        .iter()
        .filter(|a| !configured.contains(a.app_id.as_str()))
        .filter(|a| !procfs::is_system_app(&a.app_id))
        .filter(|a| seen.insert(a.app_id.clone()))
        .map(|a| {
            let suggested = procfs::suggest_command(&a.app_id, a.pid);
            json!({
                "app_id": a.app_id,
                "title": a.title,
                "suggested_command": suggested,
            })
        })
        .collect();

    json!({
        "applications": {
            "apps": cfg.apps,
            "missing": missing,
            "candidates": candidates,
        },
        "mail": mail_for_js(mail),
    })
}
```

- [ ] **Step 2: Build**

```bash
cargo make build
```

Expected: clean build. If `command_missing` / `is_system_app` / `suggest_command` cause "unused" warnings in procfs.rs from Task 3, this task now uses them all.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-settings/src/app.rs
git commit -m "$(cat <<'EOF'
feat(sola-settings): port bus handlers + JS commands to kit

SettingsApp gains the Application / MailConfig / Windows handlers
and all seven JS commands from the legacy implementation. Adds
mail_update_rule alongside add/remove so the new edit-in-place rule
UI can patch rules without delete+add. Validation is shared between
add and update via validate_rule + normalize_dest.

state_payload preserved verbatim from legacy semantics — "missing"
flag via PATH check, "candidates" derived from Windows minus
configured + minus system apps.
EOF
)"
```

---

## Task 5: Settings — Applications panel

Goal: implement the `Applications` section UI in `web/panels/applications.tsx`. Uses kit primitives end-to-end.

**Files:**
- Create: `crates/sola-settings/web/panels/applications.tsx`

**Behavior** (drawn from the spec and the legacy `/tmp/sola-settings-app-ts.reference`):
- Two Cards: "Configured" + "Running, not configured".
- Configured rows are editable in place. Edits commit 500 ms after the last keystroke, via debounced `applications_update`. Errors surface inline under the row.
- A remove button per row uses `Button confirm` (`variant="danger"`) and calls `applications_remove` on confirmed click.
- `+ Add application` button at the bottom appends a draft row; first non-empty blur passing validation calls `applications_add`. While draft, the row issues no debounced updates.
- "Running, not configured" rows show `app_id`, `title`, `suggested_command` (or "command unknown" muted), and a single `Configure` button that pre-fills a new draft row in the Configured card.
- A "not found" `Badge kind="warning"` renders next to the row's label when the app is in `state.missing`.

- [ ] **Step 1: Create `crates/sola-settings/web/panels/applications.tsx`**

The panel takes a `state` slice and exposes its own draft tracking. It coordinates "Configure from candidate" through a module-level callback that the consumer can subscribe to (so the candidates card can ask the configured card to spawn a draft). Use a closure-captured ref pattern for the draft-spawn handle.

```tsx
// Applications panel. Two Cards: "Configured" (editable rows + add)
// and "Running, not configured" (candidate rows with one-click
// Configure that spawns a draft in the Configured list).
//
// Drafts are local to this panel; commit happens via the
// applications_add / applications_update JS commands. Errors come
// back as { error } from invoke and render inline.

import { type Handle } from "@remix-run/ui";
import { Badge } from "@sola/badge";
import { Button } from "@sola/button";
import { Card } from "@sola/card";
import { Field } from "@sola/field";
import { invoke } from "@sola/ipc";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";
import { TextInput } from "@sola/text-input";

interface Application {
  app_id: string;
  label: string;
  command: string;
  icon: string;
}

interface Candidate {
  app_id: string;
  title: string;
  suggested_command: string | null;
}

export interface ApplicationsState {
  apps: Application[];
  missing: string[];
  candidates: Candidate[];
}

export interface ApplicationsProps {
  state: ApplicationsState;
}

const DEBOUNCE_MS = 500;

interface DraftRow {
  app_id: string;
  label: string;
  command: string;
  icon: string;
  /** Stable client-side key — null for committed rows (we key by app_id),
      string for unsaved drafts (keeps DOM identity stable across updates). */
  draftKey: string | null;
  error: string;
}

function emptyDraft(seed?: Partial<Application>): DraftRow {
  return {
    app_id: seed?.app_id ?? "",
    label: seed?.label ?? seed?.app_id ?? "",
    command: seed?.command ?? "",
    icon: seed?.icon ?? "",
    draftKey: `draft-${Date.now()}-${Math.random()}`,
    error: "",
  };
}

export function ApplicationsPanel(handle: Handle<ApplicationsProps>) {
  // Pending drafts that haven't been committed yet. After commit,
  // the canonical row replaces the draft on the next `state` event.
  let drafts: DraftRow[] = [];

  // Per-row debounce timers, keyed by app_id (committed) or draftKey
  // (uncommitted). Cleared on commit or unmount.
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  // Per-row inline error display. Keyed by app_id or draftKey.
  const rowErrors = new Map<string, string>();

  const clearTimer = (key: string) => {
    const t = timers.get(key);
    if (t) {
      clearTimeout(t);
      timers.delete(key);
    }
  };

  const update = () => handle.update();

  const setRowError = (key: string, msg: string) => {
    if (msg) rowErrors.set(key, msg);
    else rowErrors.delete(key);
    update();
  };

  const commitDraft = async (draft: DraftRow) => {
    if (
      !draft.app_id.trim() ||
      !draft.label.trim() ||
      !draft.command.trim()
    ) {
      // Don't commit incomplete drafts silently — surface a hint.
      setRowError(
        draft.draftKey ?? "",
        "app_id, label, and command are required",
      );
      return;
    }
    setRowError(draft.draftKey ?? "", "");
    try {
      await invoke("applications_add", {
        app_id: draft.app_id.trim(),
        label: draft.label.trim(),
        command: draft.command.trim(),
        icon: draft.icon.trim(),
      });
      // Drop the draft — the canonical row will arrive via state event.
      drafts = drafts.filter((d) => d.draftKey !== draft.draftKey);
      update();
    } catch (e) {
      setRowError(draft.draftKey ?? "", String(e));
    }
  };

  const commitUpdate = async (originalAppId: string, edits: Application) => {
    setRowError(originalAppId, "");
    try {
      await invoke("applications_update", {
        old_app_id: originalAppId,
        app_id: edits.app_id.trim(),
        label: edits.label.trim(),
        command: edits.command.trim(),
        icon: edits.icon.trim(),
      });
    } catch (e) {
      setRowError(originalAppId, String(e));
    }
  };

  const scheduleUpdate = (originalAppId: string, edits: Application) => {
    clearTimer(originalAppId);
    timers.set(
      originalAppId,
      setTimeout(() => {
        timers.delete(originalAppId);
        commitUpdate(originalAppId, edits);
      }, DEBOUNCE_MS),
    );
  };

  const removeApp = async (appId: string) => {
    try {
      await invoke("applications_remove", { app_id: appId });
    } catch (e) {
      setRowError(appId, String(e));
    }
  };

  const startConfigure = (c: Candidate) => {
    drafts = [
      emptyDraft({
        app_id: c.app_id,
        label: c.app_id,
        command: c.suggested_command ?? "",
        icon: "",
      }),
      ...drafts,
    ];
    update();
  };

  const startAddBlank = () => {
    drafts = [...drafts, emptyDraft()];
    update();
  };

  const discardDraft = (key: string) => {
    drafts = drafts.filter((d) => d.draftKey !== key);
    rowErrors.delete(key);
    update();
  };

  const renderConfiguredRow = (app: Application) => {
    // Working copy — each render reads from canonical state, edits flow
    // through invoke→state push.
    const working: Application = { ...app };
    const onField = (field: keyof Application) => (v: string) => {
      working[field] = v;
      scheduleUpdate(app.app_id, working);
    };

    return (
      <Stack gap="xs">
        <Stack direction="row" gap="md" align="center">
          <Text>{app.label || app.app_id}</Text>
          {handle.props.state.missing.includes(app.app_id)
            ? <Badge kind="warning">not found</Badge>
            : null}
          <Button
            variant="danger"
            confirm
            confirmLabel="Click again to remove"
            onPress={() => removeApp(app.app_id)}
          >
            Remove
          </Button>
        </Stack>
        <Stack direction="row" gap="sm">
          <Field label="app_id">
            <TextInput
              value={app.app_id}
              onChange={onField("app_id")}
            />
          </Field>
          <Field label="label">
            <TextInput
              value={app.label}
              onChange={onField("label")}
            />
          </Field>
          <Field label="command">
            <TextInput
              value={app.command}
              onChange={onField("command")}
            />
          </Field>
          <Field label="icon">
            <TextInput
              value={app.icon}
              onChange={onField("icon")}
            />
          </Field>
        </Stack>
        {rowErrors.get(app.app_id)
          ? <Text tone="muted">{rowErrors.get(app.app_id)}</Text>
          : null}
      </Stack>
    );
  };

  const renderDraftRow = (draft: DraftRow) => {
    const onField = (field: keyof DraftRow) => (v: string) => {
      (draft as Record<string, unknown>)[field] = v;
      update();
    };

    return (
      <Stack gap="xs">
        <Stack direction="row" gap="md" align="center">
          <Text tone="muted">New application</Text>
          <Button
            variant="primary"
            onPress={() => commitDraft(draft)}
          >
            Add
          </Button>
          <Button
            variant="ghost"
            onPress={() => discardDraft(draft.draftKey!)}
          >
            Discard
          </Button>
        </Stack>
        <Stack direction="row" gap="sm">
          <Field label="app_id">
            <TextInput
              value={draft.app_id}
              onInput={onField("app_id")}
              placeholder="firefox"
            />
          </Field>
          <Field label="label">
            <TextInput
              value={draft.label}
              onInput={onField("label")}
              placeholder="Firefox"
            />
          </Field>
          <Field label="command">
            <TextInput
              value={draft.command}
              onInput={onField("command")}
              placeholder="firefox"
            />
          </Field>
          <Field label="icon">
            <TextInput
              value={draft.icon}
              onInput={onField("icon")}
              placeholder="simpleicons/firefox"
            />
          </Field>
        </Stack>
        {rowErrors.get(draft.draftKey ?? "")
          ? <Text tone="muted">{rowErrors.get(draft.draftKey ?? "")}</Text>
          : null}
      </Stack>
    );
  };

  const renderCandidate = (c: Candidate) => (
    <Stack direction="row" gap="md" align="center">
      <Stack gap="xs">
        <Text>{c.app_id}</Text>
        <Text tone="muted">
          {c.title || "(no title)"}
          {c.suggested_command
            ? ` · ${c.suggested_command}`
            : " · command unknown — fill in manually"}
        </Text>
      </Stack>
      <Button variant="ghost" onPress={() => startConfigure(c)}>
        Configure
      </Button>
    </Stack>
  );

  return () => {
    const { apps, candidates } = handle.props.state;
    return (
      <Stack gap="xl">
        <Card
          label="Configured"
          description="Edits commit half a second after the last keystroke."
        >
          <Stack gap="lg">
            {apps.length === 0 && drafts.length === 0
              ? <Text tone="muted">No applications configured.</Text>
              : null}
            {drafts.map(renderDraftRow)}
            {apps.map(renderConfiguredRow)}
            <Button variant="ghost" onPress={startAddBlank}>
              + Add application
            </Button>
          </Stack>
        </Card>
        {candidates.length > 0
          ? (
            <Card
              label="Running, not configured"
              description="Pre-filled by what's currently running. One click drops a draft into Configured."
            >
              <Stack gap="md">
                {candidates.map(renderCandidate)}
              </Stack>
            </Card>
          )
          : null}
      </Stack>
    );
  };
}
```

- [ ] **Step 2: Verify the file is well-formed by building**

```bash
cargo make build
```

(The build doesn't transform the .tsx at compile time — it just embeds it. Syntax errors will surface at runtime, but the build still validates Rust + asset path resolution.)

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-settings/web/panels/applications.tsx
git commit -m "$(cat <<'EOF'
feat(sola-settings): Applications panel in Remix v3

Edit-in-place rows with 500 ms debounced applications_update.
"Configure" button on each candidate spawns a draft in Configured.
Draft rows are not committed until the user clicks Add. Remove uses
Button confirm semantics. Inline per-row error display from invoke
rejections.
EOF
)"
```

---

## Task 6: Settings — Mail panel

Goal: implement the `Mail` section UI in `web/panels/mail.tsx`. Uses kit primitives end-to-end; uses `mail_update_rule` for edit-in-place rule updates.

**Files:**
- Create: `crates/sola-settings/web/panels/mail.tsx`

**Behavior:**
- One Card "Account" with fields for email / IMAP host / IMAP port / SMTP host / SMTP port / username / password. Explicit Save + Revert buttons; both disabled when draft equals canonical.
- One Card "Rules" with one Card-per-rule + an `+ Add rule` button.
- Each rule card has fields for name, action `PopoverSelect`, destination (only when action === "move"), and a conditions Stack.
- Each condition row has `PopoverSelect` for field, `PopoverSelect` for match, `TextInput` for value, and a confirm-style remove button.
- Each rule has Save + Discard buttons enabled only when dirty. Save calls `mail_update_rule` (existing) or `mail_add_rule` (new). Discard reverts the draft.
- Each rule has a top-right confirm-style "Remove rule" button calling `mail_remove_rule`.

- [ ] **Step 1: Create `crates/sola-settings/web/panels/mail.tsx`**

```tsx
// Mail panel. Two Cards: "Account" (explicit save/revert) and
// "Rules" (one Card per rule with inline edit + save/discard, plus
// + Add rule). The bus contract added `mail_update_rule` so a rule
// can be patched without delete+add.

import { type Handle } from "@remix-run/ui";
import { Button } from "@sola/button";
import { Card } from "@sola/card";
import { Field } from "@sola/field";
import { invoke } from "@sola/ipc";
import { NumberInput } from "@sola/number-input";
import { PopoverSelect } from "@sola/popover-select";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";
import { TextInput } from "@sola/text-input";

interface MailCondition {
  field: string;
  match: string;
  value: string;
}

interface MailRule {
  name: string;
  action: string;
  dest: string | null;
  conditions: MailCondition[];
}

export interface MailConfig {
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  username: string;
  password: string;
  rules: MailRule[];
}

export interface MailProps {
  state: MailConfig;
}

function emptyAccount(): MailConfig {
  return {
    email: "",
    imap_host: "",
    imap_port: 993,
    smtp_host: "",
    smtp_port: 587,
    username: "",
    password: "",
    rules: [],
  };
}

function emptyRule(): MailRule {
  return {
    name: "",
    action: "smart_mailbox",
    dest: "",
    conditions: [],
  };
}

function emptyCondition(): MailCondition {
  return { field: "from", match: "contains", value: "" };
}

function ruleEquals(a: MailRule, b: MailRule): boolean {
  if (a.name !== b.name || a.action !== b.action || a.dest !== b.dest) {
    return false;
  }
  if (a.conditions.length !== b.conditions.length) return false;
  for (let i = 0; i < a.conditions.length; i++) {
    const ca = a.conditions[i];
    const cb = b.conditions[i];
    if (ca.field !== cb.field || ca.match !== cb.match || ca.value !== cb.value) {
      return false;
    }
  }
  return true;
}

function accountEquals(a: MailConfig, b: MailConfig): boolean {
  return (
    a.email === b.email &&
    a.imap_host === b.imap_host &&
    a.imap_port === b.imap_port &&
    a.smtp_host === b.smtp_host &&
    a.smtp_port === b.smtp_port &&
    a.username === b.username &&
    a.password === b.password
  );
}

interface RuleDraft {
  /** -1 for new (unsaved) rules; the canonical index otherwise. */
  index: number;
  /** Stable client-side key — needed only for new rules so DOM order
      doesn't get confused if multiple new rules are being authored. */
  key: string;
  draft: MailRule;
}

const FIELD_OPTIONS = [
  { value: "from", label: "from" },
  { value: "to", label: "to" },
  { value: "subject", label: "subject" },
];

const MATCH_OPTIONS = [
  { value: "contains", label: "contains" },
  { value: "equals", label: "equals" },
  { value: "address", label: "address" },
  { value: "domain", label: "domain" },
];

const ACTION_OPTIONS = [
  { value: "smart_mailbox", label: "smart mailbox" },
  { value: "move", label: "move" },
];

export function MailPanel(handle: Handle<MailProps>) {
  // Local drafts. The account draft is a clone of the canonical state.
  let accountDraft: MailConfig = { ...handle.props.state };
  let accountError = "";

  // Per-rule drafts, addressed by canonical index (existing) or by
  // a client-side key (new unsaved rules).
  let existingDrafts = new Map<number, MailRule>();
  let newRules: RuleDraft[] = [];
  const ruleErrors = new Map<string, string>();

  // Re-sync drafts on every external state change (the on('state', …)
  // listener in main.tsx re-renders us). Drafts that diverge from
  // canonical are preserved; drafts that match canonical get refreshed.
  let lastState: MailConfig = handle.props.state;
  const syncFromState = (next: MailConfig) => {
    if (accountEquals(accountDraft, lastState)) {
      accountDraft = { ...next };
    }
    // existingDrafts: drop drafts that are clean OR refer to indices
    // that no longer exist.
    const carry = new Map<number, MailRule>();
    for (const [idx, draft] of existingDrafts) {
      if (idx >= next.rules.length) continue;
      if (!ruleEquals(draft, lastState.rules[idx])) {
        carry.set(idx, draft); // dirty draft — preserve
      }
    }
    existingDrafts = carry;
    lastState = next;
  };

  const update = () => handle.update();

  const rerender = () => {
    syncFromState(handle.props.state);
    update();
  };

  const accountDirty = () => !accountEquals(accountDraft, lastState);

  const saveAccount = async () => {
    accountError = "";
    try {
      await invoke("mail_save_account", {
        email: accountDraft.email.trim(),
        imap_host: accountDraft.imap_host.trim(),
        imap_port: accountDraft.imap_port || 993,
        smtp_host: accountDraft.smtp_host.trim(),
        smtp_port: accountDraft.smtp_port || 587,
        username: accountDraft.username.trim(),
        password: accountDraft.password,
      });
    } catch (e) {
      accountError = String(e);
      update();
    }
  };

  const revertAccount = () => {
    accountDraft = { ...lastState };
    accountError = "";
    update();
  };

  const onAccountField = <K extends keyof MailConfig>(
    field: K,
    coerce: (raw: string) => MailConfig[K] = (s) => s as MailConfig[K],
  ) =>
    (v: string) => {
      accountDraft = { ...accountDraft, [field]: coerce(v) };
      update();
    };

  const numericField = (raw: string, fallback: number): number => {
    const n = Number(raw);
    return Number.isFinite(n) && n > 0 ? n : fallback;
  };

  // Existing rule drafts: lazily created on first edit.
  const ensureDraft = (index: number): MailRule => {
    const existing = existingDrafts.get(index);
    if (existing) return existing;
    const fresh = { ...lastState.rules[index] };
    existingDrafts.set(index, fresh);
    return fresh;
  };

  const editExistingRule = (index: number, patch: Partial<MailRule>) => {
    const next = { ...ensureDraft(index), ...patch };
    existingDrafts.set(index, next);
    update();
  };

  const saveExistingRule = async (index: number) => {
    const draft = existingDrafts.get(index);
    if (!draft) return;
    try {
      await invoke("mail_update_rule", {
        index,
        name: draft.name.trim(),
        action: draft.action,
        dest: draft.action === "move" ? (draft.dest ?? "").trim() : null,
        conditions: draft.conditions.map((c) => ({
          field: c.field,
          match: c.match,
          value: c.value.trim(),
        })),
      });
      existingDrafts.delete(index);
      ruleErrors.delete(`existing-${index}`);
    } catch (e) {
      ruleErrors.set(`existing-${index}`, String(e));
    }
    update();
  };

  const discardExistingRule = (index: number) => {
    existingDrafts.delete(index);
    ruleErrors.delete(`existing-${index}`);
    update();
  };

  const removeRule = async (index: number) => {
    try {
      await invoke("mail_remove_rule", { index });
      existingDrafts.delete(index);
    } catch (e) {
      ruleErrors.set(`existing-${index}`, String(e));
    }
    update();
  };

  // New rule drafts.
  const startAddRule = () => {
    newRules = [
      ...newRules,
      {
        index: -1,
        key: `new-${Date.now()}-${Math.random()}`,
        draft: emptyRule(),
      },
    ];
    update();
  };

  const editNewRule = (key: string, patch: Partial<MailRule>) => {
    newRules = newRules.map((r) =>
      r.key === key ? { ...r, draft: { ...r.draft, ...patch } } : r,
    );
    update();
  };

  const saveNewRule = async (key: string) => {
    const entry = newRules.find((r) => r.key === key);
    if (!entry) return;
    const d = entry.draft;
    try {
      await invoke("mail_add_rule", {
        name: d.name.trim(),
        action: d.action,
        dest: d.action === "move" ? (d.dest ?? "").trim() : null,
        conditions: d.conditions.map((c) => ({
          field: c.field,
          match: c.match,
          value: c.value.trim(),
        })),
      });
      newRules = newRules.filter((r) => r.key !== key);
      ruleErrors.delete(`new-${key}`);
    } catch (e) {
      ruleErrors.set(`new-${key}`, String(e));
    }
    update();
  };

  const discardNewRule = (key: string) => {
    newRules = newRules.filter((r) => r.key !== key);
    ruleErrors.delete(`new-${key}`);
    update();
  };

  const addCondition = (
    onChange: (next: MailCondition[]) => void,
    current: MailCondition[],
  ) => {
    onChange([...current, emptyCondition()]);
  };

  const updateCondition = (
    idx: number,
    patch: Partial<MailCondition>,
    onChange: (next: MailCondition[]) => void,
    current: MailCondition[],
  ) => {
    onChange(
      current.map((c, i) => (i === idx ? { ...c, ...patch } : c)),
    );
  };

  const removeCondition = (
    idx: number,
    onChange: (next: MailCondition[]) => void,
    current: MailCondition[],
  ) => {
    onChange(current.filter((_, i) => i !== idx));
  };

  const renderConditions = (
    conditions: MailCondition[],
    onChange: (next: MailCondition[]) => void,
  ) => (
    <Stack gap="sm">
      {conditions.map((c, i) => (
        <Stack direction="row" gap="sm" align="center">
          <PopoverSelect
            options={FIELD_OPTIONS}
            value={c.field}
            onChange={(v) =>
              updateCondition(i, { field: v }, onChange, conditions)}
          />
          <PopoverSelect
            options={MATCH_OPTIONS}
            value={c.match}
            onChange={(v) =>
              updateCondition(i, { match: v }, onChange, conditions)}
          />
          <TextInput
            value={c.value}
            placeholder="value"
            onChange={(v) =>
              updateCondition(i, { value: v }, onChange, conditions)}
          />
          <Button
            variant="ghost"
            confirm
            confirmLabel="Click again"
            onPress={() => removeCondition(i, onChange, conditions)}
          >
            Remove
          </Button>
        </Stack>
      ))}
      <Button
        variant="ghost"
        onPress={() => addCondition(onChange, conditions)}
      >
        + Add condition
      </Button>
    </Stack>
  );

  const renderRuleBody = (
    rule: MailRule,
    onChange: (next: MailRule) => void,
  ) => (
    <Stack gap="md">
      <Field label="Name">
        <TextInput
          value={rule.name}
          onChange={(v) => onChange({ ...rule, name: v })}
          placeholder="rule name"
        />
      </Field>
      <Field label="Action">
        <PopoverSelect
          options={ACTION_OPTIONS}
          value={rule.action}
          onChange={(v) => onChange({ ...rule, action: v })}
        />
      </Field>
      {rule.action === "move"
        ? (
          <Field label="Destination">
            <TextInput
              value={rule.dest ?? ""}
              onChange={(v) => onChange({ ...rule, dest: v })}
              placeholder="mailbox (e.g. Trash)"
            />
          </Field>
        )
        : null}
      <Text kind="label">Conditions (all must match)</Text>
      {renderConditions(rule.conditions, (next) =>
        onChange({ ...rule, conditions: next }))}
    </Stack>
  );

  return () => {
    rerender();

    return (
      <Stack gap="xl">
        <Card
          label="Account"
          description="IMAP receive + SMTP send credentials."
        >
          <Stack gap="md">
            <Field label="Email">
              <TextInput
                type="email"
                value={accountDraft.email}
                onChange={onAccountField("email")}
              />
            </Field>
            <Field label="IMAP host">
              <TextInput
                value={accountDraft.imap_host}
                onChange={onAccountField("imap_host")}
              />
            </Field>
            <Field label="IMAP port">
              <NumberInput
                value={`${accountDraft.imap_port}`}
                unit=""
                step={1}
                min={1}
                max={65535}
                onChange={(s) =>
                  onAccountField("imap_port", (raw) =>
                    numericField(raw, 993) as MailConfig["imap_port"])(s)}
              />
            </Field>
            <Field label="SMTP host">
              <TextInput
                value={accountDraft.smtp_host}
                onChange={onAccountField("smtp_host")}
              />
            </Field>
            <Field label="SMTP port">
              <NumberInput
                value={`${accountDraft.smtp_port}`}
                unit=""
                step={1}
                min={1}
                max={65535}
                onChange={(s) =>
                  onAccountField("smtp_port", (raw) =>
                    numericField(raw, 587) as MailConfig["smtp_port"])(s)}
              />
            </Field>
            <Field label="Username">
              <TextInput
                value={accountDraft.username}
                onChange={onAccountField("username")}
              />
            </Field>
            <Field label="Password">
              <TextInput
                type="password"
                value={accountDraft.password}
                onChange={onAccountField("password")}
              />
            </Field>
            <Stack direction="row" gap="md">
              <Button
                variant="primary"
                disabled={!accountDirty()}
                onPress={saveAccount}
              >
                Save account
              </Button>
              <Button
                variant="ghost"
                disabled={!accountDirty()}
                onPress={revertAccount}
              >
                Revert
              </Button>
            </Stack>
            {accountError
              ? <Text tone="muted">{accountError}</Text>
              : null}
          </Stack>
        </Card>

        <Card
          label="Rules"
          description="Each condition row must match for the rule to fire."
        >
          <Stack gap="lg">
            {lastState.rules.length === 0 && newRules.length === 0
              ? <Text tone="muted">No rules configured.</Text>
              : null}
            {lastState.rules.map((rule, index) => {
              const draft = existingDrafts.get(index);
              const working = draft ?? rule;
              const dirty = draft !== undefined &&
                !ruleEquals(draft, rule);
              return (
                <Card label={working.name || "(unnamed rule)"}>
                  <Stack gap="md">
                    {renderRuleBody(working, (next) =>
                      editExistingRule(index, next))}
                    <Stack direction="row" gap="md">
                      <Button
                        variant="primary"
                        disabled={!dirty}
                        onPress={() => saveExistingRule(index)}
                      >
                        Save
                      </Button>
                      <Button
                        variant="ghost"
                        disabled={!dirty}
                        onPress={() => discardExistingRule(index)}
                      >
                        Discard
                      </Button>
                      <Button
                        variant="danger"
                        confirm
                        confirmLabel="Click again to remove"
                        onPress={() => removeRule(index)}
                      >
                        Remove rule
                      </Button>
                    </Stack>
                    {ruleErrors.get(`existing-${index}`)
                      ? (
                        <Text tone="muted">
                          {ruleErrors.get(`existing-${index}`)}
                        </Text>
                      )
                      : null}
                  </Stack>
                </Card>
              );
            })}
            {newRules.map((entry) => (
              <Card label={entry.draft.name || "(new rule)"}>
                <Stack gap="md">
                  {renderRuleBody(entry.draft, (next) =>
                    editNewRule(entry.key, next))}
                  <Stack direction="row" gap="md">
                    <Button
                      variant="primary"
                      onPress={() => saveNewRule(entry.key)}
                    >
                      Save
                    </Button>
                    <Button
                      variant="ghost"
                      onPress={() => discardNewRule(entry.key)}
                    >
                      Discard
                    </Button>
                  </Stack>
                  {ruleErrors.get(`new-${entry.key}`)
                    ? (
                      <Text tone="muted">
                        {ruleErrors.get(`new-${entry.key}`)}
                      </Text>
                    )
                    : null}
                </Stack>
              </Card>
            ))}
            <Button variant="ghost" onPress={startAddRule}>
              + Add rule
            </Button>
          </Stack>
        </Card>
      </Stack>
    );
  };
}
```

- [ ] **Step 2: Build**

```bash
cargo make build
```

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-settings/web/panels/mail.tsx
git commit -m "$(cat <<'EOF'
feat(sola-settings): Mail panel in Remix v3

Account card with explicit Save/Revert (passwords don't autosave).
Rules card with one Card per rule, edit-in-place via mail_update_rule
(new), and a separate "new rule" lane backed by mail_add_rule. Each
rule has Save / Discard / Remove (confirm) actions. Conditions Stack
inside each rule uses PopoverSelect for field/match selectors.
EOF
)"
```

---

## Task 7: Settings — Wire up `main.tsx` + state subscription

Goal: replace the scaffold `web/main.tsx` with the full shell that subscribes to the state event, holds section state, and routes between the two panels.

**Files:**
- Modify: `crates/sola-settings/web/main.tsx`

- [ ] **Step 1: Replace `crates/sola-settings/web/main.tsx`**

```tsx
// Settings root. Owns canonical state (single `on('state', …)`
// listener), section state, and the Sidebar / Container shell.
// Each panel reads its slice via props.

import { type Handle } from "@remix-run/ui";
import { Container } from "@sola/container";
import { on } from "@sola/ipc";
import { Root } from "@sola/root";
import { Sidebar, SidebarItem, SidebarSection } from "@sola/sidebar";
import { Split } from "@sola/split";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

import {
  ApplicationsPanel,
  type ApplicationsState,
} from "./panels/applications.tsx";
import { MailPanel, type MailConfig } from "./panels/mail.tsx";

type Section = "applications" | "mail";

interface SettingsState {
  applications: ApplicationsState;
  mail: MailConfig;
}

function emptyState(): SettingsState {
  return {
    applications: { apps: [], missing: [], candidates: [] },
    mail: {
      email: "",
      imap_host: "",
      imap_port: 993,
      smtp_host: "",
      smtp_port: 587,
      username: "",
      password: "",
      rules: [],
    },
  };
}

export function Main(handle: Handle) {
  let section: Section = "applications";
  let state: SettingsState = emptyState();

  on("state", (payload: unknown) => {
    const p = payload as Partial<SettingsState>;
    state = {
      applications: p.applications ?? state.applications,
      mail: p.mail ?? state.mail,
    };
    handle.update();
  });

  const setSection = (s: Section) => {
    if (s === section) return;
    section = s;
    handle.update();
  };

  return () => (
    <Root>
      <Split direction="row" position="240px">
        <Sidebar>
          <SidebarSection label="Settings">
            <SidebarItem
              active={section === "applications"}
              onSelect={() => setSection("applications")}
            >
              Applications
            </SidebarItem>
            <SidebarItem
              active={section === "mail"}
              onSelect={() => setSection("mail")}
            >
              Mail
            </SidebarItem>
          </SidebarSection>
        </Sidebar>
        <Container maxWidth="article">
          <Stack gap="xl">
            <Text kind="display">
              {section === "applications" ? "Applications" : "Mail"}
            </Text>
            {section === "applications"
              ? <ApplicationsPanel state={state.applications} />
              : <MailPanel state={state.mail} />}
          </Stack>
        </Container>
      </Split>
    </Root>
  );
}
```

- [ ] **Step 2: Build**

```bash
cargo make build
```

Expected: clean build. The binary at `target/debug/sola-settings` is the new kit-side settings app.

- [ ] **Step 3: Verify the panels' import paths resolve**

The asset server in sola-kit (`AssetBundle::find`) tries `.js → .tsx`. The relative imports `./panels/applications.tsx` and `./panels/mail.tsx` use explicit extensions, which the resolver tries first — and our `web/main.tsx` is at `/main.tsx`, so the relative path `./panels/applications.tsx` resolves to `/panels/applications.tsx`. The `include_dir` mount at `/` ensures both panel files are served.

No code change needed — this is a sanity-check note.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-settings/web/main.tsx
git commit -m "$(cat <<'EOF'
feat(sola-settings): wire panels into main.tsx + state subscription

Single on('state') listener owns the canonical SettingsState; panels
read their slice via props. Sidebar drives section state between
Applications and Mail. Display heading + Container limit keep the
panels comfortably readable inside the kit's Split shell.
EOF
)"
```

---

## Task 8: Final verification

**Files:** none — verification only.

- [ ] **Step 1: Full workspace build**

```bash
cargo make build
```

Expected: clean build, no warnings related to dead code in `procfs.rs` or unused fields in `SettingsApp`.

- [ ] **Step 2: Sola-kit unit tests**

```bash
cargo test -p sola-kit --lib
```

Expected: `test result: ok. 3 passed; 0 failed`. The theme snapshot covers the badge addition.

- [ ] **Step 3: Confirm the settings binary built**

```bash
ls -la target/debug/sola-settings
```

Expected: file exists, mtime recent.

- [ ] **Step 4: Note for the user**

Print the message: "Port complete. To smoke-test, run `cargo make install sola-settings` and launch `/opt/sola/bin/sola-settings` from a TTY in the running sola environment. (`install` is left to the user — the assistant does not run it.)"

- [ ] **Step 5: Final commit (only if anything is uncommitted)**

```bash
git status
```

If clean, skip. Otherwise commit any final tidy-up under a descriptive message.

---

## Self-review

**Spec coverage:**
- Decisions: replace in place ✓ (Task 3); no worktree ✓ (master); approach C — UI redesign with kit primitives ✓ (Tasks 5+6); bus contract preserved + `mail_update_rule` added ✓ (Task 4); no `initial_state` ✓ (Task 7 uses orphan-buffered `on('state')`).
- Crate layout: `main.rs` + `app.rs` + `procfs.rs` ✓ (Tasks 3+4); `web/main.tsx` + `web/panels/{applications,mail}.tsx` ✓ (Tasks 5+6+7).
- Window: 900×620, zoned, keyboard_target, decorated:false ✓ (Task 4).
- Kit additions: Badge ✓ (Task 2); confirm-on-Button ✓ (Task 1). TextInput `secret` not needed — discovered during plan writing that `type` already accepts `"password"`. Plan noted absence.
- Applications panel: edit-in-place rows, debounced commit, candidates with Configure CTA, draft rows, Badge warning ✓ (Task 5).
- Mail panel: Account with explicit save/revert, Rules with edit-in-place via `mail_update_rule` + new-rule lane via `mail_add_rule`, confirm-style remove ✓ (Task 6).

**Placeholder scan:** No TBD / TODO / "implement appropriate error handling" / "similar to Task N" sentences. Every code-changing step has the exact code to write.

**Type consistency:** `ApplicationsState`, `MailConfig`, `MailRule`, `MailCondition`, `Application`, `Candidate` interfaces are repeated where used and named consistently across files. `Section = "applications" | "mail"` is consistent. Backend `state_payload` shape mirrors the JS interfaces (`applications: { apps, missing, candidates }` + `mail: { …, rules: [{name, action, dest, conditions: [{field, match, value}]}] }`).

**Scope:** Single-app port, three independent kit additions (one elided), implementable in one plan. ✓

---

## Risks reminder for executor

- **`Theme::to_css` snapshot ordering.** The expected snapshot in `theme.rs` must match the exact ordering `to_css` produces (alphabetical by component, alphabetical by slot within a component, atoms first before components). If Task 2 Step 8's snapshot insertion doesn't match, the test surfaces the diff — paste the actual output back as the new expected.
- **JSON shape for `state` event.** Rust pushes `{ event: "state", applications: {…}, mail: {…} }`. Main.tsx reads `applications` + `mail` off `payload` (not off `payload.data` or any wrapper). If shape mismatches, the panels see `undefined` and stay empty.
- **No `initial_state`.** Bus replay → orphan buffer → drain on `on('state')` registration. The order is: Main mounts → `on('state', …)` registers → orphan-buffered events drain immediately. If a panel attempts to read state before its props arrive, it has fallback empty values.
