# Sola Terminal Design

Port of the Cogsworth terminal to Sola. Single-window terminal emulator with tabbed tmux-backed sessions, xterm.js rendering, and sola-bus integration.

## Architecture

Single OS process (`sola-terminal`) with two threads:

- **Main thread (glib):** GTK4 Application, WebKit6 WebView, sola-bus polling. Owns the window.
- **Background thread (tokio):** WebSocket server, PTY management, tmux interaction, state persistence. One additional std::thread per active PTY for reading the master fd.

### Thread Communication

- **tokio → glib:** `glib::Sender` / `glib::idle_add_local()` for pushing state to the WebView.
- **glib → tokio:** `tokio::sync::mpsc` channel for forwarding bus events (Super+T).

### Startup Sequence

1. Parse env: `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, `SOLA_BUS_PATH`.
2. Wait for Wayland socket (poll until it exists).
3. Start tokio runtime on background thread, bind WebSocket server on ephemeral port.
4. Start GTK Application, create window + WebView.
5. Load embedded frontend HTML (port injected via placeholder replacement).
6. Connect to sola-bus, start polling for Key events.

## File Structure

```
apps/terminal/
├── Cargo.toml
├── src/
│   ├── main.rs          # GTK4 init, WebView, bus connection, glib↔tokio bridge
│   ├── server.rs        # tokio WebSocket server, command dispatch
│   ├── pty.rs           # PTY creation/management (ported from Cogsworth)
│   ├── tmux.rs          # tmux session lifecycle (ported from Cogsworth)
│   └── state.rs         # Tab metadata, JSON persistence
└── web/
    ├── package.json
    ├── vite.config.ts
    ├── svelte.config.js
    ├── src/
    │   ├── main.ts              # Mount Svelte app
    │   ├── App.svelte           # Root, WebSocket connection, tab state
    │   ├── Terminal.svelte      # xterm.js instance per tab
    │   ├── TerminalSidebar.svelte  # Tab list, reorder, rename
    │   └── ws.ts                # WebSocket helpers (~50 lines)
    └── dist/                    # Built output, embedded via include_dir!
```

## Dependencies

### Rust

- `gtk4`, `gdk4`, `webkit6`, `glib`, `gio` — GTK4/WebKit6 windowing
- `sola-bus` — IPC
- `tokio` (rt-multi-thread, sync, net, io-util) — async runtime
- `tokio-tungstenite` — WebSocket server
- `nix` — PTY operations (openpty, ioctl, signals)
- `base64` — PTY data encoding
- `serde`, `serde_json` — state serialization
- `uuid` — tab IDs
- `tracing`, `tracing-subscriber`, `tracing-appender` — logging
- `include_dir` — embed frontend dist

### Frontend (web/)

- `svelte` 5.x, `@sveltejs/vite-plugin-svelte`
- `vite`, `typescript`
- `@xterm/xterm`, `@xterm/addon-fit`, `@xterm/addon-web-links`, `@xterm/addon-canvas`

## WebSocket Protocol

JSON command/event protocol over `ws://127.0.0.1:{port}`.

### Commands (frontend -> backend)

| Command | Args | Response |
|---------|------|----------|
| `spawn_pty` | `{ cwd?: string }` | `{ pty_id, title }` |
| `write_pty` | `{ pty_id, data: base64 }` | `"ok"` |
| `resize_pty` | `{ pty_id, cols, rows }` | `"ok"` |
| `close_pty` | `{ pty_id }` | `"ok"` |
| `rename_tab` | `{ pty_id, title }` | `"ok"` |
| `reorder_tabs` | `{ pty_ids: [...] }` | `"ok"` |
| `reconnect_pty` | `{ pty_id }` | `{ scrollback: base64 }` |

Request format: `{ id: number, cmd: string, args: object }`
Response format: `{ id: number, result: any }`

### Events (backend -> frontend)

| Event | Fields | Description |
|-------|--------|-------------|
| `pty:data` | `pty_id, data: base64` | PTY output chunk |
| `pty:exit` | `pty_id` | Child process exited |
| `pty:scrollback` | `pty_id, data: base64` | Initial scrollback on reattach |
| `new_tab` | — | Super+T pressed (from bus) |

