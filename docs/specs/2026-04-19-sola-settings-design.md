# sola-settings — Design (v1: Applications)

## Purpose

A dedicated settings app for the Sola desktop. First cut ships an **Applications** section that edits `~/.config/sola/shell/applications.json`, which the shell uses for the launcher, switcher, and session reconciliation. Subsequent sections (keybindings, zones, panel, theme, …) will plug into the same app.

## Scope (v1)

**In scope**
- New binary `sola-settings` at `apps/settings/`, pattern-matched to `apps/monitor/`.
- Sidebar-plus-content layout with a single section: **Applications**.
- Applications section supports **add**, **edit**, **remove** of entries with fields `{app_id, label, command, icon}`.
- On any change, the app writes the full `applications.json` atomically (via existing `JsonConfigIn` plumbing).
- The shell picks up edits on its normal reload point (launcher open) — no new bus plumbing.
- Entry for `sola-settings` added to the default `applications.json` so the launcher can spawn it.

**Out of scope (deferred)**
- Reorder of entries.
- Icon picker / icon browsing.
- Detect-running-apps helper.
- Command-exists / `$PATH` validation.
- Live push to the shell via a new bus topic (can be added later additively).
- Any non-Applications section.

## Architecture

### Process model

`sola-settings` is a standalone Sola app: a Rust host (using `sola-app`) that owns one WebKit6 WebView loading an embedded asset bundle. It is a bus client like any other app. The Rust side:

- Loads the existing `ApplicationsConfig` (from `apps/shell/src/applications.rs`) on startup.
- Exposes JS commands for CRUD and a save.
- Saves via `ApplicationsConfig::save()` (atomic write already implemented by `JsonConfigIn`).

The shell is untouched. It already reloads `applications.json` when the launcher opens, so edits propagate on next launcher invocation.

### Shared type

`ApplicationsConfig` and `Application` already live in `apps/shell/src/applications.rs`. To avoid duplication, move these into a small shared crate consumed by both `apps/shell` and `apps/settings`:

- **New crate**: `crates/sola-applications/` with `ApplicationsConfig`, `Application`, and the `JsonConfigIn` impl.
- `apps/shell` re-points its `use` to `sola_applications::{ApplicationsConfig, Application}`.
- `apps/settings` depends on the same crate.

This keeps "what an application record looks like" in one place — the kind of shared vocabulary that belongs outside any one app.

### Web frontend

Matches `apps/monitor/web/`:

- Arrow.js (`@arrow-js/core`) for reactive rendering.
- `@sola/ipc` `invoke` for Rust ↔ JS calls.
- No router, no framework — one `app.ts` renders the whole UI.

**Layout**
- Left: fixed-width sidebar with a vertical list of sections. v1 has one item (`Applications`) selected by default. Sidebar is rendered in-structure for future sections even though only one exists — avoids a cosmetic refactor later.
- Right: content pane. For Applications, a list of entries with inline edit/remove, plus an "Add application" button that reveals an add-form row.

**Applications UI**

Each row shows `label` (primary), `app_id` (secondary, monospaced), and `command` (secondary, monospaced). Clicking **Edit** swaps the row into a four-field form (`app_id`, `label`, `command`, `icon`) with **Save** / **Cancel**. Clicking **Remove** deletes immediately (no confirm dialog — consistent with Cogsworth's editor; revert is "re-add"). **Add application** reveals a form at the bottom of the list.

Validation is minimal: `app_id`, `label`, and `command` are required (non-empty after trim). `icon` is optional. `app_id` must be unique; duplicate add/edit is rejected in the Rust handler with an error returned to JS and shown inline.

### Rust ↔ JS commands

All commands use `sola-app`'s existing JS command mechanism (same as `save_sidebar_width` in monitor). The window receives `initial_state` containing the loaded list; live reloads after mutation are done by sending a fresh state message to JS.

| Command               | Args                                          | Behavior                                                       |
|-----------------------|-----------------------------------------------|----------------------------------------------------------------|
| `applications_add`    | `{ app_id, label, command, icon }`            | Push entry; save; push new list to JS. Error on duplicate id.  |
| `applications_update` | `{ old_app_id, app_id, label, command, icon }`| Replace entry by `old_app_id`; save; push. Error on missing / duplicate id. |
| `applications_remove` | `{ app_id }`                                  | Remove; save; push.                                            |

On startup the window's `initial_state` is `{ apps: [...] }`, consumed by `window.RESTORED_STATE` as monitor does.

### Window config

```rust
WindowConfig {
    title: "Settings".into(),
    size: (760, 560),
    position: None,
    decorated: false,
    transparent: false,
    assets: APP_ASSETS,
    initial_state: Some(initial_state),
    zoned: true,
    keyboard_target: true,
}
```

`zoned: true` so it behaves like a normal app window in the shell's zoning model (monitor is `zoned: false` because it's an overlay; settings is a regular app).

### Menu

A minimal app menu with **Quit Settings** (Meta+Q), matching monitor.

## File I/O

`ApplicationsConfig::save()` already performs atomic write (tempfile + rename) and creates the parent dir if missing. Nothing new needed.

No reload-on-change file watching in v1 — only `sola-settings` writes this file while it's open, and it holds the canonical in-memory copy.

## Build / deploy

- New workspace member in top-level `Cargo.toml`: `apps/settings`.
- New workspace member: `crates/sola-applications`.
- `sola-make` already enumerates `apps/*` for deploy — verify that `cargo make deploy settings` works after adding the crate, and add it explicitly if required.
- Add `sola-settings` to the default `applications.json` entry list so it appears in the launcher.

## Non-goals and explicit deferrals

- **Immediate shell refresh.** Not doing in v1. If the "next launcher open" delay becomes annoying, add a `Topic::ApplicationsChanged` sticky bus event emitted by settings and subscribed by the shell.
- **Icon picker.** Non-trivial (thousands of icons across packs, search, preview). A freeform `"<pack>/<name>"` text field is fine for v1.
- **Detect running apps.** Useful but requires watching `Topic::Apps` on the bus and surfacing unknown `app_id`s — explicitly deferred.
- **Reorder.** The launcher's ordering is not yet user-visible in a way that motivates manual reordering.

## Testing

- Unit test in `sola-applications`: `ApplicationsConfig` round-trip already exists; keep it.
- Rust handler tests for add/update/remove including duplicate-id rejection.
- Manual verification on canto: deploy, open the launcher, launch `sola-settings`, add/edit/remove an entry, close and reopen the launcher, confirm the change is visible.
