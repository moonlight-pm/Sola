# Sola Agent — ACP Runner Design

**Date:** 2026-07-23  
**Status:** Approved  
**Branch:** `agent-acp-runner`

## Goal

Replace the crusty Fugu/Sakana-native harness in `crates/sola-agent` with a
**kit-native desktop ACP client**. v1 wires **Grok Build** (`grok agent stdio`)
as the only backend. Architecture stays agent-agnostic so additional ACP
backends are config later, not a rewrite.

Sola Agent is a **window onto agent sessions**, not a reimplementation of tools,
permissions, MCP, memory, or model routing.

## Product decisions

| Decision | Choice |
|---|---|
| Integration | ACP over stdio (not headless `-p`, not library embed) |
| Default backend | `grok agent stdio` |
| Multi-agent v1 | Grok-only wiring; `BackendSpec` is the extension point |
| Sessions | Hybrid: agent owns transcripts on disk; Sola owns thin overlay (pins, recents) |
| Quit behaviour | **Leader outlives UI** — sola-agent attaches via `grok agent --leader stdio`; quitting kills only the thin bridge |
| TUI interoperability | Same shared leader + `~/.grok/sessions` (multi-client when TUI uses leader) |
| Live multi-client | **Required** — `ConnectionMode::Leader`; host runs sticky `grok-leader.service` |

## Non-goals (v1)

- Reimplement tools, permit gates, Fugu/Sakana provider, MCP host
- Multi-agent picker UI
- Embedding `xai-org/grok-build` crates
- Migrating old `~/.config/sola/agent` Fugu JSONL
- Full TUI slash-command parity
- Dual live attach to a running TUI process (leader)

## Background

### Superseded stack

The previous `sola-agent` (~5k LOC) was a from-scratch pi-inspired harness against
Sakana Fugu: own SSE provider, five tools, permit gate, custom JSONL sessions.
That is the wrong backend after the pivot to Grok.

The July 2026 `.worktrees/sola-agent` tree is stale UI polish on that stack —
do not merge it.

### sola-kit pattern

Same as settings / monitor / terminal:

```
startup(APP_ID) → BusSetup → iced::application(update/view/subscription)
```

Theme, fonts, app menu, window chrome come from the kit. The agent owns only
transcript UI + ACP supervision.

### Grok integration surface

| Mode | Fit |
|---|---|
| `grok agent stdio` | **v1** — ACP JSON-RPC, streaming, interactive permissions |
| `grok -p` + streaming-json | One-shot; prompts cancel tools — wrong for desktop |
| Library link of grok-build | Monorepo, not a stable client API — reject |
| `grok agent leader` | **Future** multi-client / survive disconnect |

ACP streams: `agent_message_chunk`, `agent_thought_chunk`, `tool_call` /
`tool_call_update`, `plan`, optional `usage_update`. Permissions via
`session/request_permission`.

Status fidelity is agent-dependent. Prefer standard `usage_update`; fall back
to Grok `_meta.totalTokens` and on-disk `signals.json` when needed. Never invent
usage by counting characters.

## Architecture

```
┌─ sola-agent (iced GUI thread) ─────────────────────────┐
│  App: turns, draft, pending permission, status, list   │
│  update / view / subscription                          │
└──────────▲───────────────────────────────┬─────────────┘
           │ Msg::Acp(Event)               │ Cmd
           │ Subscription                  │ mpsc
┌──────────┴───────────────────────────────▼─────────────┐
│  Worker thread (terminal-style bridge)                 │
│  owns AgentConnection implementation                   │
└──────────┬─────────────────────────────────────────────┘
           │ v1: spawn + NDJSON stdio
┌──────────▼─────────────────────────────────────────────┐
│  grok agent stdio                                      │
│  tools · ~/.grok/sessions · permissions · model        │
└────────────────────────────────────────────────────────┘
```

### Connection modes

```rust
enum ConnectionMode {
    /// v1: private child for the app lifetime
    StdioChild { spec: BackendSpec },
    /// future: attach to long-lived leader
    Leader { socket: PathBuf },
}

struct BackendSpec {
    id: String,       // "grok"
    label: String,    // "Grok"
    command: PathBuf,
    args: Vec<String>, // ["agent", "stdio"]
}
```

v1 implements only `StdioChild` + built-in `BackendSpec::grok()`.
UI event vocabulary never sees wire JSON.

### Worker bridge

Mirror `sola-terminal` / kit `bus_subscription`:

- Process-wide `mpsc` EVENT (worker → UI) and CMD (UI → worker)
- Receiver taken once; second subscription is inert
- GUI never blocks on network or child I/O

### ACP lifecycle (v1)

1. Boot → spawn child → `initialize` (client name `sola-agent`)
2. `session/new { cwd }` or `session/load { sessionId, cwd }`
3. `session/prompt` with text content blocks
4. Stream `session/update` → UI turns
5. On `session/request_permission` → pending strip → user choice → response
6. Stop → `session/cancel`
7. Child death → error + restart affordance

### Session list (hybrid)

| Source | Role |
|---|---|
| ACP `session/new` / `session/load` | Live conversation authority |
| `~/.grok/sessions/<encoded-cwd>/<id>/summary.json` | Sidebar recents for cwd (Grok adapter) |
| `~/.config/sola/agent/overlay.json` | Pins, last-opened order — **never** transcript body |

