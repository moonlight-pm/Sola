# Sola Agent — Design (pi-inspired harness)

**Date:** 2026-07-07
**Status:** Approved (design), spec review pending

## Goal

Turn the bare `crates/sola-agent` iced stub into a working **native coding
agent** for the Sola desktop: a pure-Rust iced app that runs an agent loop
against **Sakana Fugu**, executes local tools (`read`/`write`/`edit`/`bash`),
and gates dangerous actions behind an in-UI approval flow. The loop's *design*
is adapted from **pi** (pi.dev, `github.com/earendil-works/pi`) — its radical
minimalism and un-knobbed loop — but pi is TypeScript with no Rust library, so
this is a **from-scratch reimplementation of pi's ideas**, not a dependency on
pi. No Node, no WebView, no external CLI.

This replaces the retired WebView prototype (now `apocrypha/apps/agent`, a
GTK4/WebKit host that shelled out to the `claude` CLI). Nothing from that host
layer survives; a few engine-level ideas do (below).

## Non-goals (deliberately not built)

Adapting pi means adopting its "primitives, not features" posture — and because
**Fugu already orchestrates a pool of frontier models server-side**, several
things other harnesses build are redundant for us:

- **No MCP.** CLI tools + `bash` cover the same ground without the per-session
  token cost pi documents (~7–18k tokens/server).
- **No in-app sub-agents.** Fugu does model routing/delegation internally and
  opaquely; we are a single-loop client.
- **No built-in todo/plan system.** The agent edits `PLAN.md` / `TODO.md` files
  like any other file.
- **No context compaction in v1.** Fugu's context is large (272K+); v1 caps and
  warns, and compaction is added later if needed.
- **Sub-1k-token system prompt.** Role + tool descriptions + a short guideline +
  a reference to `AGENTS.md`. No mega-prompt.

These are defaults, not walls — each could return later as an opt-in.

## Background — what already exists

1. **The stub** (`crates/sola-agent/src/main.rs`). A `sola-kit`/iced 0.14 app
   with a conversations sidebar and message bubbles, wired for `Topic::Theme` /
   `MenuAction` / `CloseApp` and an "Agent" app-menu. `SendDraft` currently
   appends a canned *"Agent backend is not wired yet."* reply. We grow this;
   we do not restart it.

2. **The retired prototype** (`apocrypha/apps/agent`, on the frozen
   `apocrypha/sola-app` stack). Worth carrying forward at the engine level:
   - the **stream reader state machine** shape (`in_turn` / text accumulation /
     block tracking; delta → tool-start → tool-result → usage),
   - a clean **internal event vocabulary** as a UI protocol,
   - **batched line draining** to coalesce bursty output into fewer UI updates.

   Explicitly dropped: the WebView/`AsyncDispatcher`/glib-pump host layer; the
   three-way session reconciliation (`sync.rs`/`active.rs`) that existed only
   to keep our state in step with the CLI's own `~/.claude` JSONL; the
   hard-coded `--dangerously-skip-permissions`; and `stderr → /dev/null` (which
   violated Sola's "never lose output" rule). Note the old parser consumed
   **Anthropic** message shapes — reusable in *architecture* but not at the wire
   level, since Fugu speaks the OpenAI-compatible **Responses** API.

3. **The proven iced bridge** (`crates/sola-terminal/src/emulator.rs`). The
   template for getting an off-thread producer into iced: a process-wide
   `static NOTIFY_TX: OnceLock<mpsc::Sender<_>>` + `static NOTIFY_RX:
   Mutex<Option<Receiver>>`, exposed as a `Subscription::run(...)` stream with a
   **receiver-taken guard** (iced rebuilds the subscription set on every
   update). `pty.rs` runs the actual backend on dedicated OS threads. We mirror
   both.

4. **No HTTP stack today.** The workspace (742 crates) vendors **no** HTTP or
   network-TLS client — the only `*tls*` crates are thread-local-storage libs.
   The Sakana client is therefore a genuine, deliberate dependency addition.

5. **Secrets home.** `sola-core::Encrypted<T>` (`crates/sola-core/src/
   encrypted.rs`) — age-encrypted on disk, clear on the postcard wire — is
   where the API key lives.

6. **Process persistence.** `sola-session` relaunches the sola-agent *process*
   at boot via `Topic::SessionApps`. It does **not** persist conversation
   content — that is this app's job (mirror how terminal/browser own their
   own state).

## Architecture overview

