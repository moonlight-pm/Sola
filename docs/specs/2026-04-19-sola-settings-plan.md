# sola-settings Implementation Plan (v1: Applications)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Post-implementation note:** the plan below calls for a new crate `sola-applications`. In the final shipped refactor this was collapsed: the pure data types and CRUD live in `crates/sola-core/src/applications.rs`, and the `JsonConfigIn` impl (with `APP_DIR = "shell"`, `FILE_NAME = "applications.json"`) lives in `crates/sola-app/src/config.rs`. `sola-app` depends on `sola-core`. Read the code, not the plan, for the current layout.

**Goal:** Ship `sola-settings`, a new Sola app with a sidebar-plus-content layout whose first section edits `~/.config/sola/shell/applications.json` (add / edit / remove).

**Architecture:** New binary `apps/settings` modeled on `apps/monitor`. Extract `ApplicationsConfig` + `Application` from `apps/shell` into `crates/sola-core` (shared primitives); `JsonConfigIn` impl lives in `sola-app`. JS front-end is Arrow.js + `@sola/ipc` invoking Rust commands `applications_add`, `applications_update`, `applications_remove`. The shell is not modified beyond its import path — it already reloads `applications.json` when the launcher opens.

**Tech Stack:** Rust 2024 edition, `sola-app` framework, WebKit6 WebView, Arrow.js (`@arrow-js/core`), `@sola/ipc`, `cargo make` build system.

**Spec:** `docs/specs/2026-04-19-sola-settings-design.md`

**Worktree:** `.worktrees/sola-settings` (branch `sola-settings`)

---

## File Structure

**Created**
- `crates/sola-applications/Cargo.toml`
- `crates/sola-applications/src/lib.rs` — `Application`, `ApplicationsConfig`, CRUD methods, `DuplicateAppId` error
- `apps/settings/Cargo.toml`
- `apps/settings/src/main.rs` — `SolaApp` impl, `APP_ID = "sola-settings"`, window + JS command handlers
- `apps/settings/web/index.html`
- `apps/settings/web/src/main.ts` — Arrow.js bootstrap
- `apps/settings/web/src/app.ts` — sidebar layout + applications section UI
- `apps/settings/web/src/theme.css` — surface/text tokens (copied from monitor and reduced)

**Modified**
- `apps/shell/src/applications.rs` — deleted; contents extracted
- `apps/shell/src/main.rs` — drop `mod applications;`
- `apps/shell/src/app.rs` — import from `sola_applications::{Application, ApplicationsConfig}`
- `apps/shell/src/launcher/state.rs` — same import change
- `apps/shell/Cargo.toml` — add `sola-applications` dependency

**Touched on canto (deploy-time)**
- `~/.config/sola/shell/applications.json` — add a `sola-settings` entry so the launcher can spawn it

---

## Task 1: Extract shared `sola-applications` crate

**Files:**
- Create: `crates/sola-applications/Cargo.toml`
- Create: `crates/sola-applications/src/lib.rs`

### - [ ] Step 1: Create the crate directory and Cargo.toml

Create `crates/sola-applications/Cargo.toml`:

```toml
[package]
name = "sola-applications"
version.workspace = true
edition.workspace = true

[dependencies]
sola-app = { path = "../sola-app" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### - [ ] Step 2: Write the unit tests (TDD)

Create `crates/sola-applications/src/lib.rs` starting with only the tests:

```rust
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use sola_app::config::JsonConfigIn;