## PTY & tmux Lifecycle

### Tab Creation (spawn_pty)

1. Generate UUID for the tab.
2. Create tmux session: `tmux new-session -d -s sola-{uuid} -x {cols} -y {rows}`.
3. If `cwd` provided, session starts in that directory.
4. Open PTY pair via `nix::pty::openpty()`.
5. Attach to the tmux session: child process runs `tmux attach-session -t sola-{uuid}`.
6. Spawn reader thread on the master fd (4KB chunks -> base64 -> WebSocket event).
7. Capture initial scrollback if reattaching to an existing session.
8. Persist state to disk.

### Tab Close (close_pty)

1. Kill the PTY reader thread.
2. Close the master fd.
3. Kill the tmux session: `tmux kill-session -t sola-{uuid}`.
4. Remove from state, persist to disk.

### Resize (resize_pty)

1. `ioctl(TIOCSWINSZ)` on the master fd.
2. `tmux resize-window -t sola-{uuid} -x {cols} -y {rows}`.
3. Send SIGWINCH to the child process group.

### Startup Reconciliation

1. Load `~/.config/sola/terminal-state.json`.
2. List live tmux sessions matching `sola-*`.
3. Sessions in state file + alive in tmux: reattach (restore title, CWD metadata).
4. Sessions alive in tmux but not in state: adopt with default metadata.
5. Sessions in state but dead in tmux: discard.

## Bus Integration

Minimal surface:

### Inbound: Super+T

- Poll sola-bus on glib main thread (50ms interval).
- Listen for `Topic::Key` events matching Super+T.
- Forward over glib->tokio channel.
- Tokio spawns new PTY, sends `{ event: "new_tab" }` over WebSocket.
- Frontend receives event, calls `spawn_pty`, creates new xterm tab.

### Identity

- `gtk::glib::set_prgname(Some("sola-terminal"))` sets `app_id`.
- Compositor sees this in the Wayland toplevel, switcher picks it up automatically.
- No explicit registration needed.

## Frontend Architecture

### App.svelte

- Establishes WebSocket connection (port injected into HTML by Rust as `__WS_PORT__` replacement).
- Manages tab list as Svelte 5 reactive state: `$state` array of `{ pty_id, title, active }`.
- Listens for `new_tab` event from backend, triggers spawn.
- Handles `pty:exit` by removing the tab.
- Key listener for Super+1-9 tab switching.

### Terminal.svelte

- Creates xterm.js instance on mount with FitAddon, WebLinksAddon, CanvasAddon.
- Wires `onData` -> `write_pty` command over WebSocket.
- Listens for `pty:data` events filtered by `pty_id` -> `terminal.write(atob(data))`.
- Handles resize via FitAddon + ResizeObserver -> `resize_pty` command.
- OSC 0/2 handler updates tab title (unless custom title set).
- OSC 7 handler tracks CWD.

### TerminalSidebar.svelte

- Tab list: click to select, middle-click to close.
- Drag-to-reorder (pointer-based DnD).
- Double-click to rename (sends `rename_tab` command).

### ws.ts

- `connect(port)` — returns WebSocket wrapper.
- `invoke(cmd, args)` — sends command, returns promise resolved by matching response ID.
- `on(event, callback)` — registers event listener by event name.
- Replaces `@cogsworth/shared/api`.

## State Persistence

**File:** `~/.config/sola/terminal-state.json`

```json
{
  "tabs": [
    {
      "tmux_session": "sola-{uuid}",
      "custom_title": "Build Server",
      "cwd": "/home/user/project"
    }
  ]
}
```

**Sync points:**
- After every mutation: spawn, close, rename, reorder -> persist to disk.
- Atomic writes: write to `.tmp`, rename into place.

**tmux is the source of truth** for session liveness. The state file stores metadata only (custom titles, CWDs, tab order). On startup, reconcile state file against live tmux sessions.

## Logging

- File log: `/opt/sola/log/sola-terminal.log`
- Same tracing setup as switcher (stderr + file appender).

## What's Deferred

- Multi-window support
- URL forwarding to browser
- Inter-app communication beyond Super+T
- Theming / shared CSS system
- Copy/paste integration (will use xterm.js defaults)