```
                 iced GUI thread (single-threaded)
   ┌──────────────────────────────────────────────────────┐
   │  App state (transcript tree, draft, pending approval) │
   │  update(&mut self, Msg)      view(&self) -> Element   │
   └───────▲───────────────────────────────┬──────────────┘
           │ Msg::Agent(AgentEvent)         │ AgentCmd
           │ (Subscription::run)            │ (send / approve / abort)
   events out │                             │ commands in
           │                                ▼
   ┌────────┴────────────────────────────────────────────┐
   │  Agent worker  (dedicated std thread)                │
   │  ── the loop ──                                      │
   │  build Responses request (input + tools)             │
   │    → ureq POST https://api.sakana.ai/v1/responses    │
   │    → parse SSE semantic events → emit AgentEvent      │
   │    → on function_call items:                         │
   │         permit gate → run tool → function_call_output│
   │    → repeat until a response has no function_call     │
   └───────────────┬──────────────────────────────────────┘
                   │ writes transcript nodes
                   ▼
        ~/.config/sola/agent/sessions/<id>.jsonl  (tree, source of truth)
```

Two rules drive the shape: **(a)** iced's `update`/`view` runs on one thread and
must never block on network or tool I/O; **(b)** the permission gate makes the
loop *pause and wait* for a UI decision, so communication is bidirectional. That
two-way need — not the token streaming — is what makes a real bridge mandatory.

## Crate & module layout

Grow `crates/sola-agent` in small, focused files (Sola's "many small files"
rule):

| File | Responsibility |
| --- | --- |
| `main.rs` | kit boot, `App` state, `update`, top-level `view` |
| `event.rs` | `AgentEvent` (out) / `AgentCmd` (in) enums + the iced bridge (`agent_subscription`, the `OnceLock`/`Mutex` channels, `agent_send`) |
| `engine.rs` | the agent worker thread + the loop |
| `provider.rs` | Sakana Fugu Responses client: request build, `ureq` POST, SSE parse |
| `tools/mod.rs` + `tools/{read,write,edit,bash,search}.rs` | the tool fns + their JSON function-schemas |
| `permit.rs` | the permission gate chain + session policy |
| `session.rs` | the transcript tree, node schema, JSONL load/append, branching |
| `view/` | conversation view, message bubbles, approval prompt, session/branch UI |

No provider trait, no plugin abstraction — one concrete path, per "no
speculative abstractions." Because Fugu is OpenAI-shaped, retargeting a
different OpenAI-compatible base later is a config change (base URL + key +
model), not a rewrite.

## The agent loop (pi's loop, Responses terms)

One turn:

1. Build the request from the **active branch** of the transcript tree (see
   Sessions): `model`, `input` (the reconstructed conversation), `tools`,
   `stream:true`, `store:false`, `reasoning:{effort}`.
2. POST and stream. Forward text deltas as `AgentEvent::Delta`; surface any
   reasoning summary as `AgentEvent::Reasoning` (display-only).
3. Collect any `function_call` items the model emits.
4. For each function call: run it through the **permit gate** (which may
   auto-allow, classify, or block on user approval), then execute the tool
   locally, producing a `function_call_output`.
5. Append the assistant's `function_call` items and our `function_call_output`
   items to the transcript, then **loop back to step 1** — until a response
   completes with **no** function calls (`finish` with only a message). Emit
   `AgentEvent::TurnEnd { usage }`.

No `max_steps` knob (pi's stance). **Abort** is an `AgentCmd::Abort` checked
between steps and honored during the streaming read (drop the response / set an
atomic flag); a partial turn is persisted as-is.

## Provider — Sakana Fugu (Responses API)

- **Endpoint:** `POST https://api.sakana.ai/v1/responses`
- **Auth:** `Authorization: Bearer <key>`, `Content-Type: application/json`
- **Models:** `fugu` (fast default) and `fugu-ultra-20260615` (deep). Selectable
  per conversation; `fugu` is also used by the risk classifier.
- **Reasoning:** `reasoning: { effort: "high" | "xhigh" | "max" }`.
- **`store: false`** — we do **not** rely on server-side response retention or
  `previous_response_id`. Our local JSONL tree is the source of truth and we
  resend the active branch as `input` each turn. This keeps branching purely
  client-side and provider-independent.

**Request `input`** is an ordered array of items rebuilt from the active branch:
input messages (`{role, content:[{type:"input_text", text}]}`), prior assistant
`function_call` items, and our `function_call_output` items
(`{type:"function_call_output", call_id, output}`).

**`tools`** are flat function tools:
`{type:"function", name, description, parameters:<JSON Schema>, strict:true}`.

**Streaming** parses OpenAI Responses *semantic events* (`event: <type>` +
`data: <json>`): `response.output_text.delta` → text delta;
`response.function_call_arguments.delta` / `.done` → accumulate a call's args;
`response.output_item.done` → a completed `function_call` (with `call_id`,
`name`, `arguments`); `response.completed` → turn end + `usage`; error events →
`AgentEvent::Error`.

> **Implementation checkpoint (do this first).** The exact Responses **function-
> call streaming event names and client-tool round-trip** must be confirmed
> against Sakana's live API before the loop is built — Sakana advertises
> Responses as the "broader compatibility" path, so this is expected to hold,
> but it is the single fact the whole loop depends on. A one-file spike
> (send one tool, print raw SSE, send one `function_call_output`) de-risks
> everything downstream. Fugu's internal routing is opaque; reasoning items are
> treated as display-only and not round-tripped.

**HTTP client:** **`ureq` + rustls**, called *blocking* from the worker thread
(the loop is synchronous; the thread already exists, mirroring `pty.rs`). SSE =
read the response body and split on `\n`. This keeps the dependency small and
avoids pulling an async HTTP stack (hyper/h2/tower) into a minimalist workspace.
Pin rustls' crypto provider explicitly for NixOS reproducibility.

**Auth storage:** the key is held as `Encrypted<String>` at
`~/.config/sola/agent/credentials` (age-encrypted on disk). Fallback:
`SAKANA_API_KEY` from the environment. A missing key surfaces as a first-run
prompt in the UI, not a crash.

## Tools

pi's four primitives plus the safe read-only variants (kept **enabled** — a GUI
benefits and they carry no write risk):