// ... types declared below ...

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Application {
        Application {
            app_id: "firefox".into(),
            label: "Firefox".into(),
            command: "firefox".into(),
            icon: "simpleicons/firefox".into(),
        }
    }

    #[test]
    fn round_trips_example_json() {
        let cfg = ApplicationsConfig { apps: vec![sample()] };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ApplicationsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.apps.len(), 1);
        assert_eq!(back.apps[0].app_id, "firefox");
    }

    #[test]
    fn missing_apps_field_defaults_to_empty() {
        let cfg: ApplicationsConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.apps.is_empty());
    }

    #[test]
    fn get_finds_by_app_id() {
        let cfg = ApplicationsConfig { apps: vec![sample()] };
        assert_eq!(cfg.get("firefox").unwrap().label, "Firefox");
        assert!(cfg.get("nope").is_none());
    }

    #[test]
    fn add_appends_new_entry() {
        let mut cfg = ApplicationsConfig::default();
        cfg.add(sample()).unwrap();
        assert_eq!(cfg.apps.len(), 1);
    }

    #[test]
    fn add_rejects_duplicate_app_id() {
        let mut cfg = ApplicationsConfig { apps: vec![sample()] };
        let err = cfg.add(sample()).unwrap_err();
        assert_eq!(err, DuplicateAppId("firefox".into()));
        assert_eq!(cfg.apps.len(), 1);
    }

    #[test]
    fn update_replaces_entry_in_place() {
        let mut cfg = ApplicationsConfig { apps: vec![sample()] };
        let new = Application {
            app_id: "firefox".into(),
            label: "Firefox ESR".into(),
            command: "firefox-esr".into(),
            icon: "simpleicons/firefox".into(),
        };
        cfg.update("firefox", new).unwrap();
        assert_eq!(cfg.apps[0].label, "Firefox ESR");
        assert_eq!(cfg.apps[0].command, "firefox-esr");
    }

    #[test]
    fn update_can_rename_app_id() {
        let mut cfg = ApplicationsConfig { apps: vec![sample()] };
        let new = Application {
            app_id: "firefox-nightly".into(),
            label: "Firefox Nightly".into(),
            command: "firefox-nightly".into(),
            icon: "simpleicons/firefox".into(),
        };
        cfg.update("firefox", new).unwrap();
        assert!(cfg.get("firefox").is_none());
        assert_eq!(cfg.get("firefox-nightly").unwrap().label, "Firefox Nightly");
    }

    #[test]
    fn update_rejects_rename_that_collides() {
        let other = Application {
            app_id: "brave".into(),
            label: "Brave".into(),
            command: "brave".into(),
            icon: "simpleicons/brave".into(),
        };
        let mut cfg = ApplicationsConfig { apps: vec![sample(), other] };
        let renamed = Application {
            app_id: "brave".into(),
            label: "Firefox".into(),
            command: "firefox".into(),
            icon: "simpleicons/firefox".into(),
        };
        let err = cfg.update("firefox", renamed).unwrap_err();
        assert_eq!(err, DuplicateAppId("brave".into()));
    }

    #[test]
    fn update_missing_returns_not_found() {
        let mut cfg = ApplicationsConfig::default();
        assert!(matches!(cfg.update("nope", sample()), Err(UpdateError::NotFound(_))));
    }

    #[test]
    fn remove_deletes_entry() {
        let mut cfg = ApplicationsConfig { apps: vec![sample()] };
        cfg.remove("firefox");
        assert!(cfg.apps.is_empty());
    }

    #[test]
    fn remove_missing_is_noop() {
        let mut cfg = ApplicationsConfig { apps: vec![sample()] };
        cfg.remove("nope");
        assert_eq!(cfg.apps.len(), 1);
    }
}
```

### - [ ] Step 3: Run tests — verify they fail to compile

Run: `cargo test -p sola-applications --lib`

Expected: compile error, "cannot find type `Application` in this scope" (and similar). This confirms the test code is wired up but the types don't yet exist.

### - [ ] Step 4: Write the types and methods

Replace the `// ... types declared below ...` placeholder in `src/lib.rs` with:

