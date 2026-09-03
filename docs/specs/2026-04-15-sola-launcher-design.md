# Sola Launcher — Design

**Status:** Approved 2026-04-15
**Scope:** Add a Meta+Space application launcher to the shell, a new `sola-assets` crate for shared icon packs, an `applications.json` registry shared across shell features, and a `LaunchApp` bus topic handled by `sola`.

## Goals

- Meta+Space opens a search-driven launcher; typing filters a user-curated list; Enter launches, Escape dismisses, focus restores.
- The set of launchable applications is hand-edited in `<config>/sola/shell/applications.json` today and accessible to a future config app tomorrow.
- Icons (lucide, simpleicons) are deployed once as shared data on disk and available to any Sola WebView via a `sola-assets://` URI scheme.
- Shell owns desktop state (what applications exist, what should be running); `sola` performs process management (fork/exec, reap). Communication is over the bus.

## Non-goals

- Searching `/usr/share/applications/.desktop` files or any system application index.
- Shell-interpreted commands (pipes, redirection, env var substitution). Commands are argv, split on whitespace.
- PID tracking, child supervision, or session restore. `sola` spawns, waits on SIGCHLD, logs exit — nothing more for now.
- Launcher keyboard shortcuts beyond arrows, Enter, Escape, and Meta+Space.
- A UI for editing `applications.json`. Hand-edited for now.

## Overview

Three additions and one extension:

1. **`crates/sola-assets/`** — new crate. Owns vendored icon packs under `assets/icons/<pack>/<name>.svg` and registers a `sola-assets://` WebKit URI scheme that serves them from `/opt/sola/share/` (deployed) or the workspace (dev). Includes a `cargo make assets pull` subcommand to refresh pinned upstreams.
2. **`Application` + `ApplicationsConfig`** — a typed list of known applications loaded by the shell from `<config>/sola/shell/applications.json`. Shared across shell features (launcher renders it, switcher resolves icons from it).
3. **Shell launcher window** — new `launcher` WebView under `apps/shell/src/launcher/` + `apps/shell/web/launcher.{html,ts}`, mirroring the switcher layout. Keyboard-target on open.
4. **`Topic::LaunchApp`** — new bus topic. Shell emits; `sola` subscribes and spawns.

An extension to `sola-app::config` lets apps own a sub-directory under `<config>/sola/<app_dir>/`.

## Architecture

```
┌────────────────────────────────┐        ┌──────────────────────┐
│ shell (ShellApp)               │        │ sola (process mgr)   │
│ ┌──────────────────────────┐   │        │                      │
│ │ ApplicationsConfig       │   │        │                      │
│ │   apps: Vec<Application> │   │        │                      │
│ └──────────────────────────┘   │        │                      │
│                                │  LaunchApp { command }        │
│ launcher window ──┐            │ ──────────────────────────▶   │
│ switcher window ──┤ icon_for() │        │   Command::spawn     │
│                   │            │        │   wait on SIGCHLD    │
│                   └── Application lookup │   log(pid, exit)    │
└────────────────────────────────┘        └──────────────────────┘
         │                                         │
         └───────── sola-bus (unix socket) ────────┘

         WebViews (any crate)
          │
          └── sola-assets:// URI scheme ──▶ /opt/sola/share/icons/<pack>/<name>.svg
                                              (dev: workspace crates/sola-assets/assets/icons/)
```

## Crate & file layout

### New crate: `crates/sola-assets/`

```
crates/sola-assets/
  Cargo.toml
  upstream.toml             # pinned icon-pack sources (used by `cargo make assets pull`)
  src/
    lib.rs                  # register_uri_scheme(), assets_dir()
    icons.rs                # resolve(pack, name) -> Option<PathBuf>
  assets/
    icons/
      lucide/               # vendored SVGs, committed
      simpleicons/
```

- `register_uri_scheme(context: &WebContext)` called once per `WebContext` in `sola-app`. Registers `sola-assets://<path>` to stream the file at `<assets_dir>/<path>`.
- `assets_dir()` resolves to `/opt/sola/share/` when present, else `<workspace-root>/crates/sola-assets/assets/` via `CARGO_MANIFEST_DIR` at compile time. Fails loudly if neither exists.
- Zero SVG data is compiled into consumer binaries.

### Extension to `sola-app::config`

Add a sibling trait for per-app sub-directory configs, alongside `JsonConfig`:

```rust
pub trait JsonConfigIn: Serialize + DeserializeOwned + Default {
    /// Sub-directory under <config>/sola/
    const APP_DIR: &'static str;
    /// File name inside the sub-directory.
    const FILE_NAME: &'static str;

    fn path() -> PathBuf {
        sola_config_dir().join(Self::APP_DIR).join(Self::FILE_NAME)
    }

    // load / save / try_load / try_load_or_default / try_save / try_save_pretty
    // mirror JsonConfig (shared helper fns already exist in config.rs).
}
```

`ShellConfig` (existing, `shell.json`) stays on `JsonConfig`. `ApplicationsConfig` uses `JsonConfigIn` with `APP_DIR = "shell"`, `FILE_NAME = "applications.json"`.