- `read(path, [range])` — file contents.
- `write(path, content)` — create/overwrite.
- `edit(path, old, new)` — exact-string replacement.
- `bash(command)` — run via `Command::new("sh").arg("-c")`, output **captured**
  (never `/dev/null`) and logged; cwd = the session's project root.
- read-only `grep` / `find` / `ls`.

Each tool is a Rust fn returning a **split result**: the text the model sees vs.
a richer structured detail for the UI (pi's idea — e.g. a diff for `edit`, exit
code + captured streams for `bash`).

**Project root.** Every conversation has a working directory chosen at session
start (defaults to the launcher's cwd / `$HOME`). It scopes `bash` and defines
"inside vs. outside the project" for the permission policy — without it the
policy is meaningless.

## Permission gate chain

Evaluated for every tool call before execution (pi's `beforeToolCall` seam),
as an ordered, configurable chain:

1. **Static policy.** Auto-allow read-only tools and `write`/`edit` whose target
   resolves **inside the project root**. Require a decision for `bash` and for
   writes **outside** the root.
2. **Optional LLM risk classifier ("auto" mode).** For any action that would
   otherwise prompt, first screen the concrete command/diff with a cheap,
   non-streaming `fugu` call that returns a JSON verdict `safe|caution|danger`
   + reason. `safe` auto-runs; anything else falls through to the prompt.
   Toggleable (off by default until validated); its own token cost is disclosed
   in the UI.
3. **User approval.** Emit `AgentEvent::ApprovalRequest { call_id, tool,
   preview }`; the view shows the exact command or diff with **Approve /
   Deny / Always-allow-this-kind**. The loop blocks on `AgentCmd::Approve` /
   `AgentCmd::Deny`. "Always" appends a rule to the session policy (in-memory +
   persisted with the session). Deny returns a `function_call_output` telling
   the model the action was declined, so it can adapt.

Every decision (and classifier verdict) is logged. This is precisely the piece
pi omits by design; building it is the point of a *desktop* agent.

## Sessions — tree model & persistence

**Node schema** (one JSONL line per node), pi-style:

```
{ "id": "<uuid>", "parentId": "<uuid|null>", "role": "user|assistant|tool",
  "content": <text | function_call | function_call_output>,
  "model": "fugu-ultra-...", "usage": {...}?, "ts": <ms> }
```

- A session file is a **tree**: nodes reference `parentId`; an **active-leaf**
  pointer marks the current head. Appending a turn adds children under the leaf
  and advances it.
- **Branching** = selecting an earlier node (e.g. editing/resubmitting a past
  user turn) and appending a *new* child off that parent. No file duplication;
  the old branch remains. The request `input` for any turn is the path
  **root → active leaf**, which is why local-source-of-truth + `store:false`
  makes branching a pure client-side path rebuild.
- **Storage:** append-only JSONL at `~/.config/sola/agent/sessions/<id>.jsonl`.
  Kept off the bus (postcard payloads must stay small); the bus only ever sees
  small deltas via the subscription, never the transcript.
- **Session index:** a lightweight `sessions/index.json` (id, title, updated-at,
  project root) for the sidebar, rebuilt from the files if lost.

**UI scope for v1:** the tree *data model* is fully v1. The *navigation UI*
starts minimal — branch-from-a-past-message and a simple branch switcher — and
can grow toward a full `/tree`-style view. This is the largest single build item
in v1; it is explicitly allowed to start simple.

## The bridge (iced integration)

`event.rs` owns it, mirroring `emulator.rs`:

- **events out** (worker → GUI): `static AGENT_TX: OnceLock<mpsc::Sender<
  AgentEvent>>` + `static AGENT_RX: Mutex<Option<Receiver>>`, drained by
  `agent_subscription() -> Subscription<AgentEvent>` via `Subscription::run`,
  with the receiver-taken guard. `App::subscription` batches
  `bus_subscription().map(Msg::Bus)` + `agent_subscription().map(Msg::Agent)`.
- **commands in** (GUI → worker): `static AGENT_CMD_TX: OnceLock<mpsc::Sender<
  AgentCmd>>`; `update` calls `agent_send(cmd)` on Send / Approve / Deny /
  Abort. The worker owns the matching `Receiver` and blocks on it between and
  during turns (e.g. while awaiting an approval).

The worker thread is spawned once at startup and owns the engine, so the loop's
lifetime is decoupled from any single `update`. `App::update` folds
`AgentEvent`s into transcript state and returns `Task::none()`; `view`
re-renders from state; only small deltas travel in `Msg`.

> **Note.** For a single active conversation, iced's lighter `Task::stream`
> could replace the global-static Subscription. We use the `emulator.rs` pattern
> deliberately because approvals require a back-channel *into* a running turn
> (and to leave room for concurrent sessions). If v1 makes approvals a
> between-turns affair, the lighter path is a valid simplification to revisit.

## Event & command vocabulary

```
AgentEvent (worker → UI):
  Delta { node_id, text }            // streamed assistant text
  Reasoning { text }                 // display-only reasoning summary
  ToolStart { call_id, tool, args }
  ToolOutput { call_id, chunk }      // batched tool stdout/stderr
  ToolEnd { call_id, result }
  ApprovalRequest { call_id, tool, preview }
  TurnEnd { usage }
  Error { message }

AgentCmd (UI → worker):
  Send { text }                      // + optional branch-from node
  Approve { call_id, remember: bool }
  Deny { call_id, reason? }
  Abort
  SetModel { model, effort }
```

## UI (view)

Grows the existing stub: sidebar of sessions (+ new); a transcript pane of
user/assistant/tool bubbles (tool calls render their split detail — diffs,
`bash` output). Live turns append via `Delta`. An **approval strip/modal**
shows the pending action with Approve/Deny/Always. A small footer shows model,
`reasoning.effort`, and running token usage. Follow Sola constraints: iced
`Shadow` quads don't blur in this stack — use borders/fills, not drop shadows.
Theme via `theme_from_bus` on `Topic::Theme`; quit via `MenuAction`/`CloseApp`.

## Error handling & logging

- Network / API errors → `AgentEvent::Error`, shown inline, turn preserved;
  retriable. Nothing swallowed.
- `bash`/tool failures → captured (code + streams) and returned to the model as
  the tool result, so it can react — and logged.
- All logs via `tracing` to Sola's log path; never to `/dev/null` (the legacy
  app's mistake). Structured fields: session id, node id, tool, call id.

## Testing

- **Pure units, no network:** SSE event parsing (golden Responses streams,
  incl. split function-call deltas); `input` reconstruction from a tree path;
  branching (append child off a non-leaf); the static permission policy
  (in/out-of-root, bash); `edit` exact-match semantics; title summarizer.
- **Provider** behind a seam that reads canned SSE fixtures so the loop is
  testable without hitting Sakana.
- **Manual smoke** (user-run, per Sola rules — no auto-install): a real turn
  that reads a file, proposes an edit (approval), runs a `bash`, branches.

## Build sequence (phases, structure only)

1. **Responses spike** — confirm the streaming + function-call round-trip
   against the live API (the checkpoint above). Gate on this.
2. **Provider + loop** — `provider.rs` (ureq/rustls, SSE parse) and `engine.rs`
   (loop, worker thread), tested against fixtures; text-only turns first.
3. **Bridge + minimal UI** — `event.rs` channels, wire streaming into the stub's
   view. First visible streamed reply.
4. **Tools** — the four + read-only, split results, project root.
5. **Permission gate** — static policy → approval UI → the optional classifier.
6. **Sessions** — tree schema, JSONL persistence, branching + minimal nav UI.
7. **Polish** — model/effort switch, usage footer, errors, first-run key prompt.

## Open questions / risks

- **The Responses function-call contract** is the load-bearing unknown; phase 1
  exists to close it before anything is built on top.
- **Classifier value** — the LLM risk screen may not earn its token cost; it
  ships off by default and stays optional.
- **Tree nav UI scope** — easy to over-build; v1 stays minimal by intent.
- **rustls crypto provider on NixOS** — pin explicitly to avoid init surprises
  from a bare TTY.