```rust
/// A launchable application known to the shell.
///
/// Used by the launcher for search+spawn, by the switcher for icon lookup,
/// and intended as the single source of truth for "applications this
/// desktop knows about."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Application {
    /// Stable identifier. Matches the `app_id` the program reports on the
    /// bus when it connects; used for icon lookups by the switcher.
    pub app_id: String,
    /// Human-readable name shown in UI; used as the search target.
    pub label: String,
    /// Command to spawn. Whitespace-split into argv; no shell interpretation.
    pub command: String,
    /// Icon reference in `"<pack>/<name>"` form (e.g. `"lucide/terminal"`).
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplicationsConfig {
    #[serde(default)]
    pub apps: Vec<Application>,
}

impl JsonConfigIn for ApplicationsConfig {
    const APP_DIR: &'static str = "shell";
    const FILE_NAME: &'static str = "applications.json";
}

/// An `app_id` that conflicts with an existing entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateAppId(pub String);

impl std::fmt::Display for DuplicateAppId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "app_id already exists: {}", self.0)
    }
}

impl std::error::Error for DuplicateAppId {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    NotFound(String),
    Duplicate(DuplicateAppId),
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "no entry with app_id: {id}"),
            Self::Duplicate(d) => write!(f, "{d}"),
        }
    }
}

impl std::error::Error for UpdateError {}

impl ApplicationsConfig {
    pub fn get(&self, app_id: &str) -> Option<&Application> {
        self.apps.iter().find(|a| a.app_id == app_id)
    }

    /// Append a new entry. Errors if `app_id` already exists.
    pub fn add(&mut self, app: Application) -> Result<(), DuplicateAppId> {
        if self.get(&app.app_id).is_some() {
            return Err(DuplicateAppId(app.app_id));
        }
        self.apps.push(app);
        Ok(())
    }

    /// Replace the entry currently under `old_app_id` with `new`.
    ///
    /// If `new.app_id != old_app_id`, the entry is renamed — fails with
    /// `Duplicate` if another entry already uses `new.app_id`.
    pub fn update(&mut self, old_app_id: &str, new: Application) -> Result<(), UpdateError> {
        let idx = self
            .apps
            .iter()
            .position(|a| a.app_id == old_app_id)
            .ok_or_else(|| UpdateError::NotFound(old_app_id.to_string()))?;
        if new.app_id != old_app_id
            && self.apps.iter().any(|a| a.app_id == new.app_id)
        {
            return Err(UpdateError::Duplicate(DuplicateAppId(new.app_id)));
        }
        self.apps[idx] = new;
        Ok(())
    }

    /// Remove the entry with `app_id` if present. No-op if absent.
    pub fn remove(&mut self, app_id: &str) {
        self.apps.retain(|a| a.app_id != app_id);
    }
}
```

Then delete the `#![allow(dead_code)]` line at the top — nothing is dead now.

### - [ ] Step 5: Run tests — verify they pass

Run: `cargo test -p sola-applications --lib`

Expected: 10 tests pass.

### - [ ] Step 6: Commit

```bash
git add crates/sola-applications/
git commit -m "feat(sola-applications): extract Applications config into shared crate"
```

---

## Task 2: Point `apps/shell` at the shared crate

**Files:**
- Delete: `apps/shell/src/applications.rs`
- Modify: `apps/shell/src/main.rs:2` (remove `mod applications;`)
- Modify: `apps/shell/src/app.rs:13` (change use path)
- Modify: `apps/shell/src/launcher/state.rs:1` (change use path)
- Modify: `apps/shell/Cargo.toml` (add dep)

### - [ ] Step 1: Add the dependency

Edit `apps/shell/Cargo.toml`, inserting under `[dependencies]` alongside `sola-app`:

```toml
sola-applications = { path = "../../crates/sola-applications" }
```

### - [ ] Step 2: Update imports in `app.rs`

In `apps/shell/src/app.rs`, replace:

```rust
use crate::applications::{Application, ApplicationsConfig};
```

with:

```rust
use sola_applications::{Application, ApplicationsConfig};
```

### - [ ] Step 3: Update imports in `launcher/state.rs`

In `apps/shell/src/launcher/state.rs`, replace:

```rust
use crate::applications::{Application, ApplicationsConfig};
```

with:

```rust
use sola_applications::{Application, ApplicationsConfig};
```

### - [ ] Step 4: Drop the shell's `applications` module

In `apps/shell/src/main.rs`, remove the line `mod applications;`.

Then delete the file `apps/shell/src/applications.rs`.

```bash
rm apps/shell/src/applications.rs
```

### - [ ] Step 5: Verify shell builds and tests pass

Run: `cargo check -p sola-shell`

Expected: clean build.

Run: `cargo test -p sola-shell --lib`

Expected: existing launcher tests pass.

### - [ ] Step 6: Commit

```bash
git add apps/shell/
git commit -m "refactor(shell): use sola-applications shared crate"
```

---

## Task 3: Scaffold `apps/settings` binary