### Applications in the shell

`apps/shell/src/applications.rs` (new):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Application {
    pub app_id: String,
    pub label: String,
    pub command: String,
    pub icon: String, // "<pack>/<name>", e.g. "lucide/terminal"
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
```

`ShellApp` gains:
- `pub applications: ApplicationsConfig`
- `pub fn application(&self, app_id: &str) -> Option<&Application>`
- `pub fn icon_for(&self, app_id: &str) -> Option<&str>` — returns the `"pack/name"` string, suitable for `<img src="sola-assets://icons/<pack>/<name>.svg">`.

Loaded in `ShellApp::new()` via `ApplicationsConfig::load()`.

### Launcher window

`apps/shell/src/launcher/` (new):
- `mod.rs` — re-exports.
- `assets.rs` — `LAUNCHER_ASSETS` bundle for `launcher.html` + `launcher.ts`.
- `state.rs` — `LauncherState { active, prior_focus, query, filtered_ids, selected }` with filter/navigation methods.

`apps/shell/web/launcher.html` + `apps/shell/web/src/launcher.ts` (new, distinct from `overlay.html`).

`ShellWindows` gains `launcher: WindowHandle`, added in `ShellApp::new()`:
- size 560×420, centered on output (position set when opened, like the switcher).
- `decorated: false`, `transparent: true`.
- `keyboard_target: true` — **first shell window to opt in**. This is a small generalization; no other code paths change.

### Bus topic

`sola-bus/src/topics.rs`:
```rust
pub struct LaunchAppPayload {
    pub command: String,
}
// ...
Topic::LaunchApp(LaunchAppPayload),
```

`sola` subscribes to `LaunchApp`:
- Splits `command` on whitespace into argv.
- `Command::new(argv[0]).args(&argv[1..]).spawn()`.
- Logs `info!(pid, command, "launched")` on success; `warn!(command, err, "launch failed")` on error.
- SIGCHLD handler reaps children and logs exit status. No tracking structure; fire-and-forget.

### Compositor key routing

No compositor changes. Shell adds `KeyCode::SPACE.meta()` to its `ShellKeyBindings` list (same code path as `Meta+Tab`).

## Configuration

### `<config>/sola/shell/applications.json`

```json
{
  "apps": [
    {
      "app_id": "firefox",
      "label": "Firefox",
      "command": "firefox",
      "icon": "simpleicons/firefox"
    },
    {
      "app_id": "sola-terminal",
      "label": "Terminal",
      "command": "/opt/sola/bin/sola-terminal",
      "icon": "lucide/terminal"
    }
  ]
}
```

**Field semantics:**
- `app_id` — identifier. Stable across renames of `label`. Matches the `app_id` the program reports on the bus when it connects. Used as icon-lookup key by `switcher`.
- `label` — human-readable name, shown in UI, search target.
- `command` — whitespace-split argv; argv[0] is the executable. No shell interpretation.
- `icon` — `"<pack>/<name>"` path form; resolves to `sola-assets://icons/<pack>/<name>.svg`. Packs: `lucide`, `simpleicons`.

Missing file: load as empty list (existing `try_load_or_default` path). Malformed file: already logged and fallback to default by `JsonConfig` helper.

### `upstream.toml` (sola-assets)

```toml
[packs.lucide]
source = "github:lucide-icons/lucide"
rev    = "<pinned commit>"
glob   = "icons/*.svg"

[packs.simpleicons]
source = "github:simple-icons/simple-icons"
rev    = "<pinned commit>"
glob   = "icons/*.svg"
```

`cargo make assets pull` fetches each pack (shallow git clone into a tempdir), wipes `assets/icons/<pack>/`, copies matching files. Commits the result by the developer. Fails loudly on network or mismatch. SVGs are committed to the repo so clean clones build offline.

## Open / search / launch flow

1. **Meta+Space (launcher closed):**
   - `keys.rs` on the menubar GTK window catches the chord.
   - Shell snapshots current `FocusTarget` into `launcher.prior_focus`.
   - `launcher.active = true`, `query = ""`, `filtered_ids = <all app_ids>`, `selected = 0`.
   - `Frame { x, y, 560, 420 }` centered on output.
   - Composition re-emitted with `("sola-shell", "launcher")` on top.
   - `Focus { app_id: "sola-shell", title: "launcher" }` emitted — compositor routes keyboard to the launcher window.
   - `renderApps(<list>, 0)` called via `eval_js`.

2. **Typing in input:** `oninput` sends `{ cmd: "query", text }` to Rust. Rust filters `ShellApp::applications` by case-insensitive substring match on `label` (order preserved from config; simple `contains`, no fuzzy ranking). Pushes `renderApps(filtered, 0)` back.

3. **ArrowUp / ArrowDown:** handled in JS; updates highlighted row. Not reported to Rust.

4. **Enter:** JS sends `{ cmd: "launch", app_id }`. Rust:
   - Looks up `Application` by `app_id`.
   - Emits `Topic::LaunchApp { command }`.
   - Closes the launcher (same path as ESC).

5. **ESC or Meta+Space (launcher open):** JS sends `{ cmd: "close" }`. Rust:
   - `launcher.active = false`, clear state.
   - Composition re-emitted without the launcher entry.
   - `Focus` re-emitted for `prior_focus` (or the first MRU app if `prior_focus` is `None`).

6. **Prior focus unknown at open time** (nothing focused): launcher still opens; on close, focus simply isn't re-emitted — MRU handling in `set_focus` takes over as apps interact.

## Rendering

### launcher.html

- Full-window transparent background.
- Centered panel, width 100%, height 100%, `rgba(30,30,30,0.88)`, border-radius 10px (matches switcher).
- Top: `<input type="text" autofocus>`, 15px padding, transparent, large font.
- Below: scrollable `<ul>` of result rows — 40px each, `<img src="sola-assets://icons/<pack>/<name>.svg">` (22px) + label. Selected row: `rgba(56,120,240,0.85)` (matches switcher).
- Empty-result state: one muted row, "No matching applications."

### launcher.ts

- Exposes `renderApps(list, selected)`, `setSelection(i)`, `clear()` as window globals (same pattern as `overlay.ts`).
- On key events: ArrowUp/Down → `setSelection`; Enter → post `launch`; Escape → post `close`; other keys → default input behavior.
- On input → post `query`.

### Switcher consumes icons

Switcher's current hardcoded `\u2B21` glyph is replaced by `<img src="sola-assets://icons/<pack>/<name>.svg">` when `ShellApp::icon_for(app_id)` returns a value; otherwise the glyph remains as fallback. Switcher JS gets an `icon` field on each app passed to `renderSwitcher`.

## Error handling

- `applications.json` missing → empty list via `try_load_or_default`. Launcher opens with only the empty-state row.
- `applications.json` malformed → existing `JsonConfig::load()` logs `warn!` and falls back to default.
- `icon` references a nonexistent file → WebKit renders broken-image icon; row still clickable. Not worth fallback rendering until it's a real nuisance.
- `command` empty or un-splittable → `sola` logs `warn!` and drops.
- `Command::spawn` fails (missing binary, permission) → `sola` logs `warn!`. No UI surfacing yet.
- Launcher opened with no applications → shows the empty-state row; Enter does nothing.

## Testing

Unit tests:
- `sola-assets`: `resolve("lucide", "terminal")` returns a real path in dev mode; URI scheme streams the file bytes correctly.
- `shell::launcher::state`: filter reduces `["Firefox", "Terminal", "Files"]` with `"fi"` to `["Firefox", "Files"]` in order; `"ZZZ"` to empty.
- `shell::applications`: `ApplicationsConfig` round-trips the canonical example JSON.
- `sola-app::config`: `JsonConfigIn` resolves path to `<config>/sola/<APP_DIR>/<FILE_NAME>`.

Manual (TTY):
- Meta+Space opens launcher, keyboard lands in input.
- Typing filters; selection moves with arrows.
- Enter launches a configured app; launcher closes; focus restored.
- Escape closes; focus restored.
- Meta+Space while open closes.
- Missing `applications.json` shows empty-state row.
- Switcher shows icons for configured apps, glyph fallback otherwise.

## Build sequence

Each step is an independent commit that keeps the tree buildable.

1. Extend `sola-app::config` with `JsonConfigIn`. Unit test. No other callers yet.
2. Scaffold `sola-assets` crate (Cargo.toml, lib.rs, icons.rs, empty `assets/icons/{lucide,simpleicons}/.gitkeep`). No consumers yet.
3. Add `cargo make assets pull` to `sola-make`; write `upstream.toml`; pin initial revs; run it to populate `assets/icons/`; commit the SVGs.
4. Wire `cargo make install` to copy `crates/sola-assets/assets/` → `/opt/sola/share/`. Add `sola-assets://` URI scheme registration to `sola-app`'s `WebContext` setup.
5. Add `Application` / `ApplicationsConfig` to the shell; `ShellApp::application()`, `icon_for()`; load in `ShellApp::new()`.
6. Switch the switcher to use `icon_for()` + `sola-assets://` — validates the icon pipeline end-to-end before the launcher depends on it.
7. Add `Topic::LaunchApp` to `sola-bus`; `sola` subscribes and spawns with SIGCHLD reap. No shell consumer yet.
8. Launcher window + HTML/TS + `LauncherState`; Meta+Space handling in `keys.rs`; open/close/filter/launch flow. End-to-end manual test on a TTY.

## Future work (explicitly out of scope)

- `Topic::KillApp`, child-death reporting on the bus, shell-driven session restore.
- UI (shell settings app) for editing `applications.json`.
- `.desktop` file scraping.
- Fuzzy-ranked search (`fuzzy-matcher` crate or similar) if substring proves insufficient.
- Icon theming, dark/light variants.
- Additional asset kinds in `sola-assets` (fonts, cursors, wallpapers).
