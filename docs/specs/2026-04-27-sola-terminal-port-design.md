# sola-terminal Port Design

**Date:** 2026-04-27
**Status:** Proposed
**Branch:** `feature/terminal-port`

## Summary

Port the existing `apps/terminal/` into the workspace as `crates/sola-terminal/`, register it as a builtin app for the launcher, and bring it into line with current Sola conventions — primarily by replacing its on-disk `terminal-state.json` with sticky bus topics.

## Goals

- Move `apps/terminal` to `crates/sola-terminal` so it is built and installed by `cargo make`.
- Register Terminal as a builtin application in the launcher.
- Replace the `JsonConfig`-backed `terminal-state.json` with two sticky bus topics.
- Drop the custom tab name feature.
- Make the app menu's `Tab N` entries reflect the actual tab count.

## Non-goals

- Refactoring `pty.rs` (e.g. non-blocking I/O).
- Replacing tmux as the PTY/multiplexer backend.
- Adding configurable shell, font, or theme.
- Migrating any existing `~/.config/sola/terminal-state.json` — terminal is currently absent from master, so no live state needs preserving.

## Architecture

### Crate move

- Source moves: `apps/terminal/Cargo.toml` → `crates/sola-terminal/Cargo.toml` (package name `sola-terminal` unchanged); same for `src/` and `web/`.
- The workspace `Cargo.toml` already globs `crates/*`, so the move makes `sola-terminal` a workspace member automatically.
- `sola-make` auto-discovers binaries via `crates/*/src/main.rs`, so `cargo make install` picks it up without any `sola-make` change.
- Delete `apps/terminal/`.

### Builtin registration

Add to `crates/sola-core/src/applications.rs::builtin_apps()`:

```rust
Application {
    app_id: "sola-terminal",
    label: "Terminal",
    command: "/opt/sola/bin/sola-terminal",
    icon: "lucide/terminal",
}
```

### tmux backend

Unchanged from the existing terminal. The runtime tmux config write to `~/.config/sola/tmux.conf` (`tmux.rs`) stays — that file is tmux's own configuration input, not Sola application state, so it does not violate the "no config files; use bus state" rule.

## Bus topics