**Files:**
- Create: `apps/settings/Cargo.toml`
- Create: `apps/settings/src/main.rs` (skeleton — opens empty window)
- Create: `apps/settings/web/index.html`
- Create: `apps/settings/web/src/main.ts`
- Create: `apps/settings/web/src/app.ts` (stub — renders "Settings" text)
- Create: `apps/settings/web/src/theme.css`

This task gets a window on screen so later tasks can iterate on UI without fighting bootstrap bugs.

### - [ ] Step 1: Create `apps/settings/Cargo.toml`

```toml
[package]
name = "sola-settings"
version.workspace = true
edition.workspace = true

[[bin]]
name = "sola-settings"
path = "src/main.rs"

[dependencies]
sola-app = { path = "../../crates/sola-app" }
sola-applications = { path = "../../crates/sola-applications" }
sola-bus = { path = "../../crates/sola-bus" }
sola-core = { path = "../../crates/sola-core" }
gtk4 = "0.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
```

### - [ ] Step 2: Create `apps/settings/web/index.html`

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>sola-settings</title>
  <link rel="stylesheet" href="/src/theme.css">
  <script>window.RESTORED_STATE = __RESTORED_STATE__;</script>
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

### - [ ] Step 3: Create `apps/settings/web/src/main.ts`

```ts
import { createApp } from './app.js';

createApp(document.getElementById('app')!).catch((e) => {
  document.title = 'app-error:' + String(e);
  console.error('[sola-settings] createApp failed:', e);
});
```

### - [ ] Step 4: Create `apps/settings/web/src/theme.css`

```css
@import url('https://fonts.googleapis.com/css2?family=DM+Sans:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap');

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

  --cyan: #00d4ff;
  --cyan-dim: rgba(0, 212, 255, 0.12);
  --red: #f85149;
  --green: #3fb950;

  --font-mono: 'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace;
  --font-sans: 'DM Sans', system-ui, sans-serif;
}

* { box-sizing: border-box; }

html, body {
  margin: 0;
  padding: 0;
  height: 100%;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: var(--font-sans);
  font-size: 13px;
  overflow: hidden;
}

#app { height: 100%; }
```

### - [ ] Step 5: Create stub `apps/settings/web/src/app.ts`

```ts
import { html, reactive } from '@arrow-js/core';

export async function createApp(root: HTMLElement): Promise<void> {
  const state = reactive({ ready: true });
  html`<div style="padding: 20px;">Settings — loading…${() => (state.ready ? ' ready' : '')}</div>`(root);
}
```

### - [ ] Step 6: Create skeleton `apps/settings/src/main.rs`

```rust
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_applications::ApplicationsConfig;
use sola_app::config::JsonConfigIn;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
};

struct SettingsApp {
    #[allow(dead_code)]
    main_window: WindowHandle,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let applications = ApplicationsConfig::load();
        let initial_state = serde_json::json!({ "apps": applications.apps });
        let initial_state = serde_json::to_string(&initial_state).unwrap_or_default();

        let main_window = ctx.add_window(WindowConfig {
            title: "Settings".into(),
            size: (760, 560),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: Some(initial_state),
            zoned: true,
            keyboard_target: true,
        });

        tracing::info!("sola-settings ready");

        Self { main_window }
    }
}

fn main() {
    sola_app::run::<SettingsApp>();
}
```

### - [ ] Step 7: Verify it compiles

Run: `cargo check -p sola-settings`

Expected: clean build.

### - [ ] Step 8: Commit

```bash
git add apps/settings/
git commit -m "feat(settings): scaffold sola-settings app (empty window)"
```

---

## Task 4: Build the Applications section UI

**Files:**
- Modify: `apps/settings/web/src/app.ts` — full sidebar + applications list UI
- Modify: `apps/settings/web/src/theme.css` — add component styles

This task replaces the stub `app.ts` with the full UI, which only reads state. The `invoke()` calls wired in Task 5 will mutate and re-render.

### - [ ] Step 1: Replace `apps/settings/web/src/app.ts`