Selecting a session loads via ACP. History for display is rebuilt from Grok’s
`updates.jsonl` when present (load response does not carry full history).

### Event vocabulary (UI-facing)

- `Connected` / `Disconnected { reason }`
- `SessionReady { id, title? }`
- `UserEcho` / `AgentDelta` / `ThoughtDelta`
- `ToolStart` / `ToolUpdate` / `ToolEnd`
- `Plan`
- `Usage { used, size? }`
- `PermissionRequired { request_id, tool, preview, options }`
- `TurnEnded { stop_reason }`
- `Error { message }`
- `SessionsListed { entries }` (optional refresh)

### Permissions

- Approval strip while pending; composer gated
- Map Allow / Deny to ACP option ids (prefer `allow_*` / `reject_*` kinds;
  fall back to first allow-like / cancel)
- “Always” if an always-allow option is offered; else hide or map best-effort

## UI (v1)

| Region | Content |
|---|---|
| Sidebar | Sessions for cwd, New, pin, select |
| Transcript | User / assistant / thought / tool / plan |
| Composer | Draft, Send, Stop |
| Approval | Tool preview + Allow / Deny [/ Always] |
| Status bar | Backend, connection mode (`local`), context used/size or %, turn state |
| First-run | Missing `grok` binary or auth failure → install / `grok login` hints |

## Auth

Prefer Grok’s existing auth (`~/.grok/auth.json`, `XAI_API_KEY`).  
No Sakana key prompt. Surface agent auth errors with login guidance.

## Logging

- Child stderr → tracing + `/opt/sola/log/sola-agent.log` (and/or agent-specific file)
- Never swallow exit codes
- Honor `RUST_LOG`

## Crate layout

Greenfield rewrite of `crates/sola-agent` (same binary name for launcher continuity):

```
src/
  main.rs           # iced app, boot, Msg
  bridge.rs         # channels + subscription
  backend.rs        # BackendSpec, ConnectionMode
  worker.rs         # command loop, connection lifecycle
  acp/
    mod.rs
    transport.rs    # spawn child, NDJSON read/write
    client.rs       # initialize / session / prompt / permission
  sessions.rs       # list Grok sessions from disk
  overlay.rs        # pins / last-opened
  view/
    mod.rs sidebar.rs bubble.rs footer.rs approval.rs firstrun.rs
```

**Delete:** `engine`, `provider`, `tools/*`, `permit`, Fugu `session` JSONL tree,
Sakana credentials.

**Dependencies:** `sola-kit`, `sola-bus`, `sola-core`, `iced`, `serde`/`serde_json`,
`tracing`. No Fugu/ureq requirement for the agent loop. Optional thin use of
`agent-client-protocol-schema` is fine; a hand-rolled NDJSON client is acceptable
to keep the worker-thread model simple and tolerate Grok extensions.

## Error handling

| Failure | Behaviour |
|---|---|
| Binary not found | First-run panel; do not crash |
| Init / auth fail | Error turn + status; restart/login hint |
| Child crash mid-turn | Disconnect event; keep transcript; Restart |
| Malformed update line | Log warn; skip line |
| Session load miss | Error toast; stay on previous or empty |

## Testing

- Unit: session index scan (temp dir with fake `summary.json`)
- Unit: map `session/update` JSON → UI events
- Unit: permission option selection
- Manual smoke: launch → new session → prompt → tool approval → resume after restart

## Migration

- Old Fugu sessions under `~/.config/sola/agent/sessions` are **orphaned** (no import)
- Overlay path is new; empty by default
- Process manager / desktop entry keep `sola-agent` name if present

## Agent leader daemon (required)

Leader is **not** a Sola-managed process. Ownership is host-side:

| Piece | Owner |
|---|---|
| `grok agent leader --no-exit-on-disconnect` | User systemd unit `grok-leader.service` (enable at login) |
| Default client policy | `[cli] use_leader = true` in `~/.grok/config.toml` (TUI + agent clients) |
| Socket | `~/.grok/leader.sock` (`GROK_LEADER_SOCKET` override) |
| sola-agent | Attach only — `ConnectionMode::Leader` + preflight; **no** private `stdio` agent |

| Desire | Behaviour |
|---|---|
| Survives UI quit | Sticky leader (`--no-exit-on-disconnect`) |
| Reconnect | sola-agent bridges via `grok agent --leader stdio` after socket preflight |
| Multi-client | TUI and Sola share one backend |
| Missing leader | `NeedSetup` — instruct `systemctl --user start grok-leader.service` (do not auto-spawn) |

## Supersedes

- `docs/specs/2026-07-07-sola-agent-pi-harness-design.md` (Fugu harness)
- `docs/specs/2026-07-07-sola-agent-pi-harness-plan.md`
- Related UI kit-conformance agent notes tied to that harness

## Acceptance (v1)

1. `cargo make build agent` succeeds  
2. App launches as kit app (theme, menu, quit)  
3. Spawns `grok agent stdio` when `grok` is available  
4. New session + prompt streams assistant text (and tools when permitted)  
5. Permission UI can allow/deny a tool call  
6. Sidebar lists Grok sessions for the project cwd  
7. Select session → load → history visible → can send follow-up  
8. Quit and relaunch → can resume the same session id  
9. No Fugu/Sakana provider code remains in the crate  
10. Design documents leader daemon as future work  