Two new persistent topics in `crates/sola-bus/src/topics.rs`, both `#[sticky]`, modelled on `MailConfig`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalConfig {
    pub sidebar_width: u32,
    pub sidebar_collapsed: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self { sidebar_width: 220, sidebar_collapsed: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTab {
    pub id: String,
    pub tmux_session: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessions {
    pub tabs: Vec<TerminalTab>,
}
```

Added to the `Topic` enum:

```rust
#[sticky]
TerminalConfig(TerminalConfig),
#[sticky]
TerminalSessions(TerminalSessions),
```

`TerminalTab` deliberately has no `custom_title` field — the rename feature is being removed.

## Crate layout

```
crates/sola-terminal/
  Cargo.toml
  src/
    main.rs       # SolaApp impl
    state.rs      # In-memory mirror + PtyManager
    pty.rs        # PTY lifecycle (unchanged from apps/terminal)
    tmux.rs       # tmux helpers (unchanged from apps/terminal)
    commands.rs   # JS command dispatcher
    menu.rs       # NEW: terminal_menu(tab_count)
  web/
    index.html
    src/
      main.ts
      app.ts
      terminal-pane.ts
      theme.css
      components/
        sidebar.ts
    vendor/       # xterm.js + addons + arrow.js (copied)
```

### `src/state.rs`

In-memory mirror of `TerminalConfig` and `TerminalSessions`, plus `PtyManager`. No filesystem code remains. Mutation helpers emit the relevant `Topic`. On startup it reconciles bus state against live tmux: if tmux has session `sola-{id}` running, it stays; if a tab in bus state has no live tmux session, it's dropped. Tmux is the ground truth for which sessions exist; the bus contributes ordering and cwd hints.

### `src/menu.rs`

```rust
pub fn terminal_menu(tab_count: usize) -> AppMenuPayload { ... }
```

Items:

1. New Tab (Cmd+T) — action `terminal.new_tab`
2. Divider
3. `Tab 1` … `Tab N` actions (`terminal.select_tab.0` … `terminal.select_tab.{N-1}`). Cmd+1..=Cmd+9 shortcuts assigned to the first nine; tabs 10+ get no shortcut.
4. Divider
5. About
6. Quit

### `src/main.rs`

- `SolaApp::APP_ID = "sola-terminal"` (unchanged).
- `SolaApp::register_bus`:
  - `TopicKind::TerminalConfig` handler — update in-memory copy, push state to JS via `send_to_js`. Does **not** re-emit `SetAppMenu`.
  - `TopicKind::TerminalSessions` handler — update in-memory copy, push state to JS. Does **not** re-emit `SetAppMenu`. (We emit this topic ourselves from command handlers, and the bus echoes our own emits back — re-emitting here would double the menu emission. Menu re-emits are owned exclusively by the command handlers in `commands.rs`.)
  - `TopicKind::MenuAction` handler — existing dispatch logic.
- `SolaApp::new`:
  - `ctx.add_window(...)` with `initial_state = Some({ tabs: [], sidebar_width: default, sidebar_collapsed: false })`. The window mounts immediately; the JS side renders an empty terminal until state arrives.
  - Subscribe to bus topics. The bus replays sticky `TerminalConfig` and `TerminalSessions` into our handlers; the handlers do the in-memory + push-to-JS update.
  - On the first `TerminalSessions` replay, reconcile against live tmux (drop tabs whose tmux session is gone; keep ordering and cwds for survivors), then emit the reconciled `Topic::TerminalSessions(...)` and the matching `Topic::SetAppMenu(terminal_menu(tab_count))`. A small "did first reconcile yet" boolean guards this so subsequent replays don't redo it.

### `src/commands.rs`

`TerminalHandler::dispatch` matches:

| Command | Effect |
|---|---|
| `spawn_pty` | tmux new-session, push `TerminalTab`, emit `TerminalSessions`, emit `SetAppMenu(new_count)` |
| `close_pty { id }` | kill tmux session, remove tab, emit both |
| `reorder_tabs { ids }` | reorder, emit `TerminalSessions` (and `SetAppMenu` since position labels change) |
| `update_cwd { id, cwd }` | mutate `tab.cwd`, emit `TerminalSessions` |
| `write_pty`, `resize_pty`, `reconnect_pty` | unchanged, no topic emission needed |
| `set_sidebar { width, collapsed }` | mutate `TerminalConfig`, emit `TerminalConfig` |
| `rename_tab` | **removed** |

### `web/`

Three changes from existing `apps/terminal/web/`:

1. **Remove rename UX** from `web/src/components/sidebar.ts`: drop `renamingTabId`, `renameValue`, the rename input element, and its event handlers. Drop `config.onRename`. Tab label is always derived (CWD basename → `tmux session id`).
2. **Remove localStorage `persist()`** in `web/src/app.ts`. `sidebarCollapsed` and `sidebarWidth` come from `__RESTORED_STATE__`. Mutations `invoke('set_sidebar', { width, collapsed })`. State updates from Rust (`event: 'state'`) re-sync the store.
3. **Remove `rename_tab` invocation** and `displayTitle`'s custom-title branch in `app.ts` / `sidebar.ts`.

## Data flow

```
Startup
  bus  → replay TerminalConfig + TerminalSessions (sticky)
  app  → reconcile against tmux → emit SetAppMenu(count)
  app  → ctx.add_window(initial_state = {tabs, sidebar_*})

JS spawn_pty
  app  → tmux new-session
  app  → emit TerminalSessions, emit SetAppMenu(count+1)

JS close_pty / reorder_tabs
  app  → mutate → emit TerminalSessions, emit SetAppMenu(count)

JS update_cwd (OSC 7)
  app  → mutate tab.cwd → emit TerminalSessions

JS set_sidebar
  app  → mutate config → emit TerminalConfig

Bus → app TerminalConfig / TerminalSessions handler
  app  → send_to_js({event:'state', state:payload})
```

## Error handling

- Bus emit is fire-and-forget; existing tracing remains.
- PTY/tmux errors continue to surface as JS command result errors and `tracing` warnings.
- Sticky topic decode failures fall back to `Default::default()` (this is the bus's existing behavior for sticky sections).

## Testing

### Unit

In `crates/sola-bus/src/topics.rs`, mirror the existing `MailConfig` tests:

- `terminal_config_roundtrip` — encode / parse / `from_toml_section`.
- `terminal_sessions_roundtrip` — same, with a non-empty tabs vec.

### Manual smoke (post `cargo make install`)

1. Run `sola` from a TTY. Open the launcher; confirm "Terminal" appears.
2. Open Terminal. Spawn 2 tabs. Confirm menu shows only `Tab 1`, `Tab 2` (not Tab 3–9). Close Tab 2; menu shows only `Tab 1`.
3. Drag the sidebar to a new width. Quit and relaunch the terminal app. Sidebar width restored.
4. Open 3 tabs, change cwd in each (`cd somewhere`). Quit and relaunch. Tabs reattach to tmux; sidebar labels show the saved cwds.
5. Right-click a tab — there is no rename option (expected).

## Migration / cleanup

- Delete `apps/terminal/` directory.
- No data migration needed (terminal is currently not on master).

## Build sequence (informational, not a plan)

The detailed task plan will be written in a follow-up planning step; this design only sketches the layers:

1. Add `TerminalConfig` and `TerminalSessions` topics + tests to `sola-bus`.
2. Move `apps/terminal` to `crates/sola-terminal`. Build + install to confirm parity (still file-backed).
3. Switch persistence: replace `JsonConfig` with bus state on both Rust and JS sides.
4. Drop rename feature on Rust and JS sides.
5. Add `menu.rs` with dynamic tab count; re-emit `SetAppMenu` on count changes.
6. Register in `builtin_apps()`.
7. Smoke-test per checklist above.

## Open questions

None at design time.