```ts
import { html, reactive } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';

interface Application {
  app_id: string;
  label: string;
  command: string;
  icon: string;
}

interface RestoredState { apps: Application[] }

const state = reactive({
  section: 'applications' as 'applications',
  apps: [] as Application[],
  editing: null as string | null,
  adding: false,
  form: { app_id: '', label: '', command: '', icon: '' },
  error: '',
});

function startAdd() {
  state.adding = true;
  state.editing = null;
  state.form = { app_id: '', label: '', command: '', icon: '' };
  state.error = '';
}

function startEdit(app: Application) {
  state.editing = app.app_id;
  state.adding = false;
  state.form = { ...app };
  state.error = '';
}

function cancel() {
  state.editing = null;
  state.adding = false;
  state.error = '';
}

function validate(): string {
  const f = state.form;
  if (!f.app_id.trim()) return 'app_id is required';
  if (!f.label.trim()) return 'label is required';
  if (!f.command.trim()) return 'command is required';
  return '';
}

async function submitAdd() {
  const err = validate();
  if (err) { state.error = err; return; }
  try {
    const next = await invoke('applications_add', {
      app_id: state.form.app_id.trim(),
      label: state.form.label.trim(),
      command: state.form.command.trim(),
      icon: state.form.icon.trim(),
    }) as Application[];
    state.apps = next;
    state.adding = false;
    state.error = '';
  } catch (e) {
    state.error = String(e);
  }
}

async function submitUpdate(oldAppId: string) {
  const err = validate();
  if (err) { state.error = err; return; }
  try {
    const next = await invoke('applications_update', {
      old_app_id: oldAppId,
      app_id: state.form.app_id.trim(),
      label: state.form.label.trim(),
      command: state.form.command.trim(),
      icon: state.form.icon.trim(),
    }) as Application[];
    state.apps = next;
    state.editing = null;
    state.error = '';
  } catch (e) {
    state.error = String(e);
  }
}

async function removeApp(app_id: string) {
  try {
    const next = await invoke('applications_remove', { app_id }) as Application[];
    state.apps = next;
  } catch (e) {
    state.error = String(e);
  }
}

function renderRow(app: Application) {
  return html`
    <div class="row">
      ${() => state.editing === app.app_id
        ? renderForm(() => submitUpdate(app.app_id))
        : html`
          <div class="row-info">
            <span class="row-label">${() => app.label}</span>
            <span class="row-detail">${() => app.app_id} · ${() => app.command}</span>
          </div>
          <div class="row-actions">
            <button class="btn-text" @click="${() => startEdit(app)}">Edit</button>
            <button class="btn-text danger" @click="${() => removeApp(app.app_id)}">Remove</button>
          </div>
        `}
    </div>
  `;
}

function renderForm(onSave: () => void) {
  return html`
    <div class="form">
      <input class="field" placeholder="app_id (e.g. firefox)"
        @input="${(e: Event) => state.form.app_id = (e.target as HTMLInputElement).value}"
        value="${() => state.form.app_id}" />
      <input class="field" placeholder="label (e.g. Firefox)"
        @input="${(e: Event) => state.form.label = (e.target as HTMLInputElement).value}"
        value="${() => state.form.label}" />
      <input class="field" placeholder="command (e.g. firefox)"
        @input="${(e: Event) => state.form.command = (e.target as HTMLInputElement).value}"
        value="${() => state.form.command}" />
      <input class="field" placeholder="icon (e.g. simpleicons/firefox)"
        @input="${(e: Event) => state.form.icon = (e.target as HTMLInputElement).value}"
        value="${() => state.form.icon}" />
      ${() => state.error ? html`<span class="error">${() => state.error}</span>` : ''}
      <div class="form-actions">
        <button class="btn primary" @click="${onSave}">Save</button>
        <button class="btn" @click="${cancel}">Cancel</button>
      </div>
    </div>
  `;
}

function renderApplications() {
  return html`
    <div class="section">
      <h2>Applications</h2>
      <p class="section-desc">Entries in <code>~/.config/sola/shell/applications.json</code>. The launcher reloads them each time it opens.</p>
      <div class="list">
        ${() => state.apps.map((app) => renderRow(app))}
      </div>
      ${() => state.adding
        ? renderForm(submitAdd)
        : html`<button class="btn add" @click="${startAdd}">+ Add application</button>`}
    </div>
  `;
}

function renderSidebar() {
  return html`
    <nav class="sidebar">
      <div class="sidebar-title">Settings</div>
      <button class="nav-item active">Applications</button>
    </nav>
  `;
}

export async function createApp(root: HTMLElement): Promise<void> {
  const restored = (window as unknown as { RESTORED_STATE?: RestoredState }).RESTORED_STATE;
  state.apps = restored?.apps ?? [];

  // Future: respond to state pushes from Rust if we ever send unsolicited updates.
  on('state', (payload: unknown) => {
    const p = payload as Partial<RestoredState>;
    if (Array.isArray(p.apps)) state.apps = p.apps;
  });

  html`
    <div class="layout">
      ${renderSidebar()}
      <main class="content">
        ${() => state.section === 'applications' ? renderApplications() : ''}
      </main>
    </div>
  `(root);
}
```

