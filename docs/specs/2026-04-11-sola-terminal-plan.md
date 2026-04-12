# Sola Terminal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the Cogsworth terminal to Sola as a GTK4/WebKit6 app with tmux-backed tabs, xterm.js rendering, and sola-bus integration.

**Architecture:** Single-process app with two threads — glib main thread (GTK4/WebKit6, bus polling) and a tokio background thread (WebSocket server, PTY management). Svelte 5 + xterm.js frontend embedded via `include_dir!`. Communication between threads via channels.

**Tech Stack:** Rust, GTK4, WebKit6, tokio, tokio-tungstenite, nix, Svelte 5, xterm.js, Vite

**Spec:** `docs/specs/2026-04-11-sola-terminal-design.md`

**Worktree:** `.worktrees/sola-terminal` (branch: `sola-terminal`)

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `Cargo.toml` (workspace root) | Modify | Add `apps/terminal` to workspace members |
| `apps/terminal/Cargo.toml` | Create | Crate manifest with all dependencies |
| `apps/terminal/src/tmux.rs` | Create | tmux session management (ported from Cogsworth) |
| `apps/terminal/src/pty.rs` | Create | PTY creation, reader threads, lifecycle |
| `apps/terminal/src/state.rs` | Create | Single-window tab state, JSON persistence |
| `apps/terminal/src/server.rs` | Create | tokio WebSocket server, command dispatch |
| `apps/terminal/src/main.rs` | Create | GTK4 app, WebView, bus, glib-tokio bridge |
| `apps/terminal/web/package.json` | Create | Frontend dependencies |
| `apps/terminal/web/vite.config.ts` | Create | Vite build config |
| `apps/terminal/web/svelte.config.js` | Create | Svelte preprocessor config |
| `apps/terminal/web/src/main.ts` | Create | Svelte mount point |
| `apps/terminal/web/src/ws.ts` | Create | WebSocket client helpers |
| `apps/terminal/web/src/App.svelte` | Create | Root component, tab state, WS connection |
| `apps/terminal/web/src/Terminal.svelte` | Create | xterm.js instance per tab |
| `apps/terminal/web/src/TerminalSidebar.svelte` | Create | Tab list, reorder, rename |
| `apps/terminal/web/src/theme.css` | Create | CSS custom properties (extracted from Cogsworth) |

---

## Tasks

12 tasks total. Each task produces a commit. Complete code provided in every step.

### Task 1: Project Scaffolding

Create the crate, add to workspace.

**Files:** Create `apps/terminal/Cargo.toml`, stub `apps/terminal/src/main.rs`

Steps: create worktree, write Cargo.toml with all deps (sola-bus, gtk4, gdk4, glib, gio, webkit6, tokio, tokio-tungstenite, futures-util, nix, libc, base64, serde, serde_json, uuid, tracing, tracing-subscriber, tracing-appender, include_dir), write stub main.rs, verify `cargo make build` passes, commit.

See spec for exact dependency versions. Match sola-switcher's Cargo.toml structure. The stub main.rs is just `fn main() { println!("sola-terminal stub"); }`.

### Task 2: tmux Module

Port from Cogsworth. Key changes: socket name `cogsworth` -> `sola`, session prefix `cogsworth-` -> `sola-`, config path `~/.config/cogsworth/` -> `~/.config/sola/`, session filter `starts_with("cogsworth-")` -> `starts_with("sola-")`.

**Files:** Create `apps/terminal/src/tmux.rs`, update `main.rs` to add `mod tmux;`

The full source is a direct port from `Cogsworth/apps/terminal/src/tmux.rs` with the above substitutions. Include tests: `session_name_format`, `config_path_under_sola_dir`, `tmux_conf_disables_status`. Run `cargo test -p sola-terminal`, commit.

### Task 3: PTY Module

Port from Cogsworth nearly verbatim. No Cogsworth-specific deps. Uses `crate::tmux` which exists from Task 2.

**Files:** Create `apps/terminal/src/pty.rs`, update `main.rs` to add `mod pty;`

The full source is from `Cogsworth/apps/terminal/src/pty.rs`. The `PtyManager` struct, `PtyEvent` enum, `OutputBuffer`, spawn/write/resize/close/reconnect methods, Drop impl — all identical. Verify `cargo build -p sola-terminal` compiles, commit.

### Task 4: State Module

Simplified from Cogsworth for single-window. No `cogsworth-app` deps, no multi-window maps.

**Files:** Create `apps/terminal/src/state.rs`, update `main.rs` to add `mod state;`

Key struct:
- `TerminalState` with fields: `tabs: RwLock<Vec<TabEntry>>`, `custom_titles: RwLock<HashMap<String, String>>`, `tab_cwds: RwLock<HashMap<String, String>>`, `pty_manager: tokio::sync::Mutex<PtyManager>`
- `TabEntry`: `pty_id`, `tmux_session`, `custom_title: Option`, `cwd: Option`
- `RestoredTab`: `tmux_session`, `custom_title: Option`, `cwd: Option`

Methods: `new()`, `persist_to_disk()` (atomic write to `~/.config/sola/terminal-state.json`), `load_from_disk()` (returns `Vec<RestoredTab>`, reconciles with live tmux sessions).

Include tests: `state_file_path_under_sola`, `empty_state_serializes`. Run tests, commit.

### Task 5: WebSocket Server

New module. Tokio-based WS server with command dispatch.

**Files:** Create `apps/terminal/src/server.rs`, update `main.rs` to add `mod server;`

Public API:
- `pub async fn start(state: Arc<TerminalState>, bus_rx: mpsc::UnboundedReceiver<BusEvent>) -> u16` — binds to `127.0.0.1:0`, returns port
- `pub enum BusEvent { NewTab }` — forwarded from glib bus polling

