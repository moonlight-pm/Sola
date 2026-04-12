# sola-agent Design

A Sola desktop app for interactive AI-assisted coding. Uses claurst crates for the agent engine (LLM calls, tool execution, MCP). Hosts an Arrow.js frontend in a WebKit6 WebView.

## Architecture

Single process. GTK4 main loop on the main thread. A tokio runtime runs on a background thread. Each active conversation gets a tokio task running claurst's `run_query_loop()`. Events flow from agent tasks to the WebView via a channel drained by a GLib idle callback.

```
┌─────────────────────────────────────────────────┐
│                 sola-agent process               │
│                                                  │
│  ┌──────────────┐    ┌────────────────────────┐  │
│  │  GTK4 main   │    │   tokio runtime        │  │
│  │  loop         │    │   (background thread)  │  │
│  │              │    │                        │  │
│  │  WebKit6     │◄──►│  Session 1: agent loop │  │
│  │  WebView     │    │  Session 2: agent loop │  │
│  │              │    │  Session N: agent loop │  │
│  │              │    │                        │  │
│  │              │    │  MCP connections       │  │
│  │              │    │  Auth manager          │  │
│  └──────┬───────┘    └────────────────────────┘  │
│         │                                        │
│    sola-bus client                               │
└─────────────────────────────────────────────────┘
```

## Dependencies

Claurst crates as git dependencies (not published to crates.io):
- `claurst-core` — shared types, config, auth primitives
- `claurst-api` — multi-provider LLM client with streaming
- `claurst-query` — agent loop (`run_query_loop()`)
- `claurst-tools` — built-in tool implementations (38 tools)
- `claurst-mcp` — MCP client (stdio + HTTP/SSE transports)

Sola crates:
- `sola-bus` — bus client for inter-component IPC

GTK/WebView:
- `gtk4`, `gdk4`, `glib`, `gio` — window management, event loop
- `webkit6` — WebView hosting

Other:
- `tokio` — async runtime for agent loops
- `serde`, `serde_json` — serialization
- `reqwest` — token refresh HTTP calls
- `tracing`, `tracing-subscriber`, `tracing-appender` — logging
- `uuid` — session/tab identifiers

## Authentication

Reads Claude Code's existing OAuth credentials from `~/.claude/.credentials.json`. No login flow in the app.

### Credential file format

```json
{
  "claudeAiOauth": {
    "accessToken": "...",
    "refreshToken": "...",
    "expiresAt": 1775981878849,
    "scopes": ["user:inference", "..."]
  }
}
```

### AuthManager

- Loads `accessToken` and `refreshToken` from the credentials file
- Checks expiry before each API call: `now + 5min >= expiresAt`
- Refreshes via `POST https://platform.claude.com/v1/oauth/token` with `grant_type=refresh_token`
- Writes updated tokens back to the same file (stays in sync with Claude Code)
- On 401 from API: force refresh and retry
- Passes `accessToken` to claurst's `AnthropicClient` with `use_bearer_auth: true`

## JS-Rust Bridge

WebKit6 `UserContentManager` for structured bidirectional communication.

### Rust → JS

`webview.evaluate_javascript()` calls `window.sola.dispatch(event)` with JSON-serialized events.

Event types:
- `message_start { session_id }` — assistant begins responding
- `message_delta { session_id, text }` — streaming text chunk
- `message_end { session_id }` — assistant finished
- `tool_start { session_id, tool_name, tool_input }` — tool execution begins
- `tool_end { session_id, tool_name, result }` — tool execution complete
- `session_state { session_id, status }` — status change (idle, running, error)
- `conversations_list { conversations }` — response to list request
- `session_loaded { session_id, messages }` — full history for resumed session

### JS → Rust

`window.webkit.messageHandlers.sola.postMessage(JSON.stringify(command))` sends commands to Rust via `UserContentManager` script message handler.

Command types:
- `send_message { session_id, text }` — user sends a prompt
- `cancel { session_id }` — interrupt the agent
- `new_session { working_dir }` — create a new conversation
- `resume_session { session_id }` — open an existing conversation
- `close_session { session_id }` — close a conversation
- `list_conversations` — request saved conversation list
- `rename_conversation { session_id, name }` — rename a conversation

## Session Management