### - [ ] Step 2: Append component styles to `apps/settings/web/src/theme.css`

Add to the end of the file:

```css
.layout {
  display: flex;
  height: 100%;
}

.sidebar {
  width: 200px;
  flex-shrink: 0;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-subtle);
  padding: 16px 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.sidebar-title {
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--text-muted);
  padding: 4px 10px 10px;
}

.nav-item {
  background: none;
  border: none;
  text-align: left;
  color: var(--text-secondary);
  padding: 8px 10px;
  border-radius: 4px;
  font: inherit;
  cursor: pointer;
}

.nav-item:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.nav-item.active {
  background: var(--cyan-dim);
  color: var(--cyan);
}

.content {
  flex: 1;
  overflow: auto;
  padding: 24px 28px;
}

.section h2 {
  margin: 0 0 4px;
  font-size: 16px;
  font-weight: 600;
}

.section-desc {
  margin: 0 0 20px;
  font-size: 12px;
  color: var(--text-tertiary);
}

.section-desc code {
  font-family: var(--font-mono);
  color: var(--text-secondary);
}

.list {
  display: flex;
  flex-direction: column;
  gap: 1px;
  margin-bottom: 12px;
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  background: var(--bg-secondary);
  border-radius: 6px;
}

.row-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.row-label {
  font-size: 13px;
  font-weight: 500;
}

.row-detail {
  font-size: 11px;
  color: var(--text-tertiary);
  font-family: var(--font-mono);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.btn-text {
  background: none;
  border: none;
  color: var(--text-secondary);
  font-size: 11px;
  padding: 4px 8px;
  border-radius: 4px;
  cursor: pointer;
  font-family: inherit;
}

.btn-text:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.btn-text.danger:hover {
  color: var(--red);
}

.form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  background: var(--bg-secondary);
  border-radius: 6px;
  margin-bottom: 12px;
}

.field {
  width: 100%;
  padding: 6px 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 4px;
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 12px;
  outline: none;
}

.field:focus { border-color: var(--cyan); }

.error {
  font-size: 11px;
  color: var(--red);
}

.form-actions {
  display: flex;
  gap: 8px;
}

.btn {
  padding: 5px 14px;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}

.btn.primary {
  background: var(--cyan-dim);
  color: var(--cyan);
}

.btn.add {
  width: 100%;
  padding: 10px;
  border: 1px dashed var(--border-subtle);
  background: none;
  color: var(--text-secondary);
}

.btn.add:hover {
  border-color: var(--cyan);
  color: var(--cyan);
}
```

### - [ ] Step 3: Verify it compiles (assets embedded via include_str)

Run: `cargo check -p sola-settings`

Expected: clean build.

### - [ ] Step 4: Commit

```bash
git add apps/settings/
git commit -m "feat(settings): applications section UI (list + form)"
```

---

## Task 5: Wire JS commands in Rust

**Files:**
- Modify: `apps/settings/src/main.rs`

All three commands follow the same pattern: mutate the in-memory `ApplicationsConfig`, call `.save()`, reply with the new list. On error, reply with `{error: "..."}`.

### - [ ] Step 1: Replace `apps/settings/src/main.rs`