Internal: `handle_connection` accepts WS, splits into send/recv tasks. Recv loop parses `{ id, cmd, args }` JSON, dispatches to handlers, sends `{ id, result }` response. PTY events forwarded via per-client channel. Bus events forwarded via broadcast channel.

Commands: `spawn_pty`, `write_pty`, `resize_pty`, `close_pty`, `reconnect_pty`, `rename_tab`, `reorder_tabs`. After mutation commands, calls `state.persist_to_disk()`.

Verify compiles, commit.

### Task 6: Main Entry Point

Replace stub with full GTK4/WebKit6 app.

**Files:** Replace `apps/terminal/src/main.rs`

Pattern follows sola-switcher: logging setup (stderr + file at `/opt/sola/log/sola-terminal.log`), Wayland socket wait, `glib::set_prgname("sola-terminal")`, GTK Application with `connect_activate`.

In activate:
1. Create `Arc<TerminalState>`, call `load_from_disk()`, serialize restored tabs to JSON
2. Create `mpsc::unbounded_channel::<BusEvent>()` for glib-to-tokio
3. Spawn tokio on background thread, call `server::start()`, get port back via `std::sync::mpsc`
4. Create GTK window (undecorated, 1920x1080), WebKit6 WebView
5. `include_str!("../web/dist/index.html")` with `__WS_PORT__` and `__RESTORED_TABS__` placeholder replacement
6. Connect to sola-bus, poll every 50ms for `Topic::Key` where `code == 28` (T) and `super_held` -> send `BusEvent::NewTab`
7. `window.present()`

Note: KEY_T XKB keycode is 28 (evdev T=20, +8=28).

Create placeholder `apps/terminal/web/dist/index.html` so `include_str!` works. Verify compiles, commit.

### Task 7: Frontend Scaffolding

**Files:** Create `web/package.json`, `web/vite.config.ts`, `web/svelte.config.js`, `web/src/main.ts`, `web/src/ws.ts`, `web/src/theme.css`, `web/index.html`

package.json: devDeps (svelte 5.x, vite 7.x, typescript, @sveltejs/vite-plugin-svelte), deps (xterm addons: fit, web-links, canvas, and @xterm/xterm).

vite.config.ts: svelte plugin, outDir `dist`.

ws.ts (~50 lines): `connect(port)`, `invoke(cmd, args)`, `on(event, callback)`. Uses JSON messages over WebSocket. `invoke` returns a Promise keyed by auto-incrementing ID.

theme.css: CSS custom properties: `--bg-primary: #0a0b0d`, `--bg-secondary: #101216`, `--bg-tertiary: #181b21`, `--text-primary: #f0f2f5`, `--text-secondary: #a0a8b4`, `--text-muted: #5a6270`, `--border-subtle: #1e2228`, `--cyan: #00a8ff`, `--cyan-dim: rgba(0,168,255,0.15)`, `--red: #ff3d5a`, `--green: #00ff88`, `--font-mono`.

index.html: standard Vite entry with `<script>window.WS_PORT = __WS_PORT__; window.RESTORED_TABS = __RESTORED_TABS__;</script>` in head, `<div id="app">`, module script src `/src/main.ts`.

main.ts: import theme.css, mount App.svelte to `#app`.

Create placeholder App.svelte, run `npm install && npm run build`, verify Rust build works with real dist, commit.

### Task 8: App.svelte

Root component. Manages tab array, WebSocket connection, sidebar state.

**Files:** Replace `web/src/App.svelte`

On mount: `await connect(port)`, restore tabs from `window.RESTORED_TABS` or create one new tab. Listen for `new_tab` WS event (from Super+T). Key handler for Super+1-9 tab switching.

Tab management functions: `createTab`, `closeTab`, `removeTab`, `switchTab`, `handleReorder`, `handleRename`. Sidebar state persisted to localStorage.

Template: flex layout with TerminalSidebar + terminal-area. Terminal panes use absolute positioning with `display:none` / `display:block` for active tab.

Commit.

### Task 9: Terminal.svelte

xterm.js instance per tab. Port from Cogsworth, replace `@cogsworth/shared/api` imports with local `./ws` imports.

**Files:** Create `web/src/Terminal.svelte`

Props: `tabId`, `tmuxSession?`, `initialCwd?`, `focused?`, `onExit?`, `onTitleChange?`, `onCwdChange?`.

Exports: `closePty()`, `refit()`.

On mount: wait for layout, create xterm Terminal with FitAddon + WebLinksAddon + CanvasAddon, spawn PTY via `invoke('spawn_pty', ...)`, wire data channels (`pty:data`, `pty:exit`, `pty:scrollback` events filtered by `pty_id`), wire resize observer.

Key differences from Cogsworth: field names use `pty_id` (snake_case, matching server protocol), no `open_url` handler, no copy/paste event wiring (deferred), no `tmux_scroll` (deferred).

Commit.

### Task 10: TerminalSidebar.svelte

Port from Cogsworth. Identical functionality: tab list, click/middle-click, drag-to-reorder, double-click rename. Uses CSS custom properties from theme.css.

**Files:** Create `web/src/TerminalSidebar.svelte`

Exports the `TerminalTab` interface used by App.svelte.

Commit.

### Task 11: Build Integration

**Files:** Create `web/.gitignore` (exclude `node_modules/`)

Rebuild frontend, verify full Rust build, run all tests.

Commit.

### Task 12: Final Integration

Full workspace build (`cargo make build`), full test run (`cargo test --workspace`). Fix any issues, commit.