### Session struct

```rust
struct Session {
    session_id: String,
    name: Option<String>,
    working_dir: PathBuf,
    messages: Vec<Message>,
    cancel_token: CancellationToken,
    status: SessionStatus,  // Idle, Running, Error
    tools: Vec<Box<dyn Tool>>,
    tool_ctx: ToolContext,
}
```

### New session flow

1. Frontend sends `new_session { working_dir }`
2. Rust creates a Session with claurst ToolContext rooted at that directory
3. Loads project-local MCP servers (walks directory tree + global)
4. Pushes `session_state { status: idle }` to frontend
5. Conversation appears in sidebar under "Running"

### Resume session flow

1. Frontend sends `list_conversations`
2. Rust scans claurst's session storage, returns summaries
3. User picks one → frontend sends `resume_session { session_id }`
4. Rust loads conversation history, pushes `session_loaded` to frontend

### Persistence

Claurst handles session persistence (JSONL format). Custom metadata (conversation name) stored alongside in a sola-agent-specific metadata file.

## MCP Configuration

Reads `.mcp.json` files in Claude Code's format:

```json
{
  "mcpServers": {
    "server-name": {
      "command": "...",
      "args": ["..."],
      "env": { "KEY": "value" }
    }
  }
}
```

### Resolution order (per session)

Walk up from the session's working directory, then global:

```
/home/user/Workspace/Project/   ← session working dir
/home/user/Workspace/           ← .mcp.json here
/home/user/                     ← check here
~/.claude/.mcp.json             ← global
```

All discovered files are merged. Closer-to-project configs win on name conflicts. Global config is loaded once at startup. Directory walk happens per-session.

## Sola Bus Tools

Custom tools implementing claurst's `Tool` trait that interact with the Sola bus:

- `raise_app { app_id }` — brings an app to the foreground (`Topic::RaiseApp`)
- `launch_app { app_id }` — launches an app (`Topic::LaunchApp`)
- `list_apps` — queries running apps (`Topic::ListApps` / `Topic::Apps`)

Each is a thin wrapper that emits a bus topic and optionally waits for a response.

## Permissions

Auto-approve all tool executions. No permission UI. Claurst's `PermissionMode` set to unrestricted/bypass.

## Frontend

Arrow.js — tiny (~2KB) reactive UI library. No build step. HTML/CSS/JS embedded in the binary via `include_str!()`.

### Layout

```
┌──────────────┬───────────────────────────────┐
│              │                               │
│ Conversations│  Message Log                  │
│              │                               │
│ [+ New]      │  ┌─────────────────────────┐  │
│ [Search]     │  │ User message            │  │
│              │  ├─────────────────────────┤  │
│ ● Running    │  │ Assistant response      │  │
│   session-1  │  │ (streaming markdown)    │  │
│              │  ├─────────────────────────┤  │
│ Today        │  │ Tool call (collapsible) │  │
│   convo-a    │  ├─────────────────────────┤  │
│   convo-b    │  │ Assistant continues...  │  │
│              │  └─────────────────────────┘  │
│ Yesterday    │                               │
│   convo-c    ├───────────────────────────────┤
│              │  Input area                   │
│ Older        │  [textarea]        [Send]     │
│   convo-d    │                               │
└──────────────┴───────────────────────────────┘
```

### Conversation sidebar

- Grouped: Running, Today, Yesterday, This Week, Older
- Searchable (filters by name and first prompt)
- Renameable (double-click to edit)
- Click to switch active conversation
- "+" button to create new session (prompts for working directory)

### Message log

- User messages displayed as plain text
- Assistant messages rendered as markdown with syntax highlighting
- Tool calls shown as collapsible cards (tool name, input summary, output)
- Auto-scrolls when scrolled near bottom; preserves position when scrolled up
- Streaming text appended in real-time

### Input area

- Multi-line textarea
- Enter to submit, Shift+Enter for newline
- Disabled while agent is running
- Cancel button appears while agent is running

## Logging

Structured logging via `tracing` to `/opt/sola/log/sola-agent.log` and stderr.

## Build System

Standard Sola app pattern:
- Binary crate at `apps/sola-agent/`
- Discovered automatically by `cargo make build` and `cargo make deploy`
- Frontend files embedded at compile time