```rust
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sola_app::config::JsonConfigIn;
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_applications::{Application, ApplicationsConfig};

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
};

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

#[derive(Serialize)]
struct ErrorReply {
    error: String,
}

struct SettingsApp {
    applications: ApplicationsConfig,
    main_window: WindowHandle,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let applications = ApplicationsConfig::load();
        let initial_state = serde_json::to_string(&json!({
            "apps": applications.apps,
        }))
        .unwrap_or_default();

        let main_window = ctx.add_window(WindowConfig {
            title: "Settings".into(),
            size: (760, 560),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: Some(initial_state),
            zoned: true,
            keyboard_target: true,
        });

        tracing::info!("sola-settings ready");

        Self { applications, main_window }
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        source: &WindowHandle,
        _ctx: &mut AppCtx,
    ) {
        let result = match cmd {
            "applications_add" => self.handle_add(args),
            "applications_update" => self.handle_update(args),
            "applications_remove" => self.handle_remove(args),
            _ => {
                tracing::warn!(cmd, "unknown command");
                return;
            }
        };

        if let Some(id) = id {
            let payload = match result {
                Ok(apps) => json!({ "id": id, "result": apps }),
                Err(msg) => json!({ "id": id, "error": msg }),
            };
            source.send_to_js(&payload);
        }
    }
}

impl SettingsApp {
    fn handle_add(&mut self, args: &Value) -> Result<Vec<Application>, String> {
        let args: AddArgs = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
        self.applications
            .add(Application {
                app_id: args.app_id,
                label: args.label,
                command: args.command,
                icon: args.icon,
            })
            .map_err(|e| e.to_string())?;
        self.applications.save();
        Ok(self.applications.apps.clone())
    }

    fn handle_update(&mut self, args: &Value) -> Result<Vec<Application>, String> {
        let args: UpdateArgs = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
        self.applications
            .update(
                &args.old_app_id,
                Application {
                    app_id: args.app_id,
                    label: args.label,
                    command: args.command,
                    icon: args.icon,
                },
            )
            .map_err(|e| e.to_string())?;
        self.applications.save();
        Ok(self.applications.apps.clone())
    }

    fn handle_remove(&mut self, args: &Value) -> Result<Vec<Application>, String> {
        let args: RemoveArgs = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
        self.applications.remove(&args.app_id);
        self.applications.save();
        Ok(self.applications.apps.clone())
    }
}

fn main() {
    sola_app::run::<SettingsApp>();
}
```

Notes on the changes from Task 3's skeleton:
- Added `applications` field so state persists across commands.
- Added three command handlers dispatched in `on_js_command`.
- Error replies use `{id, error}`; `@sola/ipc` `invoke()` rejects on `error` keys (confirmed in how other apps use the protocol — see `apps/browser/src/app.rs:153`, where success uses `{id, result}` and errors use `{id, error}`). If `@sola/ipc` rejects only on `error`, the catch blocks in `app.ts` receive the string.
- `main_window` field kept to match the existing pattern (unused `#[allow(dead_code)]` attribute no longer needed since it's held on the struct normally).

### - [ ] Step 2: Verify `@sola/ipc` rejection shape

Open `crates/sola-app/src/bridge.rs` and `crates/sola-app/lib/ipc.js` (or wherever the JS-side `invoke` lives). Skim for the error/reject shape. If rejection requires a specific key other than `error`, adjust the Rust side.

Run:

```bash
grep -rn "error\|reject" crates/sola-app/src/bridge.rs | head -20
```

Expected: confirms the reply envelope convention. If the convention is `{id, result}` on success and the JS rejects when `result === undefined || error !== undefined`, we're aligned. If the shape differs, fix the Rust side and re-check.

### - [ ] Step 3: Build and test

Run: `cargo check -p sola-settings` → expect clean.

Run: `cargo test -p sola-applications --lib` → expect 10 passing (sanity re-check).

### - [ ] Step 4: Commit

```bash
git add apps/settings/src/main.rs
git commit -m "feat(settings): wire applications CRUD commands to shared config"
```

---

## Task 6: Workspace-wide build sanity

**Files:** none modified.

### - [ ] Step 1: Build the whole workspace

Run: `cargo check --workspace`

Expected: clean build for all crates. Any new warning lives under `apps/settings` or `crates/sola-applications`; address before moving on.

### - [ ] Step 2: Run all tests

Run: `cargo test --workspace --lib`

Expected: all tests pass. Specifically includes `sola-applications` (10 tests) and `sola-shell` launcher tests.

### - [ ] Step 3: Confirm sola-make resolves the new app

Run: `cargo run -q -p sola-make -- build settings`

Expected: resolves `settings` → `sola-settings` and builds it in debug mode. (This exercises `resolve_crate_name` in `crates/sola-make/src/main.rs:155`.)

### - [ ] Step 4: Commit (no-op if nothing changed)

If any warnings surfaced and were fixed:

```bash
git add -A
git commit -m "chore(settings): address warnings from workspace build"
```

Otherwise skip.

---

## Task 7: Deploy and manual verification

**Only run after user grants deploy permission** (per `CLAUDE.md`).

### - [ ] Step 1: Ask for deploy permission

Pause here and confirm with the user: *"Ready to deploy `sola-settings` to canto. Proceed?"*

### - [ ] Step 2: Deploy

Run: `cargo make deploy settings --canto`

Expected: release build + rsync `sola-settings` binary to `/opt/sola/bin/` on canto.

### - [ ] Step 3: Add the launcher entry on canto

SSH to canto and append `sola-settings` to `~/.config/sola/shell/applications.json`:

```bash
ssh canto 'python3 -c "
import json, pathlib
p = pathlib.Path.home() / \".config/sola/shell/applications.json\"
cfg = json.loads(p.read_text())
if not any(a[\"app_id\"] == \"sola-settings\" for a in cfg[\"apps\"]):
    cfg[\"apps\"].append({
        \"app_id\": \"sola-settings\",
        \"label\": \"Settings\",
        \"command\": \"/opt/sola/bin/sola-settings\",
        \"icon\": \"lucide/settings\",
    })
    p.write_text(json.dumps(cfg, indent=2))
print(\"ok\")
"'
```

Expected: prints `ok`. Subsequent runs are idempotent.

### - [ ] Step 4: Verify end-to-end

Ask the user to:
1. On canto, open the launcher (usual shortcut) — confirm "Settings" appears.
2. Launch it — confirm a window opens titled "Settings" with a sidebar and an Applications list populated with current entries.
3. Add a test entry, close and reopen the launcher — confirm the new entry appears.
4. Edit an entry (change its label), close and reopen the launcher — confirm the label updated.
5. Remove the test entry — confirm it's gone from the next launcher open.
6. Inspect `~/.config/sola/shell/applications.json` on canto — confirm it's pretty-printed and well-formed.

If any step fails, inspect `/opt/sola/log/sola.log` on canto for tracing output.

### - [ ] Step 5: Commit any follow-up fixes, then stop

Merge only on explicit user request (per `CLAUDE.md` worktree rules).

---

## Self-Review

Against the spec `docs/specs/2026-04-19-sola-settings-design.md`:

- **Crate layout** — Task 3 creates `apps/settings/` mirroring `apps/monitor/` ✓
- **Shared `ApplicationsConfig`** via new `crates/sola-applications` — Task 1 + Task 2 ✓
- **Sidebar-plus-content layout** — Task 4 renders a sidebar and content pane even though only one section exists ✓
- **Add / edit / remove** — Task 4 (UI) + Task 5 (Rust handlers) ✓
- **Atomic save via `JsonConfigIn`** — `ApplicationsConfig::save()` already does tempfile+rename ✓
- **Shell picks up edits on launcher open** — no shell changes beyond imports; the existing reload-on-open logic in `apps/shell/src/app.rs:863` is untouched ✓
- **Entry added to applications.json on canto** — Task 7 step 3 ✓
- **Out-of-scope confirmations:** no reorder, no icon picker, no detect-running, no live shell-refresh bus topic, no validation beyond non-empty required fields ✓

Placeholder scan: no TBD / TODO / fill-in; every code-changing step shows code.

Type consistency: `ApplicationsConfig::add/update/remove` signatures in Task 1's implementation match the tests (Task 1) and the Rust handlers (Task 5). JS command names (`applications_add`, `applications_update`, `applications_remove`) match between `app.ts` (Task 4) and `main.rs` (Task 5). Argument shapes (`{app_id, label, command, icon}` and `old_app_id` for update) line up between JS and Rust.
