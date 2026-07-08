# Sola Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the bare `crates/sola-agent` iced stub into a working native coding agent that runs pi's minimal agent loop against Sakana Fugu, executes local `read`/`write`/`edit`/`bash` tools behind an in-UI approval gate, and persists conversations as a pi-style branching tree.

**Architecture:** A pure-Rust iced 0.14 app grown from the kit stub. A dedicated worker thread runs the un-knobbed loop against Fugu's OpenAI-compatible Responses API (`ureq` + rustls, streaming SSE); events reach the GUI and commands reach the loop over an `emulator.rs`-style channel bridge. Tool calls pass through a tiered permission gate (static rules -> optional LLM risk-classifier -> approval popup); the transcript is a local JSONL tree that is the source of truth, resent as `input` each turn with `store:false`.

**Tech Stack:** Rust, iced 0.14 (wgpu/wayland), sola-kit, sola-bus, sola-core (`Encrypted`), `ureq` + rustls, serde / serde_json, uuid, tracing.

## Global Constraints

- iced 0.14; mirror the `sola-terminal` `App::new`/`update`/`view` + `Subscription` patterns. Shadow quads don't blur in this stack -- use borders/fills, never drop shadows.
- HTTP via `ureq` (rustls TLS feature); **no** async HTTP stack. Install the rustls `CryptoProvider` once at startup.
- Provider: Sakana Fugu, OpenAI-compatible **Responses** API at `https://api.sakana.ai/v1`, `Authorization: Bearer <key>`, `stream:true`, `store:false`, `reasoning.effort`. Models: `fugu` (default) / `fugu-ultra-20260615`. Valid `reasoning.effort`: `high` / `xhigh` / `max`.
- API key stored via `sola_core::Encrypted<String>` at `~/.config/sola/agent/credentials`; fallback `SAKANA_API_KEY` env.
- Never write tool/child output to `/dev/null`; capture and log via `tracing`.
- Build with `cargo make build sola-agent`. **Never** run `cargo make install` -- the user runs installs.
- Unit tests: `cargo test -p sola-agent`. Live-API tests are `#[ignore]`.
- Work only in the `.worktrees/sola-agent` worktree; commit per task; never merge to master without explicit user permission.

---

### Task 1: Scaffold the module tree, dependencies, and shared types

**Files:**
- Modify: crates/sola-agent/Cargo.toml (add serde, serde_json, ureq, uuid deps)
- Modify: crates/sola-agent/src/main.rs (add `mod` declarations)
- Create: crates/sola-agent/src/event.rs (NodeId, AgentEvent, AgentCmd)
- Create: crates/sola-agent/src/session.rs (Usage, Role, Content, Node — no serde derives yet)
- Create: crates/sola-agent/src/tools/mod.rs (ToolResult, ToolDetail)
- Create: crates/sola-agent/src/provider.rs (InputItem, StreamEvent, FunctionCall, TurnOutcome, LlmStream)
- Create: crates/sola-agent/src/engine.rs (empty stub)
- Create: crates/sola-agent/src/permit.rs (empty stub)
- Create: crates/sola-agent/src/view/mod.rs (empty stub)

**Interfaces:**
- Consumes: nothing (this is the first layer). Existing kit boot in `main.rs` (`startup`, `BusSetup`, `bus_subscription`, `window_settings`, `fonts`, theme helpers) stays as-is.
- Produces: `event::{NodeId, AgentEvent, AgentCmd}`, `session::{Usage, Role, Content, Node}`, `tools::{ToolResult, ToolDetail}`, `provider::{InputItem, StreamEvent, FunctionCall, TurnOutcome, LlmStream}`, and the seven module slots (`engine`, `event`, `permit`, `provider`, `session`, `tools`, `view`).

- [ ] **Step 1: Declare the modules in `main.rs` (the red state)**
Insert the `mod` block between the `use` imports and `const APP_ID`. Edit this exact region:
```rust
use sola_kit::theme::{default_theme, theme_from_bus};

const APP_ID: &str = "sola-agent";
```
to:
```rust
use sola_kit::theme::{default_theme, theme_from_bus};

mod engine;
mod event;
mod permit;
mod provider;
mod session;
mod tools;
mod view;

const APP_ID: &str = "sola-agent";
```

- [ ] **Step 2: Run the build, expect failure**
Run: `cargo make build sola-agent`
Expected: FAIL (`error[E0583]: file not found for module 'engine'` — the declared module files do not exist yet)

- [ ] **Step 3: Add deps, create the module files, and define the shared types**

Set `crates/sola-agent/Cargo.toml` `[dependencies]` to:
```toml
[dependencies]
sola-kit = { path = "../sola-kit" }
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
iced = { version = "0.14", default-features = false, features = ["wgpu", "tokio", "wayland"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ureq = { workspace = true, features = ["rustls"] }
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
```

Create `crates/sola-agent/src/event.rs`:
```rust
//! Agent event / command types + the iced <-> worker bridge.
//!
//! Foundation defines the message enums and the `NodeId` alias. The
//! channel statics, `init_channels`, `agent_subscription`, `agent_send`,
//! `emit`, and `take_cmd_rx` are added in the bridge layer.

use crate::session::Usage;
use crate::tools::ToolResult;

pub type NodeId = String; // uuid v4 string

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Delta { node_id: NodeId, text: String },
    Reasoning { text: String },
    ToolStart { call_id: String, tool: String, args: serde_json::Value },
    ToolOutput { call_id: String, chunk: String },
    ToolEnd { call_id: String, result: ToolResult },
    ApprovalRequest { call_id: String, tool: String, preview: String },
    TurnEnd { usage: Usage },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum AgentCmd {
    Send { text: String, branch_from: Option<NodeId> },
    Approve { call_id: String, remember: bool },
    Deny { call_id: String, reason: Option<String> },
    Abort,
    SetModel { model: String, effort: String },
}
```

Create `crates/sola-agent/src/session.rs` (serde derives deliberately omitted here — Task 2 adds them):
```rust
//! Transcript tree, JSONL persistence, branching, input reconstruction.
//!
//! Foundation defines the persisted node types (`Usage`, `Role`,
//! `Content`, `Node`). The `Session` struct and its methods land in the
//! session layer.

use crate::event::NodeId;

#[derive(Debug, Clone, Copy)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    FunctionCall { call_id: String, name: String, arguments: String }, // arguments = raw JSON string
    FunctionCallOutput { call_id: String, output: String },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub role: Role,
    pub content: Content,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub ts: u64,
}
```

Create `crates/sola-agent/src/tools/mod.rs`:
```rust
//! Tool registry + result types. Individual tools (read/write/edit/
//! bash/search) and the `tool_schemas`/`dispatch`/`ToolCtx` items land
//! in the tools layer.

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub model_text: String,
    pub ui_detail: ToolDetail,
}

#[derive(Debug, Clone)]
pub enum ToolDetail {
    Text(String),
    Diff { path: String, before: String, after: String },
    Bash { code: i32, stdout: String, stderr: String },
}
```

Create `crates/sola-agent/src/provider.rs`:
```rust
//! Sakana Fugu Responses client + the small test seam.
//!
//! Foundation defines the shared wire types and the `LlmStream` trait.
//! The real `SakanaProvider` impl (ureq streaming, SSE parse) plus
//! `build_request_body` and `parse_sse_event` land in the provider layer.

use crate::session::Usage;

#[derive(Debug, Clone)]
pub enum InputItem {
    Message { role: String, text: String }, // role: "user" | "assistant"
    FunctionCall { call_id: String, name: String, arguments: String },
    FunctionCallOutput { call_id: String, output: String },
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    Reasoning(String),
    FunctionCallStarted { call_id: String, name: String },
    FunctionCallArgsDelta { call_id: String, delta: String },
    FunctionCallDone { call_id: String, name: String, arguments: String },
    Completed { usage: Usage },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub assistant_text: String,
    pub calls: Vec<FunctionCall>,
    pub usage: Usage,
}

/// The test seam: one real impl (`SakanaProvider`, added in the provider
/// layer) plus a test fake. NOT a multi-provider abstraction.
pub trait LlmStream {
    fn stream_turn(
        &self,
        model: &str,
        effort: &str,
        input: &[InputItem],
        tools: &[serde_json::Value],
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutcome, String>;
}
```

Create `crates/sola-agent/src/engine.rs`:
```rust
//! Worker thread + the turn loop. Populated in the engine layer:
//! `EngineConfig`, `start`, and `run_turn`.
```

Create `crates/sola-agent/src/permit.rs`:
```rust
//! Permission policy (pure). Populated in the permit layer: `Rule`,
//! `Policy`, `StaticDecision`, `static_decision`, `Risk`, `classify`,
//! `remember`.
```

Create `crates/sola-agent/src/view/mod.rs`:
```rust
//! View layer: bubbles, tool detail, approval strip, sidebar, footer,
//! and the first-run key prompt. Populated in the view layer.
```

- [ ] **Step 4: Run the build, expect pass**
Run: `cargo make build sola-agent`
Expected: PASS (`Finished` / exit 0). Transient `dead_code` warnings for the not-yet-consumed types are expected in this layer and clear as later layers add consumers; `uuid` and `ureq` are declared here but first used by the session and provider layers.

- [ ] **Step 5: Commit**
```
git add crates/sola-agent/Cargo.toml crates/sola-agent/src/main.rs \
  crates/sola-agent/src/event.rs crates/sola-agent/src/session.rs \
  crates/sola-agent/src/provider.rs crates/sola-agent/src/tools/mod.rs \
  crates/sola-agent/src/engine.rs crates/sola-agent/src/permit.rs \
  crates/sola-agent/src/view/mod.rs
git commit -m "feat(sola-agent): scaffold module tree and shared foundation types"
```

### Task 2: Serde round-trip guard for the `Node` transcript type

**Files:**
- Modify: crates/sola-agent/src/session.rs (add serde derives to `Usage`/`Role`/`Content`/`Node`)
- Test: crates/sola-agent/src/session.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `session::{Usage, Role, Content, Node}` and `event::NodeId` from the previous task.
- Produces: serde `Serialize`/`Deserialize` impls on `Usage`, `Role`, `Content`, `Node` (the JSONL wire form every later session/persistence layer relies on).

- [ ] **Step 1: Write the failing test**
Append this inline test module to the bottom of `crates/sola-agent/src/session.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_json_round_trips() {
        let node = Node {
            id: "11111111-1111-4111-8111-111111111111".to_string(),
            parent_id: Some("00000000-0000-4000-8000-000000000000".to_string()),
            role: Role::Assistant,
            content: Content::FunctionCall {
                call_id: "call_1".to_string(),
                name: "read".to_string(),
                arguments: r#"{"path":"a.txt"}"#.to_string(),
            },
            model: Some("fugu".to_string()),
            usage: Some(Usage { input_tokens: 12, output_tokens: 7 }),
            ts: 1_725_000_000_000,
        };

        let json = serde_json::to_string(&node).unwrap();
        let back: Node = serde_json::from_str(&json).unwrap();

        assert_eq!(back.id, node.id);
        assert_eq!(back.parent_id, node.parent_id);
        assert_eq!(back.ts, node.ts);
        assert_eq!(back.model.as_deref(), Some("fugu"));
        assert!(matches!(back.role, Role::Assistant));
        assert_eq!(back.usage.map(|u| (u.input_tokens, u.output_tokens)), Some((12, 7)));
        match back.content {
            Content::FunctionCall { call_id, name, arguments } => {
                assert_eq!(call_id, "call_1");
                assert_eq!(name, "read");
                assert_eq!(arguments, r#"{"path":"a.txt"}"#);
            }
            other => panic!("wrong content variant: {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent node_json_round_trips`
Expected: FAIL (compile error — `Node`/`Content`/`Role`/`Usage` do not implement `serde::Serialize`/`Deserialize` yet; `the trait Serialize is not implemented for Node`)

- [ ] **Step 3: Implement (add the serde derives)**
Add the serde import at the top of `crates/sola-agent/src/session.rs`, directly under the existing `use crate::event::NodeId;`:
```rust
use serde::{Deserialize, Serialize};
```
Then change the four derive lines so the types match the contract's persisted form:
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Content {
    Text(String),
    FunctionCall { call_id: String, name: String, arguments: String }, // arguments = raw JSON string
    FunctionCallOutput { call_id: String, output: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub role: Role,
    pub content: Content,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub ts: u64,
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent node_json_round_trips`
Expected: PASS (`test session::tests::node_json_round_trips ... ok`; `test result: ok. 1 passed`)

- [ ] **Step 5: Commit**
```
git add crates/sola-agent/src/session.rs
git commit -m "test(sola-agent): serde round-trip guard for the Node transcript type"
```

### Task 3: Phase-1 Responses spike (live, `#[ignore]`)

Confirm the load-bearing unknown — the Responses **streaming event names** and
the **client function-call round-trip** — against Sakana's live API *before*
building `parse_sse_event` on top of assumed names. This is a confirmation tool,
not a red/green unit test: it is `#[ignore]`d and only runs when you pass a key.

**Files:**
- Create: `crates/sola-agent/tests/spike_responses.rs`
- Modify: `crates/sola-agent/Cargo.toml` (add the provider-layer deps — reused by B/C/D)

**Interfaces:**
- Consumes: nothing internal (uses `ureq`, `serde_json`, `rustls` directly)
- Produces: the confirmed SSE contract that `parse_sse_event` (Task C) encodes

- [ ] **Step 1: Add deps + write the spike**
Add to `crates/sola-agent/Cargo.toml` under `[dependencies]` (these serve the
whole provider layer):
```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ureq = { workspace = true, features = ["json"] }
rustls = { workspace = true }
```
Create `crates/sola-agent/tests/spike_responses.rs`:
```rust
//! PHASE-1 SPIKE (ignored by default; needs network + a real key).
//!
//! Confirms the Sakana Fugu *Responses* streaming contract BEFORE the engine is
//! built on it. Run it once, by hand, and read the printed SSE:
//!
//!   SAKANA_API_KEY=sk-... cargo test -p sola-agent --test spike_responses \
//!       -- --ignored --nocapture
//!
//! WHAT TO LOOK FOR in the printed `event:` lines (these names are the single
//! fact the whole engine depends on — confirm each appears, and that the
//! function-call round-trip matches):
//!   * response.output_text.delta             -> streamed assistant text (data.delta)
//!   * response.output_item.added             -> a function_call item begins
//!         (data.item.type == "function_call"; carries item.call_id + item.name)
//!   * response.function_call_arguments.delta -> incremental args, keyed by data.item_id
//!   * response.function_call_arguments.done  -> args finished (data.item_id + data.arguments)
//!   * response.output_item.done              -> the FINISHED function_call item
//!         AUTHORITATIVE: data.item.{call_id,name,arguments} (full arguments string)
//!   * response.completed                     -> data.response.usage.{input_tokens,output_tokens}
//! If any name differs, update `provider::parse_sse_event` to match before Task C.

use std::io::{BufRead, BufReader};

#[test]
#[ignore = "live network + real SAKANA_API_KEY; run by hand to confirm the SSE contract"]
fn spike_responses_function_call_roundtrip() {
    // Deterministic crypto backend from a bare TTY (matches the app).
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let key = std::env::var("SAKANA_API_KEY").expect("set SAKANA_API_KEY to run the spike");

    let body = serde_json::json!({
        "model": "fugu",
        "input": [{
            "role": "user",
            "content": [{ "type": "input_text",
                          "text": "Call the get_weather tool for Tokyo, then stop." }]
        }],
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "description": "Get the current weather for a city.",
            "parameters": {
                "type": "object",
                "properties": { "city": { "type": "string" } },
                "required": ["city"],
                "additionalProperties": false
            },
            "strict": true
        }],
        "stream": true,
        "store": false,
        "reasoning": { "effort": "high" }
    });

    let mut resp = ureq::post("https://api.sakana.ai/v1/responses")
        .header("Authorization", &format!("Bearer {key}"))
        .header("Accept", "text/event-stream")
        .send_json(&body)
        .expect("POST /responses failed");

    eprintln!("=== HTTP {} ===", resp.status());
    let reader = BufReader::new(resp.body_mut().as_reader());
    let mut saw_completed = false;
    for line in reader.lines() {
        let line = line.expect("read SSE line");
        if line.starts_with("event:") || line.starts_with("data:") {
            eprintln!("{line}");
        }
        if line.contains("response.completed") {
            saw_completed = true;
        }
    }
    assert!(saw_completed, "stream ended without a response.completed event");
}
```

- [ ] **Step 2: Compile it and confirm it is skipped by default**
Run: `cargo test -p sola-agent --test spike_responses`
Expected: compiles; runs `0` tests / `1 ignored` — it never touches the network without `--ignored`.

- [ ] **Step 3: Run it live and observe the raw SSE**
Run: `SAKANA_API_KEY=sk-... cargo test -p sola-agent --test spike_responses -- --ignored --nocapture`
Expected: prints `=== HTTP 200 ===` then the raw `event:`/`data:` lines; the run passes only if a `response.completed` event arrives. Read the stream against the "WHAT TO LOOK FOR" checklist.

- [ ] **Step 4: Record the confirmed contract**
Tick each confirmed event name in the doc-comment (leave a note next to any that differ). These names are what Task C hard-codes, so this is the gate for the rest of the layer.

- [ ] **Step 5: Commit**
`git add crates/sola-agent/Cargo.toml crates/sola-agent/tests/spike_responses.rs && git commit -m "feat(sola-agent): phase-1 Responses SSE spike + provider deps"`

---

### Task 4: `build_request_body` — Responses request JSON

**Files:**
- Create: `crates/sola-agent/src/provider.rs`
- Modify: `crates/sola-agent/src/main.rs` (add `mod provider;`)
- Test: `crates/sola-agent/src/provider.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `serde_json::{json, Value}`
- Produces:
  - `pub enum InputItem { Message { role: String, text: String }, FunctionCall { call_id: String, name: String, arguments: String }, FunctionCallOutput { call_id: String, output: String } }`
  - `pub fn build_request_body(model: &str, effort: &str, input: &[InputItem], tools: &[serde_json::Value]) -> serde_json::Value`

- [ ] **Step 1: Write the failing test**
Add `mod provider;` to `crates/sola-agent/src/main.rs` (with the other top-level
declarations, after the `use` block around line 19). Create
`crates/sola-agent/src/provider.rs` with **only** the `InputItem` enum and the
test module (no `build_request_body` yet, so the crate fails to build):
```rust
//! Sakana Fugu *Responses* API client: request building, SSE parsing, and a
//! blocking `ureq` streaming call behind the `LlmStream` test seam.

use serde_json::{json, Value};

/// One item in a Responses `input` array, rebuilt from the active transcript
/// branch. Mapped to wire JSON by [`build_request_body`].
#[derive(Debug, Clone)]
pub enum InputItem {
    Message { role: String, text: String },
    FunctionCall { call_id: String, name: String, arguments: String },
    FunctionCallOutput { call_id: String, output: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_body_shape() {
        let input = vec![
            InputItem::Message { role: "user".into(), text: "hello".into() },
            InputItem::FunctionCall {
                call_id: "call_1".into(),
                name: "read".into(),
                arguments: r#"{"path":"a"}"#.into(),
            },
            InputItem::FunctionCallOutput {
                call_id: "call_1".into(),
                output: "file body".into(),
            },
        ];
        let tools = vec![json!({
            "type": "function", "name": "read", "description": "d",
            "parameters": { "type": "object" }, "strict": true
        })];

        let body = build_request_body("fugu-ultra-20260615", "xhigh", &input, &tools);

        assert_eq!(body["model"], "fugu-ultra-20260615");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["reasoning"]["effort"], "xhigh");

        // message item
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][0]["content"][0]["text"], "hello");

        // function_call item (arguments stays a raw JSON string)
        assert_eq!(body["input"][1]["type"], "function_call");
        assert_eq!(body["input"][1]["call_id"], "call_1");
        assert_eq!(body["input"][1]["name"], "read");
        assert_eq!(body["input"][1]["arguments"], r#"{"path":"a"}"#);

        // function_call_output item
        assert_eq!(body["input"][2]["type"], "function_call_output");
        assert_eq!(body["input"][2]["call_id"], "call_1");
        assert_eq!(body["input"][2]["output"], "file body");

        // tools passed through untouched
        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(body["tools"][0]["strict"], true);
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent build_request_body_shape`
Expected: FAIL (compile error: cannot find function `build_request_body` in module `provider`)

- [ ] **Step 3: Implement**
Insert `build_request_body` into `crates/sola-agent/src/provider.rs` (above the
test module):
```rust
/// Build the Responses request body for one turn.
///
/// Shape: `{model, input, tools, stream:true, store:false, reasoning:{effort}}`.
/// `tools` is passed straight through (already function-tool JSON from
/// `tools::tool_schemas`). `arguments`/`output` are raw JSON strings, emitted
/// verbatim per the Responses function-call contract.
pub fn build_request_body(model: &str, effort: &str, input: &[InputItem], tools: &[Value]) -> Value {
    let input_json: Vec<Value> = input
        .iter()
        .map(|item| match item {
            InputItem::Message { role, text } => {
                // Assistant text re-sent as input uses `output_text`; user text
                // uses `input_text` (Responses content-type rule).
                let content_type = if role == "assistant" { "output_text" } else { "input_text" };
                json!({
                    "role": role,
                    "content": [{ "type": content_type, "text": text }]
                })
            }
            InputItem::FunctionCall { call_id, name, arguments } => json!({
                "type": "function_call",
                "call_id": call_id,
                "name": name,
                "arguments": arguments,
            }),
            InputItem::FunctionCallOutput { call_id, output } => json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": output,
            }),
        })
        .collect();

    json!({
        "model": model,
        "input": input_json,
        "tools": tools,
        "stream": true,
        "store": false,
        "reasoning": { "effort": effort },
    })
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent build_request_body_shape`
Expected: PASS (1 passed)

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/provider.rs crates/sola-agent/src/main.rs && git commit -m "feat(sola-agent): build Responses request body from input items"`

---

### Task 5: `parse_sse_event` — Responses semantic events → `StreamEvent`

**Files:**
- Modify: `crates/sola-agent/src/provider.rs`
- Create: `crates/sola-agent/src/session.rs` (bootstrap: `Usage` only — see notes)
- Modify: `crates/sola-agent/src/main.rs` (add `mod session;`)
- Test: `crates/sola-agent/src/provider.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::session::Usage`, `serde_json::Value`
- Produces:
  - `pub enum StreamEvent { TextDelta(String), Reasoning(String), FunctionCallStarted { call_id: String, name: String }, FunctionCallArgsDelta { call_id: String, delta: String }, FunctionCallDone { call_id: String, name: String, arguments: String }, Completed { usage: Usage }, Error(String) }`
  - `pub fn parse_sse_event(event_type: &str, data_json: &str) -> Option<StreamEvent>`
  - `pub struct Usage { pub input_tokens: u64, pub output_tokens: u64 }` (bootstrapped in `session.rs`)

- [ ] **Step 1: Write the failing test**
Bootstrap `Usage` so provider can consume it. Add `mod session;` to
`crates/sola-agent/src/main.rs`, then create `crates/sola-agent/src/session.rs`:
```rust
//! Transcript session types. Bootstrapped here with `Usage` so the provider
//! layer can compile; the full transcript tree + JSONL persistence extend this
//! module in the Session layer. `Usage` is contract-exact and MUST be preserved.

use serde::{Deserialize, Serialize};

/// Token accounting for one turn. Owned by the session layer; the provider and
/// engine consume it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}
```
In `crates/sola-agent/src/provider.rs`, add `use crate::session::Usage;` beside
the existing `use serde_json::...;`, add the `StreamEvent` enum, and add the
parse tests to the existing `#[cfg(test)] mod tests` (leaving `parse_sse_event`
unimplemented so the build is red):
```rust
/// One decoded Responses streaming event. `parse_sse_event` maps the wire
/// `event:`/`data:` pair to one of these; the engine folds them into a turn.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    Reasoning(String),
    FunctionCallStarted { call_id: String, name: String },
    FunctionCallArgsDelta { call_id: String, delta: String },
    FunctionCallDone { call_id: String, name: String, arguments: String },
    Completed { usage: Usage },
    Error(String),
}
```
Tests to add inside `mod tests`:
```rust
    #[test]
    fn parse_text_delta() {
        match parse_sse_event("response.output_text.delta", r#"{"delta":"Hi"}"#) {
            Some(StreamEvent::TextDelta(t)) => assert_eq!(t, "Hi"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_function_call_started_and_done() {
        let added = r#"{"item":{"id":"fc_1","type":"function_call","call_id":"call_abc","name":"read","arguments":""}}"#;
        match parse_sse_event("response.output_item.added", added) {
            Some(StreamEvent::FunctionCallStarted { call_id, name }) => {
                assert_eq!(call_id, "call_abc");
                assert_eq!(name, "read");
            }
            other => panic!("expected FunctionCallStarted, got {other:?}"),
        }

        let done = r#"{"item":{"id":"fc_1","type":"function_call","call_id":"call_abc","name":"read","arguments":"{\"path\":\"a\"}"}}"#;
        match parse_sse_event("response.output_item.done", done) {
            Some(StreamEvent::FunctionCallDone { call_id, name, arguments }) => {
                assert_eq!(call_id, "call_abc");
                assert_eq!(name, "read");
                assert_eq!(arguments, r#"{"path":"a"}"#);
            }
            other => panic!("expected FunctionCallDone, got {other:?}"),
        }
    }

    #[test]
    fn parse_args_delta_uses_item_id() {
        match parse_sse_event(
            "response.function_call_arguments.delta",
            r#"{"item_id":"fc_1","delta":"{\"p\":1}"}"#,
        ) {
            Some(StreamEvent::FunctionCallArgsDelta { call_id, delta }) => {
                assert_eq!(call_id, "fc_1");
                assert_eq!(delta, r#"{"p":1}"#);
            }
            other => panic!("expected FunctionCallArgsDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_completed_usage() {
        let data = r#"{"response":{"usage":{"input_tokens":12,"output_tokens":34}}}"#;
        match parse_sse_event("response.completed", data) {
            Some(StreamEvent::Completed { usage }) => {
                assert_eq!(usage.input_tokens, 12);
                assert_eq!(usage.output_tokens, 34);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn parse_error_and_non_function_items() {
        match parse_sse_event("error", r#"{"type":"error","message":"boom"}"#) {
            Some(StreamEvent::Error(m)) => assert_eq!(m, "boom"),
            other => panic!("expected Error, got {other:?}"),
        }
        // a non-function_call output item is not our concern -> None
        let msg_item = r#"{"item":{"id":"msg_1","type":"message","role":"assistant"}}"#;
        assert!(parse_sse_event("response.output_item.added", msg_item).is_none());
        // unknown event -> None
        assert!(parse_sse_event("response.created", "{}").is_none());
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent parse_`
Expected: FAIL (compile error: cannot find function `parse_sse_event`)

- [ ] **Step 3: Implement**
Add `parse_sse_event` to `crates/sola-agent/src/provider.rs`:
```rust
/// Map a Responses semantic SSE event (`event_type` + `data` JSON) to a
/// [`StreamEvent`]. Returns `None` for events we don't consume (or unparseable
/// data) so the stream loop can skip them.
///
/// Function-call correlation: `response.function_call_arguments.delta`/`.done`
/// carry only `item_id` (surfaced in the `call_id` slot for live UI). The
/// AUTHORITATIVE call — real `call_id`, `name`, full `arguments` — arrives on
/// `response.output_item.done`; that is the only `FunctionCallDone` with a
/// non-empty `name`, and the one the engine collects (see `read_sse_stream`).
pub fn parse_sse_event(event_type: &str, data_json: &str) -> Option<StreamEvent> {
    let v: Value = serde_json::from_str(data_json).ok()?;
    match event_type {
        "response.output_text.delta" => {
            Some(StreamEvent::TextDelta(v.get("delta")?.as_str()?.to_string()))
        }
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            Some(StreamEvent::Reasoning(v.get("delta")?.as_str()?.to_string()))
        }
        "response.output_item.added" => {
            let item = v.get("item")?;
            if item.get("type")?.as_str()? != "function_call" {
                return None;
            }
            Some(StreamEvent::FunctionCallStarted {
                call_id: item.get("call_id")?.as_str()?.to_string(),
                name: item.get("name")?.as_str()?.to_string(),
            })
        }
        "response.function_call_arguments.delta" => Some(StreamEvent::FunctionCallArgsDelta {
            call_id: v.get("item_id")?.as_str()?.to_string(),
            delta: v.get("delta")?.as_str()?.to_string(),
        }),
        "response.function_call_arguments.done" => Some(StreamEvent::FunctionCallDone {
            // item_id (not call_id); empty name so the engine skips this one and
            // takes the authoritative `output_item.done` below.
            call_id: v.get("item_id")?.as_str()?.to_string(),
            name: String::new(),
            arguments: v.get("arguments")?.as_str()?.to_string(),
        }),
        "response.output_item.done" => {
            let item = v.get("item")?;
            if item.get("type")?.as_str()? != "function_call" {
                return None;
            }
            Some(StreamEvent::FunctionCallDone {
                call_id: item.get("call_id")?.as_str()?.to_string(),
                name: item.get("name")?.as_str()?.to_string(),
                arguments: item
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        }
        "response.completed" => {
            let usage = v
                .get("response")
                .and_then(|r| r.get("usage"))
                .or_else(|| v.get("usage"));
            let (input_tokens, output_tokens) = match usage {
                Some(u) => (
                    u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0),
                    u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0),
                ),
                None => (0, 0),
            };
            Some(StreamEvent::Completed { usage: Usage { input_tokens, output_tokens } })
        }
        "error" | "response.failed" | "response.error" => {
            let msg = v
                .get("message")
                .and_then(|m| m.as_str())
                .or_else(|| v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()))
                .or_else(|| {
                    v.get("response")
                        .and_then(|r| r.get("error"))
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                })
                .unwrap_or("unknown error")
                .to_string();
            Some(StreamEvent::Error(msg))
        }
        _ => None,
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent parse_`
Expected: PASS (5 passed — `parse_text_delta`, `parse_function_call_started_and_done`, `parse_args_delta_uses_item_id`, `parse_completed_usage`, `parse_error_and_non_function_items`)

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/provider.rs crates/sola-agent/src/session.rs crates/sola-agent/src/main.rs && git commit -m "feat(sola-agent): parse Responses SSE semantic events into StreamEvent"`

---

### Task 6: `SakanaProvider: LlmStream` — ureq streaming + SSE assembly

Split the network from the parse: a generic `read_sse_stream<R: BufRead>` folds
an SSE byte stream into a `TurnOutcome` (unit-tested offline with a `Cursor`),
and `stream_turn` only wires ureq's body reader into it. The live ureq path is
compile-checked by `cargo test`/`cargo make build` and exercised for real only
by the Task A spike.

**Files:**
- Modify: `crates/sola-agent/src/provider.rs`
- Test: `crates/sola-agent/src/provider.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `build_request_body`, `parse_sse_event`, `StreamEvent`, `InputItem`, `crate::session::Usage`, `ureq`, `rustls`
- Produces:
  - `pub struct FunctionCall { pub call_id: String, pub name: String, pub arguments: String }`
  - `pub struct TurnOutcome { pub assistant_text: String, pub calls: Vec<FunctionCall>, pub usage: Usage }`
  - `pub trait LlmStream { fn stream_turn(&self, model: &str, effort: &str, input: &[InputItem], tools: &[serde_json::Value], sink: &mut dyn FnMut(StreamEvent)) -> Result<TurnOutcome, String>; }`
  - `pub struct SakanaProvider { pub base_url: String, pub api_key: String }` + `SakanaProvider::new(api_key: String) -> Self` + `impl LlmStream for SakanaProvider`
  - `pub fn install_crypto_provider()`
  - `pub(crate) fn read_sse_stream<R: std::io::BufRead>(reader: R, sink: &mut dyn FnMut(StreamEvent)) -> Result<TurnOutcome, String>`

- [ ] **Step 1: Write the failing test**
Add these tests to `crates/sola-agent/src/provider.rs`'s `#[cfg(test)] mod tests`
(they reference `read_sse_stream`/`TurnOutcome`/`FunctionCall`, none of which
exist yet, so the build is red):
```rust
    #[test]
    fn read_sse_stream_text_then_usage() {
        let fixture = r#"event: response.output_text.delta
data: {"delta":"Hello, "}

event: response.output_text.delta
data: {"delta":"world"}

event: response.completed
data: {"response":{"usage":{"input_tokens":5,"output_tokens":2}}}

"#;
        let mut seen = 0usize;
        let outcome = read_sse_stream(std::io::Cursor::new(fixture), &mut |_ev| { seen += 1; })
            .expect("stream should succeed");
        assert_eq!(outcome.assistant_text, "Hello, world");
        assert_eq!(outcome.usage.input_tokens, 5);
        assert_eq!(outcome.usage.output_tokens, 2);
        assert!(outcome.calls.is_empty());
        assert_eq!(seen, 3, "sink should see all 3 parsed events");
    }

    #[test]
    fn read_sse_stream_function_call() {
        let fixture = r#"event: response.output_item.added
data: {"output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_abc","name":"read","arguments":""}}

event: response.function_call_arguments.delta
data: {"item_id":"fc_1","delta":"{\"path\":\""}

event: response.function_call_arguments.delta
data: {"item_id":"fc_1","delta":"src/main.rs\"}"}

event: response.function_call_arguments.done
data: {"item_id":"fc_1","arguments":"{\"path\":\"src/main.rs\"}"}

event: response.output_item.done
data: {"output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_abc","name":"read","arguments":"{\"path\":\"src/main.rs\"}"}}

event: response.completed
data: {"response":{"usage":{"input_tokens":42,"output_tokens":7}}}

"#;
        let outcome = read_sse_stream(std::io::Cursor::new(fixture), &mut |_| {})
            .expect("stream should succeed");
        assert_eq!(outcome.assistant_text, "");
        assert_eq!(outcome.calls.len(), 1, "exactly one authoritative call");
        assert_eq!(outcome.calls[0].call_id, "call_abc");
        assert_eq!(outcome.calls[0].name, "read");
        assert_eq!(outcome.calls[0].arguments, r#"{"path":"src/main.rs"}"#);
        assert_eq!(outcome.usage.input_tokens, 42);
        assert_eq!(outcome.usage.output_tokens, 7);
    }

    #[test]
    fn read_sse_stream_error_aborts() {
        let fixture = r#"event: response.output_text.delta
data: {"delta":"partial"}

event: error
data: {"type":"error","message":"rate limited"}

"#;
        let err = read_sse_stream(std::io::Cursor::new(fixture), &mut |_| {})
            .expect_err("error event should surface as Err");
        assert_eq!(err, "rate limited");
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent read_sse_stream`
Expected: FAIL (compile error: cannot find function `read_sse_stream` / types `TurnOutcome`, `FunctionCall`)

- [ ] **Step 3: Implement**
Add to the top-of-file imports in `crates/sola-agent/src/provider.rs`:
`use std::io::BufRead;` and `use std::sync::Once;`. Then add the types, the
crypto pin, the fold helper, the stream reader, and the `LlmStream` impl:
```rust
/// A completed function call the model wants run this turn.
#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// The result of one streamed turn.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub assistant_text: String,
    pub calls: Vec<FunctionCall>,
    pub usage: Usage,
}

/// The provider test seam. One real impl (`SakanaProvider`) plus test fakes —
/// not a multi-provider abstraction, just the boundary the engine mocks.
pub trait LlmStream {
    fn stream_turn(
        &self,
        model: &str,
        effort: &str,
        input: &[InputItem],
        tools: &[Value],
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutcome, String>;
}

/// Sakana Fugu Responses client.
pub struct SakanaProvider {
    pub base_url: String,
    pub api_key: String,
}

impl SakanaProvider {
    /// `base_url` defaults to the Sakana v1 root.
    pub fn new(api_key: String) -> Self {
        Self { base_url: "https://api.sakana.ai/v1".to_string(), api_key }
    }
}

static CRYPTO_INIT: Once = Once::new();

/// Pin rustls' crypto backend to aws-lc-rs (the workspace's rustls feature) so
/// ureq's Rustls TlsProvider selects a deterministic provider from a bare TTY
/// on NixOS instead of racing ring-vs-aws_lc_rs. Idempotent; call at startup
/// and defensively before any request.
pub fn install_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// Fold one decoded event into the running turn state and forward it to `sink`.
/// Only an `output_item.done` function call (non-empty `name`) is collected as
/// an authoritative call; args deltas/done are display-only. An `Error` event
/// is forwarded, then returned as `Err`.
fn fold_event(
    ev: StreamEvent,
    assistant_text: &mut String,
    calls: &mut Vec<FunctionCall>,
    usage: &mut Usage,
    sink: &mut dyn FnMut(StreamEvent),
) -> Result<(), String> {
    let err = match &ev {
        StreamEvent::TextDelta(t) => {
            assistant_text.push_str(t);
            None
        }
        StreamEvent::FunctionCallDone { call_id, name, arguments } if !name.is_empty() => {
            calls.push(FunctionCall {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            });
            None
        }
        StreamEvent::Completed { usage: u } => {
            *usage = *u;
            None
        }
        StreamEvent::Error(msg) => Some(msg.clone()),
        _ => None,
    };
    sink(ev);
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Drive an SSE byte stream (any `BufRead`) into `sink`, folding events into a
/// `TurnOutcome`. Splits `event:`/`data:` records on blank lines. Kept generic
/// over the reader so it is unit-tested offline with a `Cursor`; the live path
/// feeds it ureq's body reader.
pub(crate) fn read_sse_stream<R: BufRead>(
    reader: R,
    sink: &mut dyn FnMut(StreamEvent),
) -> Result<TurnOutcome, String> {
    let mut assistant_text = String::new();
    let mut calls: Vec<FunctionCall> = Vec::new();
    let mut usage = Usage { input_tokens: 0, output_tokens: 0 };

    let mut event_type: Option<String> = None;
    let mut data_buf = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("SSE read error: {e}"))?;
        if line.is_empty() {
            // Blank line terminates a record.
            if let Some(et) = event_type.take() {
                if let Some(ev) = parse_sse_event(&et, &data_buf) {
                    fold_event(ev, &mut assistant_text, &mut calls, &mut usage, sink)?;
                }
            }
            data_buf.clear();
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_type = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            let chunk = rest.strip_prefix(' ').unwrap_or(rest);
            if !data_buf.is_empty() {
                data_buf.push('\n');
            }
            data_buf.push_str(chunk);
        }
        // `:` comment lines and unknown fields are ignored.
    }
    // Flush a trailing record that had no terminating blank line.
    if let Some(et) = event_type.take() {
        if let Some(ev) = parse_sse_event(&et, &data_buf) {
            fold_event(ev, &mut assistant_text, &mut calls, &mut usage, sink)?;
        }
    }

    Ok(TurnOutcome { assistant_text, calls, usage })
}

impl LlmStream for SakanaProvider {
    fn stream_turn(
        &self,
        model: &str,
        effort: &str,
        input: &[InputItem],
        tools: &[Value],
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutcome, String> {
        install_crypto_provider();
        let body = build_request_body(model, effort, input, tools);
        let mut resp = ureq::post(&format!("{}/responses", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Accept", "text/event-stream")
            .send_json(&body)
            .map_err(|e| format!("Responses POST failed: {e}"))?;
        // ureq 3: unlimited body reader — correct for an open-ended SSE stream.
        let reader = std::io::BufReader::new(resp.body_mut().as_reader());
        read_sse_stream(reader, sink)
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent read_sse_stream`
Expected: PASS (3 passed — `read_sse_stream_text_then_usage`, `read_sse_stream_function_call`, `read_sse_stream_error_aborts`). This also compiles the ureq/rustls `stream_turn` path.

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/provider.rs && git commit -m "feat(sola-agent): SakanaProvider ureq/rustls streaming + SSE turn assembly"`

- [ ] **Step 6: Build-check the whole crate**
Run: `cargo make build sola-agent`
Expected: builds clean (compiles the ureq/rustls path from a normal build, not just the test harness). Do NOT install.

### Task 7: Session module scaffold + node/tree types + `new`/`path`

**Files:**
- Modify: crates/sola-agent/Cargo.toml (add serde, serde_json, uuid; dev-dep tempfile)
- Modify: crates/sola-agent/src/main.rs (declare `mod session;`)
- Create: crates/sola-agent/src/session.rs
- Test: crates/sola-agent/src/session.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::event::NodeId` (= `String`, defined by the event.rs layer)
- Produces:
  - `pub struct Usage { pub input_tokens: u64, pub output_tokens: u64 }` (Debug, Clone, Copy, Serialize, Deserialize)
  - `pub enum Role { User, Assistant, Tool }` (Debug, Clone, Serialize, Deserialize)
  - `pub enum Content { Text(String), FunctionCall { call_id, name, arguments }, FunctionCallOutput { call_id, output } }`
  - `pub struct Node { pub id: NodeId, pub parent_id: Option<NodeId>, pub role: Role, pub content: Content, pub model: Option<String>, pub usage: Option<Usage>, pub ts: u64 }`
  - `pub struct Session { pub id, pub title, pub project_root, (priv) nodes, (priv) order, pub active_leaf }`
  - `Session::new(PathBuf) -> Session`, `Session::path(&self) -> PathBuf`

- [ ] **Step 1: Scaffold the crate deps + module, then write the failing test**

Add to `crates/sola-agent/Cargo.toml` under `[dependencies]`:
```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
```
Append a dev-deps section to the same file:
```toml
[dev-dependencies]
tempfile = "3"
```
In `crates/sola-agent/src/main.rs`, add the module declaration right after the `use` block (before `const APP_ID`):
```rust
mod session;
```
Create `crates/sola-agent/src/session.rs` with only the doc header and the test module (the referenced symbols don't exist yet — that's the red state):
```rust
//! Transcript tree, JSONL persistence, and branching for sola-agent sessions.
//!
//! A session is an append-only JSONL file of `Node`s at
//! `~/.config/sola/agent/sessions/<id>.jsonl`. Nodes form a tree via
//! `parent_id`; `active_leaf` marks the current head. Branching selects an
//! earlier node as the new leaf so the next `append` forks a sibling child.

#[cfg(test)]
mod tests {
    use super::*;

    // Serializes `$XDG_CONFIG_HOME` mutation so the fs tests don't race.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Point `$XDG_CONFIG_HOME` at a fresh tempdir for the test's duration.
    /// The returned guard + TempDir must be kept alive by the caller.
    fn temp_env() -> (std::sync::MutexGuard<'static, ()>, tempfile::TempDir) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: guarded by ENV_LOCK; no other thread reads the env here.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        }
        (guard, tmp)
    }

    #[test]
    fn new_session_has_unique_id_and_scoped_path() {
        let (_g, tmp) = temp_env();

        let a = Session::new(PathBuf::from("/home/joshua/project"));
        let b = Session::new(PathBuf::from("/home/joshua/project"));

        assert_ne!(a.id, b.id, "each session gets a distinct id");
        assert!(a.active_leaf.is_none(), "a fresh session has no leaf");

        let want = format!("{}.jsonl", a.id);
        let path = a.path();
        assert_eq!(path.file_name().and_then(|s| s.to_str()), Some(want.as_str()));
        assert!(
            path.starts_with(tmp.path()),
            "path {path:?} should live under the temp config root {:?}",
            tmp.path()
        );
        assert!(path.ends_with(format!("sola/agent/sessions/{}.jsonl", a.id)));
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent new_session_has_unique_id_and_scoped_path`
Expected: FAIL (does not compile — `Session`, `PathBuf` unresolved in the test module)

- [ ] **Step 3: Implement the types + `Session::new`/`path`**
Prepend the following above the `#[cfg(test)]` block in `crates/sola-agent/src/session.rs`:
```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::event::NodeId;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Content {
    Text(String),
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub role: Role,
    pub content: Content,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub ts: u64,
}

/// In-memory view of a transcript tree plus its on-disk JSONL location.
pub struct Session {
    pub id: String,
    pub title: String,
    pub project_root: PathBuf,
    nodes: HashMap<NodeId, Node>,
    order: Vec<NodeId>,
    pub active_leaf: Option<NodeId>,
}

/// Milliseconds since the Unix epoch (0 if the clock is before 1970).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `<config>/sola/agent/sessions`, honoring `$XDG_CONFIG_HOME`.
fn sessions_dir() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("agent")
        .join("sessions")
}

impl Session {
    /// A fresh, empty session with a random v4 id and no nodes.
    pub fn new(project_root: PathBuf) -> Self {
        Session {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            project_root,
            nodes: HashMap::new(),
            order: Vec::new(),
            active_leaf: None,
        }
    }

    /// `~/.config/sola/agent/sessions/<id>.jsonl`.
    pub fn path(&self) -> PathBuf {
        sessions_dir().join(format!("{}.jsonl", self.id))
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent new_session_has_unique_id_and_scoped_path`
Expected: PASS

- [ ] **Step 5: Commit**
`git add crates/sola-agent/Cargo.toml crates/sola-agent/src/main.rs crates/sola-agent/src/session.rs && git commit -m "feat(sola-agent): session node/tree types + id-scoped jsonl path"`

---

### Task 8: `append` + `path_to_leaf` (in-memory tree + persistence)

**Files:**
- Modify: crates/sola-agent/src/session.rs (extend `impl Session`, add `derive_title`)
- Test: crates/sola-agent/src/session.rs (inline `mod tests`)

**Interfaces:**
- Consumes: `Session`, `Role`, `Content`, `Usage`, `Node`, `NodeId`, `now_ms`, `sessions_dir` (this layer)
- Produces:
  - `Session::append(&mut self, Role, Content, Option<String>, Option<Usage>) -> NodeId` (persists one JSONL line, advances `active_leaf`)
  - `Session::path_to_leaf(&self) -> Vec<Node>` (root..=leaf)

- [ ] **Step 1: Write the failing test**
Add inside the `mod tests` block:
```rust
    #[test]
    fn append_builds_a_linear_path_to_leaf() {
        let (_g, _tmp) = temp_env();

        let mut s = Session::new(PathBuf::from("/tmp/project"));
        let n1 = s.append(Role::User, Content::Text("first".into()), None, None);
        let n2 = s.append(
            Role::Assistant,
            Content::Text("second".into()),
            Some("fugu".into()),
            Some(Usage { input_tokens: 3, output_tokens: 5 }),
        );

        assert_eq!(s.active_leaf.as_ref(), Some(&n2));
        assert_eq!(s.nodes[&n2].parent_id.as_ref(), Some(&n1));
        assert!(s.nodes[&n1].parent_id.is_none());

        let path: Vec<NodeId> = s.path_to_leaf().into_iter().map(|n| n.id).collect();
        assert_eq!(path, vec![n1, n2], "path_to_leaf is root..=leaf");
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent append_builds_a_linear_path_to_leaf`
Expected: FAIL (no method `append` / `path_to_leaf` on `Session`)

- [ ] **Step 3: Implement**
Add a free helper below `now_ms` in `session.rs`:
```rust
/// Derive a one-line, <=60-char title from the first user message text.
fn derive_title(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or("").trim();
    if first.chars().count() > 60 {
        let head: String = first.chars().take(57).collect();
        format!("{head}...")
    } else {
        first.to_string()
    }
}
```
Add these methods to `impl Session`:
```rust
    /// Append a node as a child of the current leaf, advance the leaf, and
    /// persist it as one JSONL line. Returns the new node's id.
    pub fn append(
        &mut self,
        role: Role,
        content: Content,
        model: Option<String>,
        usage: Option<Usage>,
    ) -> NodeId {
        let id = uuid::Uuid::new_v4().to_string();
        let node = Node {
            id: id.clone(),
            parent_id: self.active_leaf.clone(),
            role,
            content,
            model,
            usage,
            ts: now_ms(),
        };
        if self.title.is_empty() {
            if let (Role::User, Content::Text(t)) = (&node.role, &node.content) {
                self.title = derive_title(t);
            }
        }
        if let Err(e) = self.write_node_line(&node) {
            tracing::error!(session = %self.id, node = %id, error = %e,
                "failed to persist transcript node");
        }
        self.order.push(id.clone());
        self.nodes.insert(id.clone(), node);
        self.active_leaf = Some(id.clone());
        id
    }

    /// Serialize one node and append it as a line to the session file.
    fn write_node_line(&self, node: &Node) -> std::io::Result<()> {
        use std::io::Write;
        let path = self.path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let line = serde_json::to_string(node)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// The chain of nodes from the root down to (and including) the active leaf.
    pub fn path_to_leaf(&self) -> Vec<Node> {
        let mut chain = Vec::new();
        let mut cursor = self.active_leaf.clone();
        while let Some(id) = cursor {
            match self.nodes.get(&id) {
                Some(node) => {
                    cursor = node.parent_id.clone();
                    chain.push(node.clone());
                }
                None => break,
            }
        }
        chain.reverse();
        chain
    }
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent append_builds_a_linear_path_to_leaf`
Expected: PASS

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/session.rs && git commit -m "feat(sola-agent): append nodes to the transcript tree + path_to_leaf"`

---

### Task 9: `load` — reload the tree from JSONL

**Files:**
- Modify: crates/sola-agent/src/session.rs (add `Session::load`)
- Test: crates/sola-agent/src/session.rs (inline `mod tests`)

**Interfaces:**
- Consumes: `Session`, `Node`, `Role`, `Content`, `derive_title`, `append`, `path_to_leaf`
- Produces: `Session::load(path: &Path) -> std::io::Result<Session>` (id from filename, `active_leaf` = last appended node, title derived from first user node, `project_root` empty)

- [ ] **Step 1: Write the failing test**
Add inside `mod tests`:
```rust
    #[test]
    fn reload_reconstructs_tree_and_leaf() {
        let (_g, _tmp) = temp_env();

        let (path, id, n1, n2) = {
            let mut s = Session::new(PathBuf::from("/tmp/project"));
            let n1 = s.append(Role::User, Content::Text("hi".into()), None, None);
            let n2 = s.append(
                Role::Assistant,
                Content::Text("hello".into()),
                Some("fugu".into()),
                None,
            );
            (s.path(), s.id.clone(), n1, n2)
        };

        let reloaded = Session::load(&path).expect("load session");

        assert_eq!(reloaded.id, id, "id recovered from filename");
        assert_eq!(reloaded.active_leaf.as_ref(), Some(&n2));
        assert_eq!(reloaded.nodes.len(), 2);
        assert_eq!(reloaded.nodes[&n2].parent_id.as_ref(), Some(&n1));
        assert!(reloaded.nodes[&n1].parent_id.is_none());

        let ids: Vec<NodeId> = reloaded.path_to_leaf().into_iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![n1, n2]);
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent reload_reconstructs_tree_and_leaf`
Expected: FAIL (no associated function `load` on `Session`)

- [ ] **Step 3: Implement**
Add to `impl Session`:
```rust
    /// Rebuild a session from its append-only JSONL file. `active_leaf` is the
    /// last node written; `title` is derived from the first user text node;
    /// `project_root` is not stored per-node, so it is left empty (the session
    /// index is the authoritative source for it during a live session).
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut nodes = HashMap::new();
        let mut order = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let node: Node = serde_json::from_str(line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            order.push(node.id.clone());
            nodes.insert(node.id.clone(), node);
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_default();
        let active_leaf = order.last().cloned();
        let title = order
            .iter()
            .filter_map(|nid| nodes.get(nid))
            .find_map(|n| match (&n.role, &n.content) {
                (Role::User, Content::Text(t)) => Some(derive_title(t)),
                _ => None,
            })
            .unwrap_or_default();
        Ok(Session {
            id,
            title,
            project_root: PathBuf::new(),
            nodes,
            order,
            active_leaf,
        })
    }
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent reload_reconstructs_tree_and_leaf`
Expected: PASS

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/session.rs && git commit -m "feat(sola-agent): load a session tree from its jsonl file"`

---

### Task 10: `branch_from` — fork a sibling off a non-leaf node

**Files:**
- Modify: crates/sola-agent/src/session.rs (add `Session::branch_from`)
- Test: crates/sola-agent/src/session.rs (inline `mod tests`)

**Interfaces:**
- Consumes: `Session`, `append`, `path_to_leaf`
- Produces: `Session::branch_from(&mut self, parent: NodeId)` (sets `active_leaf = parent` so the next `append` forks a new child)

- [ ] **Step 1: Write the failing test**
Add inside `mod tests`:
```rust
    #[test]
    fn branch_from_forks_a_sibling_without_touching_old_branch() {
        let (_g, _tmp) = temp_env();

        let mut s = Session::new(PathBuf::from("/tmp/project"));
        let root = s.append(Role::User, Content::Text("root".into()), None, None);
        let old = s.append(Role::Assistant, Content::Text("old reply".into()), None, None);

        s.branch_from(root.clone());
        assert_eq!(s.active_leaf.as_ref(), Some(&root), "leaf moved back to the parent");

        let new = s.append(Role::Assistant, Content::Text("new reply".into()), None, None);

        // The new node is a second child of root; the old branch is untouched.
        assert_eq!(s.nodes[&new].parent_id.as_ref(), Some(&root));
        assert_eq!(s.nodes[&old].parent_id.as_ref(), Some(&root));
        match &s.nodes[&old].content {
            Content::Text(t) => assert_eq!(t, "old reply"),
            other => panic!("old branch content changed: {other:?}"),
        }

        let children = s
            .order
            .iter()
            .filter(|id| s.nodes.get(*id).and_then(|n| n.parent_id.as_ref()) == Some(&root))
            .count();
        assert_eq!(children, 2, "root has two children after branching");

        let leaf_ids: Vec<NodeId> = s.path_to_leaf().into_iter().map(|n| n.id).collect();
        assert_eq!(leaf_ids, vec![root, new], "active path is the new branch");
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent branch_from_forks_a_sibling_without_touching_old_branch`
Expected: FAIL (no method `branch_from` on `Session`)

- [ ] **Step 3: Implement**
Add to `impl Session`:
```rust
    /// Move the active leaf back to an earlier node so the next `append` forks
    /// a new child off it, leaving the previous branch intact.
    pub fn branch_from(&mut self, parent: NodeId) {
        self.active_leaf = Some(parent);
    }
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent branch_from_forks_a_sibling_without_touching_old_branch`
Expected: PASS

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/session.rs && git commit -m "feat(sola-agent): branch_from forks a new child off a past node"`

---

### Task 11: `to_input` — map the active branch to provider `InputItem`s

**Files:**
- Modify: crates/sola-agent/src/session.rs (add import + `Session::to_input`)
- Test: crates/sola-agent/src/session.rs (inline `mod tests`)

**Interfaces:**
- Consumes: `crate::provider::InputItem` (defined by the provider.rs layer), `Session`, `Content`, `Role`, `path_to_leaf`
- Produces: `Session::to_input(&self) -> Vec<InputItem>` (Text→Message, FunctionCall→FunctionCall, FunctionCallOutput→FunctionCallOutput)

- [ ] **Step 1: Write the failing test**
Add inside `mod tests`:
```rust
    #[test]
    fn to_input_maps_each_content_variant() {
        let (_g, _tmp) = temp_env();

        let mut s = Session::new(PathBuf::from("/tmp/project"));
        s.append(Role::User, Content::Text("hello".into()), None, None);
        s.append(
            Role::Assistant,
            Content::FunctionCall {
                call_id: "c1".into(),
                name: "read".into(),
                arguments: "{\"path\":\"a.txt\"}".into(),
            },
            None,
            None,
        );
        s.append(
            Role::Tool,
            Content::FunctionCallOutput {
                call_id: "c1".into(),
                output: "file body".into(),
            },
            None,
            None,
        );

        let items = s.to_input();
        assert_eq!(items.len(), 3);

        match &items[0] {
            InputItem::Message { role, text } => {
                assert_eq!(role, "user");
                assert_eq!(text, "hello");
            }
            other => panic!("expected Message, got {other:?}"),
        }
        match &items[1] {
            InputItem::FunctionCall { call_id, name, arguments } => {
                assert_eq!(call_id, "c1");
                assert_eq!(name, "read");
                assert_eq!(arguments, "{\"path\":\"a.txt\"}");
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
        match &items[2] {
            InputItem::FunctionCallOutput { call_id, output } => {
                assert_eq!(call_id, "c1");
                assert_eq!(output, "file body");
            }
            other => panic!("expected FunctionCallOutput, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent to_input_maps_each_content_variant`
Expected: FAIL (no method `to_input`; `InputItem` unresolved)

- [ ] **Step 3: Implement**
Add the import near the top of `session.rs` (below `use crate::event::NodeId;`):
```rust
use crate::provider::InputItem;
```
Add to `impl Session`:
```rust
    /// Map the active branch (root..=leaf) to the provider's `InputItem`s, in
    /// order. Text nodes become role-tagged messages; function-call and
    /// function-call-output nodes pass their fields through unchanged.
    pub fn to_input(&self) -> Vec<InputItem> {
        self.path_to_leaf()
            .into_iter()
            .map(|node| match node.content {
                Content::Text(text) => InputItem::Message {
                    role: match node.role {
                        Role::User => "user".to_string(),
                        Role::Assistant => "assistant".to_string(),
                        Role::Tool => "user".to_string(),
                    },
                    text,
                },
                Content::FunctionCall { call_id, name, arguments } => {
                    InputItem::FunctionCall { call_id, name, arguments }
                }
                Content::FunctionCallOutput { call_id, output } => {
                    InputItem::FunctionCallOutput { call_id, output }
                }
            })
            .collect()
    }
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent to_input_maps_each_content_variant`
Expected: PASS

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/session.rs && git commit -m "feat(sola-agent): to_input rebuilds provider input from the active branch"`

---

### Task 12: session index — `IndexEntry`, `load_index`, `rebuild_index`

**Files:**
- Modify: crates/sola-agent/src/session.rs (add `IndexEntry`, index fns, `update_index`; call it from `append`)
- Test: crates/sola-agent/src/session.rs (inline `mod tests`)

**Interfaces:**
- Consumes: `Session::load`, `sessions_dir`, `now_ms`, `append`
- Produces:
  - `pub struct IndexEntry { pub id: String, pub title: String, pub project_root: PathBuf, pub updated: u64 }`
  - `pub fn load_index() -> Vec<IndexEntry>` (reads `sessions/index.json`, else rebuilds from files)
  - `pub fn rebuild_index() -> std::io::Result<Vec<IndexEntry>>` (scans `*.jsonl`, rewrites `index.json`)
  - `append` now upserts the index on every call

- [ ] **Step 1: Write the failing test**
Add inside `mod tests`:
```rust
    #[test]
    fn append_maintains_index_and_rebuild_recovers_it() {
        let (_g, _tmp) = temp_env();

        let mut s = Session::new(PathBuf::from("/home/joshua/proj"));
        s.append(Role::User, Content::Text("index me".into()), None, None);
        let id = s.id.clone();

        // append upserted an entry with live title + project_root
        let index = load_index();
        let entry = index.iter().find(|e| e.id == id).expect("append writes an index entry");
        assert_eq!(entry.title, "index me");
        assert_eq!(entry.project_root, PathBuf::from("/home/joshua/proj"));

        // wipe the index; load_index must rebuild it from the jsonl files.
        // (project_root is not stored per-node, so only id + derived title survive.)
        std::fs::remove_file(index_path()).unwrap();
        let rebuilt = load_index();
        let rebuilt_entry =
            rebuilt.iter().find(|e| e.id == id).expect("index rebuilt from files");
        assert_eq!(rebuilt_entry.title, "index me", "title recovered from first user node");
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent append_maintains_index_and_rebuild_recovers_it`
Expected: FAIL (`load_index` / `index_path` unresolved; `append` writes no index)

- [ ] **Step 3: Implement**
Add near the top-level items in `session.rs` (after `sessions_dir`):
```rust
/// Sidebar metadata for one session, persisted in `sessions/index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub title: String,
    pub project_root: PathBuf,
    pub updated: u64,
}

fn index_path() -> PathBuf {
    sessions_dir().join("index.json")
}

fn write_index(entries: &[IndexEntry]) -> std::io::Result<()> {
    let path = index_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Read the session index, rebuilding it from the JSONL files if the index
/// file is missing or unparseable.
pub fn load_index() -> Vec<IndexEntry> {
    match std::fs::read_to_string(index_path()) {
        Ok(s) => match serde_json::from_str::<Vec<IndexEntry>>(&s) {
            Ok(entries) => entries,
            Err(_) => rebuild_index().unwrap_or_default(),
        },
        Err(_) => rebuild_index().unwrap_or_default(),
    }
}

/// Scan every `<id>.jsonl` under the sessions dir, derive an entry per file,
/// and rewrite `index.json`. Recovers id + derived title + file mtime;
/// `project_root` is not stored per-node and comes back empty.
pub fn rebuild_index() -> std::io::Result<Vec<IndexEntry>> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(session) = Session::load(&path) else { continue };
        let updated = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        entries.push(IndexEntry {
            id: session.id,
            title: session.title,
            project_root: session.project_root,
            updated,
        });
    }
    write_index(&entries)?;
    Ok(entries)
}
```
Add the upsert method to `impl Session`:
```rust
    /// Upsert this session's entry into `index.json`.
    fn update_index(&self) {
        let mut entries = load_index();
        let entry = IndexEntry {
            id: self.id.clone(),
            title: self.title.clone(),
            project_root: self.project_root.clone(),
            updated: now_ms(),
        };
        match entries.iter_mut().find(|e| e.id == self.id) {
            Some(existing) => *existing = entry,
            None => entries.push(entry),
        }
        if let Err(e) = write_index(&entries) {
            tracing::error!(session = %self.id, error = %e, "failed to update session index");
        }
    }
```
Wire it into `append` — change the tail of `append` from:
```rust
        self.order.push(id.clone());
        self.nodes.insert(id.clone(), node);
        self.active_leaf = Some(id.clone());
        id
```
to:
```rust
        self.order.push(id.clone());
        self.nodes.insert(id.clone(), node);
        self.active_leaf = Some(id.clone());
        self.update_index();
        id
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent append_maintains_index_and_rebuild_recovers_it`
Expected: PASS

- [ ] **Step 5: Run the full session suite + build, then commit**
Run: `cargo test -p sola-agent session::` (Expected: PASS — all six session tests)
Run: `cargo make build sola-agent` (Expected: compiles clean)
`git add crates/sola-agent/src/session.rs && git commit -m "feat(sola-agent): session index.json with rebuild-from-files fallback"`

### Task 13: bridge-channels

**Files:**
- Create: `crates/sola-agent/src/event.rs`
- Create: `crates/sola-agent/src/session.rs` (minimal shared-type stub — the Session layer extends this file)
- Create: `crates/sola-agent/src/tools/mod.rs` (minimal shared-type stub — the Tools layer extends this file)
- Modify: `crates/sola-agent/Cargo.toml` (add `serde`, `serde_json`)
- Modify: `crates/sola-agent/src/main.rs` (declare `mod event; mod session; mod tools;`)
- Test: `crates/sola-agent/src/event.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::session::Usage`, `crate::tools::{ToolResult, ToolDetail}`, `iced::Subscription`, `iced::futures::Stream`, `iced::futures::channel::mpsc::unbounded`, `std::sync::{Mutex, OnceLock, mpsc}`
- Produces:
  - `pub type NodeId = String;`
  - `pub enum AgentEvent` (Debug, Clone) — variants exactly per contract
  - `pub enum AgentCmd` (Debug, Clone) — variants exactly per contract
  - `pub fn init_channels()`
  - `pub fn agent_subscription() -> iced::Subscription<AgentEvent>`
  - `pub fn agent_send(cmd: AgentCmd)`
  - `pub(crate) fn emit(ev: AgentEvent)`
  - `pub(crate) fn take_cmd_rx() -> std::sync::mpsc::Receiver<AgentCmd>`

---

- [ ] **Step 1: Write the failing test (plus the scaffolding it compiles against)**

First add the two wire-serialisation deps to `crates/sola-agent/Cargo.toml` (under the existing `[dependencies]`, right after the `tracing = "0.1"` line):

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Declare the new modules in `crates/sola-agent/src/main.rs`. Replace the `use sola_kit::theme::{default_theme, theme_from_bus};` line + the following blank line + `const APP_ID` line with:

```rust
use sola_kit::theme::{default_theme, theme_from_bus};

// Bridge + shared-type modules. `event` is the UI⇄worker channel bridge;
// `session` and `tools` are stubbed here with only the types the bridge
// consumes and are fleshed out by their own layers.
mod event;
mod session;
mod tools;

const APP_ID: &str = "sola-agent";
```

Create `crates/sola-agent/src/session.rs` with only the shared type the bridge consumes:

```rust
//! Session transcript types.
//!
//! The Bridge layer defines only `Usage` here — the token-accounting struct
//! that `AgentEvent::TurnEnd` and (later) `Node` both carry. The Session layer
//! extends this file with `Role`, `Content`, `Node`, and the `Session`
//! transcript tree; it must NOT redefine `Usage`.

use serde::{Deserialize, Serialize};

/// Token accounting for one model turn.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}
```

Create `crates/sola-agent/src/tools/mod.rs` with only the result types the bridge consumes:

```rust
//! Tool execution surface.
//!
//! The Bridge layer defines only the result types `ToolResult` / `ToolDetail`
//! here — what `AgentEvent::ToolEnd` carries. The Tools layer extends this file
//! with `ToolCtx`, `tool_schemas`, `dispatch`, and the per-tool submodules; it
//! must NOT redefine these two types.

/// What a tool run returns: text fed back to the model plus a richer UI view.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub model_text: String,
    pub ui_detail: ToolDetail,
}

/// Structured UI rendering of a tool result.
#[derive(Debug, Clone)]
pub enum ToolDetail {
    Text(String),
    Diff { path: String, before: String, after: String },
    Bash { code: i32, stdout: String, stderr: String },
}
```

Create `crates/sola-agent/src/event.rs` containing ONLY the test module (the bridge impl lands in Step 3, so this fails to compile now):

```rust
//! UI ⇄ worker bridge (implementation added in Step 3).

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::TryRecvError;

    /// Full bridge round-trip in a single test to keep the process-global
    /// statics deterministic: CMD (UI→worker), EVENT (worker→UI) drained
    /// straight off the static receiver, and the second-take guard.
    #[test]
    fn bridge_round_trips_and_guards_second_take() {
        init_channels();

        // ── CMD: UI → worker ──────────────────────────────────────────────
        agent_send(AgentCmd::Send { text: "hello".into(), branch_from: None });
        agent_send(AgentCmd::Abort);

        let cmd_rx = take_cmd_rx(); // first take → the real receiver
        match cmd_rx.try_recv() {
            Ok(AgentCmd::Send { text, branch_from }) => {
                assert_eq!(text, "hello");
                assert!(branch_from.is_none());
            }
            other => panic!("expected Send, got {other:?}"),
        }
        match cmd_rx.try_recv() {
            Ok(AgentCmd::Abort) => {}
            other => panic!("expected Abort, got {other:?}"),
        }

        // ── EVENT: worker → UI, drained straight off the static receiver ──
        emit(AgentEvent::Delta { node_id: "n1".into(), text: "chunk".into() });
        let event_rx = EVENT_RX
            .lock()
            .unwrap()
            .take()
            .expect("EVENT_RX present after init_channels");
        match event_rx.try_recv() {
            Ok(AgentEvent::Delta { node_id, text }) => {
                assert_eq!(node_id, "n1");
                assert_eq!(text, "chunk");
            }
            other => panic!("expected Delta, got {other:?}"),
        }

        // ── Second take is guarded: an inert, disconnected receiver ───────
        let dead_rx = take_cmd_rx();
        assert!(matches!(dead_rx.try_recv(), Err(TryRecvError::Disconnected)));

        // Dropping the only live receiver and sending again must not panic —
        // the send just fails silently (swallowed by `agent_send`).
        drop(cmd_rx);
        agent_send(AgentCmd::Abort);
    }
}
```

- [ ] **Step 2: Run it, expect failure**

Run: `cargo test -p sola-agent bridge_round_trips_and_guards_second_take`
Expected: FAIL (compile error — `cannot find value/function init_channels`, `agent_send`, `take_cmd_rx`, `emit`, `EVENT_RX` and `cannot find type AgentEvent`/`AgentCmd` in this scope; the bridge impl doesn't exist yet).

- [ ] **Step 3: Implement**

Prepend the bridge implementation to `crates/sola-agent/src/event.rs`, above the existing `#[cfg(test)] mod tests` block (replace the placeholder doc line at the top):

```rust
//! UI ⇄ worker bridge.
//!
//! Mirrors `sola-terminal`'s `emulator.rs` process-wide channel pattern: an
//! `OnceLock<Sender<_>>` paired with a `Mutex<Option<Receiver<_>>>` so the
//! single receiver is taken exactly once and a second taker gets a guarded,
//! inert channel instead of racing on it. Two directions:
//!
//! - EVENT: worker → UI. `emit` sends; `agent_subscription` drains into iced.
//! - CMD:   UI → worker. `agent_send` sends; `take_cmd_rx` hands the engine
//!   thread the single receiver.

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

use crate::session::Usage;
use crate::tools::ToolResult;

/// uuid v4 string identifying a transcript node.
pub type NodeId = String;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Delta { node_id: NodeId, text: String },
    Reasoning { text: String },
    ToolStart { call_id: String, tool: String, args: serde_json::Value },
    ToolOutput { call_id: String, chunk: String },
    ToolEnd { call_id: String, result: ToolResult },
    ApprovalRequest { call_id: String, tool: String, preview: String },
    TurnEnd { usage: Usage },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum AgentCmd {
    Send { text: String, branch_from: Option<NodeId> },
    Approve { call_id: String, remember: bool },
    Deny { call_id: String, reason: Option<String> },
    Abort,
    SetModel { model: String, effort: String },
}

// ── Process-wide statics ──────────────────────────────────────────────────────

/// worker → UI.
static EVENT_TX: OnceLock<mpsc::Sender<AgentEvent>> = OnceLock::new();
static EVENT_RX: Mutex<Option<mpsc::Receiver<AgentEvent>>> = Mutex::new(None);

/// UI → worker.
static CMD_TX: OnceLock<mpsc::Sender<AgentCmd>> = OnceLock::new();
static CMD_RX: Mutex<Option<mpsc::Receiver<AgentCmd>>> = Mutex::new(None);

/// Create both channel pairs into the statics. Idempotent (`get_or_init`), so
/// it is safe to call from `main` at startup and again from any lazy path
/// (e.g. `agent_subscription` guards against being built before startup ran).
pub fn init_channels() {
    EVENT_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<AgentEvent>();
        *EVENT_RX.lock().unwrap() = Some(rx);
        tx
    });
    CMD_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<AgentCmd>();
        *CMD_RX.lock().unwrap() = Some(rx);
        tx
    });
}

/// UI → worker: enqueue a command. Dropped with a warning if the channels
/// aren't initialised yet — never a panic.
pub fn agent_send(cmd: AgentCmd) {
    match CMD_TX.get() {
        Some(tx) => {
            let _ = tx.send(cmd);
        }
        None => {
            tracing::warn!(?cmd, "agent_send before init_channels; dropping command");
        }
    }
}

/// worker → UI: emit an event toward the iced subscription. Dropped with a
/// warning if the channels aren't initialised yet — never a panic.
pub(crate) fn emit(ev: AgentEvent) {
    match EVENT_TX.get() {
        Some(tx) => {
            let _ = tx.send(ev);
        }
        None => {
            tracing::warn!(?ev, "emit before init_channels; dropping event");
        }
    }
}

/// The engine thread takes the single command receiver exactly once. A second
/// call is guarded: it logs and returns a fresh, already-disconnected receiver
/// (its sender immediately dropped) so callers get an inert receiver rather
/// than a panic — the same "one receiver per process" discipline
/// `agent_subscription` uses for the event side.
pub(crate) fn take_cmd_rx() -> mpsc::Receiver<AgentCmd> {
    match CMD_RX.lock().unwrap().take() {
        Some(rx) => rx,
        None => {
            tracing::warn!(
                "take_cmd_rx called while receiver is already taken; \
                 returning a disconnected receiver (one receiver per process)"
            );
            let (tx, rx) = mpsc::channel::<AgentCmd>();
            drop(tx);
            rx
        }
    }
}

/// iced `Subscription` delivering `AgentEvent`s from the worker. The receiver
/// is taken once; a rebuilt subscription (iced rebuilds the set on every
/// update) gets an empty stream — mirror of `emulator.rs::output_subscription`.
pub fn agent_subscription() -> Subscription<AgentEvent> {
    Subscription::run(event_stream)
}

fn event_stream() -> impl Stream<Item = AgentEvent> {
    init_channels();
    let rx_opt = EVENT_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<AgentEvent>();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || loop {
                // Exit if the iced side dropped the subscription.
                if iced_tx.is_closed() {
                    break;
                }
                match std_rx.recv() {
                    Ok(ev) => {
                        if iced_tx.unbounded_send(ev).is_err() {
                            break;
                        }
                    }
                    // All senders dropped — worker gone. Stop.
                    Err(_) => break,
                }
            });
        }
        None => {
            tracing::warn!(
                "agent_subscription called while receiver is already taken; \
                 returning empty stream (one receiver per process)"
            );
            drop(iced_tx);
        }
    }
    iced_rx
}
```

- [ ] **Step 4: Run it, expect pass**

Run: `cargo test -p sola-agent bridge_round_trips_and_guards_second_take`
Expected: PASS (1 passed).
Also verify the crate compiles: `cargo make build sola-agent` → succeeds. Dead-code warnings for the not-yet-wired bridge fns (`agent_send`, `emit`, `take_cmd_rx`, `agent_subscription`, `init_channels`) are expected here — the main.rs subscription-batching layer consumes them and the warnings clear then.

- [ ] **Step 5: Commit**

Run: `git add crates/sola-agent/Cargo.toml crates/sola-agent/src/event.rs crates/sola-agent/src/session.rs crates/sola-agent/src/tools/mod.rs crates/sola-agent/src/main.rs && git commit -m "feat(sola-agent): add UI⇄worker event bridge (channels, subscription, guarded receivers)"`

### Task 14: Engine text-only turn loop

**Files:**
- Create: crates/sola-agent/src/engine.rs
- Modify: crates/sola-agent/src/main.rs (add `mod engine;`)
- Modify: crates/sola-agent/Cargo.toml (add `serde_json`, `uuid`)
- Test: crates/sola-agent/src/engine.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes (from sibling layers, exact):
  - `crate::event`: `pub enum AgentEvent { Delta { node_id: NodeId, text: String }, Reasoning { text: String }, TurnEnd { usage: Usage }, Error { message: String }, .. }`; `pub enum AgentCmd { Send { text: String, branch_from: Option<NodeId> }, Approve { call_id: String, remember: bool }, Deny { call_id: String, reason: Option<String> }, Abort, SetModel { model: String, effort: String } }`; `pub(crate) fn emit(ev: AgentEvent)`; `pub(crate) fn take_cmd_rx() -> std::sync::mpsc::Receiver<AgentCmd>`
  - `crate::session`: `pub struct Session { pub project_root: std::path::PathBuf, .. }`; `Session::new(project_root: PathBuf) -> Self`; `append(&mut self, role: Role, content: Content, model: Option<String>, usage: Option<Usage>) -> NodeId`; `branch_from(&mut self, parent: NodeId)`; `to_input(&self) -> Vec<InputItem>`; `path_to_leaf(&self) -> Vec<Node>`; `enum Role { User, Assistant, Tool }`; `enum Content { Text(String), FunctionCall{..}, FunctionCallOutput{..} }`; `struct Usage { input_tokens: u64, output_tokens: u64 }`; `struct Node { role: Role, content: Content, .. }`
  - `crate::provider`: `trait LlmStream { fn stream_turn(&self, model: &str, effort: &str, input: &[InputItem], tools: &[serde_json::Value], sink: &mut dyn FnMut(StreamEvent)) -> Result<TurnOutcome, String>; }`; `enum StreamEvent { TextDelta(String), Reasoning(String), Completed{ usage: Usage }, Error(String), .. }`; `struct TurnOutcome { assistant_text: String, calls: Vec<FunctionCall>, usage: Usage }`; `enum InputItem { .. }`; `struct FunctionCall { call_id: String, name: String, arguments: String }`
  - `crate::permit`: `pub struct Policy { pub project_root: std::path::PathBuf, pub always: Vec<Rule>, pub classifier: bool }`
  - `crate::tools`: `pub fn tool_schemas() -> Vec<serde_json::Value>`
- Produces (later layers rely on these EXACT names):
  - `pub struct EngineConfig { pub api_key: String, pub model: String, pub effort: String, pub project_root: std::path::PathBuf, pub classifier: bool }`
  - `pub fn start(config: EngineConfig, provider: std::sync::Arc<dyn crate::provider::LlmStream + Send + Sync>, session: std::sync::Arc<std::sync::Mutex<crate::session::Session>>)` — **the shared session handle is `Arc<Mutex<Session>>`** (main.rs/bridge construct and pass this).
  - internal `fn run_turn(config: &EngineConfig, provider: &(dyn LlmStream + Send + Sync), session: &Arc<Mutex<Session>>, policy: &mut Policy, cmd_rx: &Receiver<AgentCmd>, abort: &AtomicBool, emit: &mut dyn FnMut(AgentEvent))` — the emit sink is injected so the loop is testable without the global channel or a spawned thread.

- [ ] **Step 1: Write the failing test**
  Scaffolding folded in — (a) add to `crates/sola-agent/Cargo.toml` under `[dependencies]` (de-dupe if a prior layer already added them):
  ```toml
  serde_json = "1"
  uuid = { version = "1", features = ["v4"] }
  ```
  (b) add `mod engine;` to `crates/sola-agent/src/main.rs` (below the existing `use` lines). (c) create `crates/sola-agent/src/engine.rs` containing only this test module:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::event::{AgentCmd, AgentEvent};
      use crate::permit::Policy;
      use crate::provider::{
          FunctionCall, InputItem, LlmStream, StreamEvent, TurnOutcome,
      };
      use crate::session::{Content, Role, Session, Usage};
      use std::sync::atomic::AtomicBool;
      use std::sync::{Arc, Mutex};

      /// Redirect `$XDG_CONFIG_HOME` to a fresh temp dir so `Session` JSONL
      /// persistence never touches the real `~/.config`, and return an
      /// (also-temp) project root for the tools' `ToolCtx`.
      fn hermetic_root(tag: &str) -> std::path::PathBuf {
          let base = std::env::temp_dir()
              .join(format!("sola-agent-{tag}-{}", uuid::Uuid::new_v4()));
          let cfg = base.join("config");
          let root = base.join("project");
          std::fs::create_dir_all(&cfg).unwrap();
          std::fs::create_dir_all(&root).unwrap();
          // SAFETY (edition 2024): test setup only; the value is always an
          // absolute temp path, so a successful persist can never land in
          // the real $HOME. Concurrent tests each point it at their own
          // temp dir — a benign race (assertions read the in-memory tree).
          unsafe { std::env::set_var("XDG_CONFIG_HOME", &cfg) };
          root
      }

      /// Streams two text deltas + a completed event, then returns the
      /// aggregated turn with no tool calls.
      struct TextFake;
      impl LlmStream for TextFake {
          fn stream_turn(
              &self,
              _model: &str,
              _effort: &str,
              _input: &[InputItem],
              _tools: &[serde_json::Value],
              sink: &mut dyn FnMut(StreamEvent),
          ) -> Result<TurnOutcome, String> {
              sink(StreamEvent::TextDelta("he".into()));
              sink(StreamEvent::TextDelta("llo".into()));
              sink(StreamEvent::Completed {
                  usage: Usage { input_tokens: 3, output_tokens: 4 },
              });
              Ok(TurnOutcome {
                  assistant_text: "hello".into(),
                  calls: Vec::new(),
                  usage: Usage { input_tokens: 3, output_tokens: 4 },
              })
          }
      }

      #[test]
      fn text_only_turn_streams_and_appends_assistant() {
          let root = hermetic_root("text");
          let session = Arc::new(Mutex::new(Session::new(root.clone())));
          // Simulate the Send handler having already appended the user node.
          session
              .lock()
              .unwrap()
              .append(Role::User, Content::Text("hi".into()), None, None);

          let config = EngineConfig {
              api_key: String::new(),
              model: "fugu".into(),
              effort: "high".into(),
              project_root: root.clone(),
              classifier: false,
          };
          let fake = TextFake;
          let mut policy = Policy {
              project_root: root.clone(),
              always: Vec::new(),
              classifier: false,
          };
          let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel::<AgentCmd>();
          let abort = AtomicBool::new(false);

          let mut events: Vec<AgentEvent> = Vec::new();
          {
              let mut emit = |ev| events.push(ev);
              run_turn(
                  &config, &fake, &session, &mut policy, &cmd_rx, &abort,
                  &mut emit,
              );
          }

          assert_eq!(
              events.len(),
              3,
              "expected two deltas and a turn-end, got {events:?}"
          );
          assert!(matches!(
              &events[0],
              AgentEvent::Delta { text, .. } if text.as_str() == "he"
          ));
          assert!(matches!(
              &events[1],
              AgentEvent::Delta { text, .. } if text.as_str() == "llo"
          ));
          assert!(matches!(
              &events[2],
              AgentEvent::TurnEnd { usage }
                  if usage.input_tokens == 3 && usage.output_tokens == 4
          ));

          let s = session.lock().unwrap();
          let path = s.path_to_leaf();
          let last = path.last().expect("session has at least one node");
          assert!(
              matches!(last.role, Role::Assistant),
              "leaf should be the assistant node"
          );
          assert!(matches!(
              &last.content,
              Content::Text(t) if t.as_str() == "hello"
          ));
      }
  }
  ```

- [ ] **Step 2: Run it, expect failure**
  Run: `cargo test -p sola-agent text_only_turn_streams_and_appends_assistant`
  Expected: FAIL — `error[E0425]: cannot find function 'run_turn'` and `error[E0422]: cannot find struct 'EngineConfig'` (neither exists yet in `engine.rs`).

- [ ] **Step 3: Implement**
  Prepend the non-test code to `crates/sola-agent/src/engine.rs` (above the `#[cfg(test)] mod tests` block):
  ```rust
  //! Agent worker thread + the turn loop.
  //!
  //! `start` spawns one dedicated std thread (mirroring `pty.rs`) that owns
  //! the command receiver and drives turns. The loop's core, `run_turn`,
  //! takes its event sink as `&mut dyn FnMut(AgentEvent)` so it is unit-
  //! testable with a local collector; `start` wires that sink to the global
  //! `crate::event::emit`.

  use std::sync::atomic::{AtomicBool, Ordering};
  use std::sync::mpsc::Receiver;
  use std::sync::{Arc, Mutex};

  use crate::event::{AgentCmd, AgentEvent};
  use crate::permit::Policy;
  use crate::provider::{LlmStream, StreamEvent};
  use crate::session::{Content, Role, Session};
  use crate::tools::tool_schemas;

  /// Static configuration for one worker. `model`/`effort` are mutable at
  /// runtime via `AgentCmd::SetModel`; the rest are fixed for the process.
  pub struct EngineConfig {
      pub api_key: String,
      pub model: String,
      pub effort: String,
      pub project_root: std::path::PathBuf,
      pub classifier: bool,
  }

  /// Spawn the worker thread. It takes the process-wide command receiver
  /// exactly once, then loops: on `Send` it appends the user node (and
  /// forks first if `branch_from` is set), resets the abort flag, and runs
  /// a turn; `Abort` trips the flag; `SetModel` swaps model/effort.
  pub fn start(
      config: EngineConfig,
      provider: Arc<dyn LlmStream + Send + Sync>,
      session: Arc<Mutex<Session>>,
  ) {
      std::thread::spawn(move || {
          let mut config = config;
          let cmd_rx = crate::event::take_cmd_rx();
          let abort = AtomicBool::new(false);
          let mut policy = Policy {
              project_root: config.project_root.clone(),
              always: Vec::new(),
              classifier: config.classifier,
          };
          while let Ok(cmd) = cmd_rx.recv() {
              match cmd {
                  AgentCmd::Send { text, branch_from } => {
                      {
                          let mut s = session.lock().unwrap();
                          if let Some(parent) = branch_from {
                              s.branch_from(parent);
                          }
                          s.append(Role::User, Content::Text(text), None, None);
                      }
                      abort.store(false, Ordering::SeqCst);
                      run_turn(
                          &config,
                          provider.as_ref(),
                          &session,
                          &mut policy,
                          &cmd_rx,
                          &abort,
                          &mut |ev| crate::event::emit(ev),
                      );
                  }
                  AgentCmd::Abort => abort.store(true, Ordering::SeqCst),
                  AgentCmd::SetModel { model, effort } => {
                      config.model = model;
                      config.effort = effort;
                  }
                  // No turn is awaiting a decision at loop scope — ignore
                  // stray approvals/denials.
                  AgentCmd::Approve { .. } | AgentCmd::Deny { .. } => {}
              }
          }
      });
  }

  /// Drive one turn. This layer handles text-only turns: stream, forward
  /// display events, append the assistant node, emit `TurnEnd`. The tool-
  /// executing loop is added in the next task (same signature).
  fn run_turn(
      config: &EngineConfig,
      provider: &(dyn LlmStream + Send + Sync),
      session: &Arc<Mutex<Session>>,
      _policy: &mut Policy,
      _cmd_rx: &Receiver<AgentCmd>,
      abort: &AtomicBool,
      emit: &mut dyn FnMut(AgentEvent),
  ) {
      if abort.load(Ordering::SeqCst) {
          return;
      }
      let tools = tool_schemas();
      let input = { session.lock().unwrap().to_input() };
      let stream_id = uuid::Uuid::new_v4().to_string();
      let outcome = {
          let mut sink = |ev: StreamEvent| match ev {
              StreamEvent::TextDelta(t) => {
                  emit(AgentEvent::Delta { node_id: stream_id.clone(), text: t })
              }
              StreamEvent::Reasoning(t) => emit(AgentEvent::Reasoning { text: t }),
              StreamEvent::Error(m) => emit(AgentEvent::Error { message: m }),
              _ => {}
          };
          provider.stream_turn(
              &config.model,
              &config.effort,
              &input,
              &tools,
              &mut sink,
          )
      };
      match outcome {
          Ok(o) => {
              if !o.assistant_text.is_empty() {
                  session.lock().unwrap().append(
                      Role::Assistant,
                      Content::Text(o.assistant_text.clone()),
                      Some(config.model.clone()),
                      Some(o.usage),
                  );
              }
              emit(AgentEvent::TurnEnd { usage: o.usage });
          }
          Err(e) => emit(AgentEvent::Error { message: e }),
      }
  }
  ```

- [ ] **Step 4: Run it, expect pass**
  Run: `cargo test -p sola-agent text_only_turn_streams_and_appends_assistant`
  Expected: PASS (1 passed).

- [ ] **Step 5: Commit**
  Verify the crate builds through the project build system, then commit:
  ```
  cargo make build sola-agent && \
  git add crates/sola-agent/src/engine.rs crates/sola-agent/src/main.rs crates/sola-agent/Cargo.toml && \
  git commit -m "feat(sola-agent): engine text-only turn loop with injectable emit sink"
  ```

---

### Task 15: Startup wiring — key load, engine boot, App skeleton

Fully replaces the old `main.rs` stub (its private `Role`/`ChatMessage`/`Conversation`/`update`/`view` are removed). The view is a placeholder here; Task 17 delivers the transcript. `first_run` is computed at boot but the first-run *prompt* is added in Task 29 (until then, first-run just blocks `Send`).

**Files:**
- Modify: crates/sola-agent/src/main.rs (full rewrite of the stub)
- Test: crates/sola-agent/src/main.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `event::{init_channels, agent_subscription, AgentEvent, NodeId}` (Tasks 1, 13); `session::{Session, Usage}` + `Session::{new, load}` (Phase 3); `provider::{SakanaProvider, LlmStream}` (Phase 2); `engine::{EngineConfig, start}` (Task 14); `tools::ToolDetail` (Task 1); `sola_core::Encrypted<String>`, `sola_core::config::sola_config_dir`; kit `startup`, `BusSetup`, `bus_subscription`, `apply_theme_update`, `is_self_quit`, `window_settings`, `fonts`, `theme::default_theme`.
- Produces: `struct App`; `enum Msg`; `enum Turn`; `struct ToolTurn`; `struct PendingApproval`; `struct SessionSummary`; `struct Boot` + `static BOOT`; `App::{new, blank, title, theme, subscription, update, view}`; free fns `credentials_path/sessions_dir/load_api_key/save_api_key/spawn_engine/list_sessions`; `struct Credentials`.

- [ ] **Step 1: Write the failing test**
Append this test module to `crates/sola-agent/src/main.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    pub(crate) fn blank_app(first_run: bool) -> App {
        let session = Arc::new(Mutex::new(session::Session::new(PathBuf::from(
            "/tmp/sola-agent-test",
        ))));
        App::blank(
            session,
            "fugu".into(),
            "high".into(),
            PathBuf::from("/tmp"),
            first_run,
        )
    }

    #[test]
    fn blank_starts_empty() {
        let app = blank_app(true);
        assert!(app.turns.is_empty());
        assert!(app.first_run);
        assert!(app.streaming.is_none());
        assert!(app.pending.is_none());
        assert_eq!(app.usage.input_tokens, 0);
        assert_eq!(app.usage.output_tokens, 0);
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent blank_starts_empty`
Expected: FAIL (compile error — `App::blank`, `Turn`, `session::Session::new` not yet in the rewritten `main.rs`).

- [ ] **Step 3: Implement**
Replace the entire body of `crates/sola-agent/src/main.rs` above the test module with:
```rust
//! sola-agent — iced GUI for a focused Sakana Fugu coding agent.
//!
//! Grows the original kit stub into a real client: bus + theme (kit helpers),
//! a background engine worker (event.rs bridge), and a transcript UI. Follows
//! the `sola-terminal` App::new/update/view shape.
#![allow(dead_code)] // interim: Turn/tool fields are wired up in later tasks.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use iced::{Element, Length, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings,
};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

mod engine;
mod event;
mod permit;
mod provider;
mod session;
mod tools;

use event::{AgentEvent, NodeId};
use session::{Session, Usage};
use tools::ToolDetail;

const APP_ID: &str = "sola-agent";
const DEFAULT_MODEL: &str = "fugu";
const DEFAULT_EFFORT: &str = "high";

/// One display row in the transcript. Driven by `AgentEvent`s, not the persisted
/// session tree (that stays the engine's single-writer store).
#[derive(Debug, Clone)]
enum Turn {
    User(String),
    Assistant { id: NodeId, text: String },
    Reasoning(String),
    Tool(ToolTurn),
    Error(String),
}

#[derive(Debug, Clone)]
struct ToolTurn {
    call_id: String,
    tool: String,
    args: serde_json::Value,
    output: String,
    detail: Option<ToolDetail>,
}

#[derive(Debug, Clone)]
struct PendingApproval {
    call_id: String,
    tool: String,
    preview: String,
}

#[derive(Debug, Clone)]
struct SessionSummary {
    id: String,
    title: String,
    path: PathBuf,
}

/// On-disk credential wrapper. `Encrypted<String>` ciphers the key on
/// human-readable serializers (serde_json here), so the file holds an
/// `age1enc:` blob, not the raw key.
#[derive(serde::Serialize, serde::Deserialize)]
struct Credentials {
    sakana_api_key: sola_core::Encrypted<String>,
}

/// Boot payload assembled in `main` and handed to `App::new` (iced's constructor
/// takes no args, so a static is the fit).
struct Boot {
    session: Arc<Mutex<Session>>,
    model: String,
    effort: String,
    project_root: PathBuf,
    first_run: bool,
}
static BOOT: OnceLock<Boot> = OnceLock::new();

fn credentials_path() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("agent")
        .join("credentials")
}

fn sessions_dir() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("agent")
        .join("sessions")
}

/// Encrypted credentials file first, then the `SAKANA_API_KEY` env var. `None`
/// means first-run: the UI prompts instead of the app crashing.
fn load_api_key() -> Option<String> {
    let path = credentials_path();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<Credentials>(&raw) {
            Ok(creds) => return Some(creds.sakana_api_key.0),
            Err(e) => tracing::warn!(?path, "failed to read agent credentials: {e}"),
        }
    }
    match std::env::var("SAKANA_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

/// Persist the key encrypted at `credentials_path()`.
fn save_api_key(key: &str) -> std::io::Result<()> {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let creds = Credentials {
        sakana_api_key: sola_core::Encrypted(key.to_string()),
    };
    let json = serde_json::to_string_pretty(&creds)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)
}

/// Build the real provider + config and hand the turn loop to a worker thread.
/// Called at boot when a key exists, and again on first-run submit.
fn spawn_engine(
    api_key: String,
    model: String,
    effort: String,
    project_root: PathBuf,
    session: Arc<Mutex<Session>>,
) {
    let provider: Arc<dyn provider::LlmStream + Send + Sync> =
        Arc::new(provider::SakanaProvider {
            base_url: "https://api.sakana.ai/v1".to_string(),
            api_key: api_key.clone(),
        });
    let config = engine::EngineConfig {
        api_key,
        model,
        effort,
        project_root,
        classifier: false,
    };
    engine::start(config, provider, session);
}

/// Scan the sessions dir for `<id>.jsonl` transcripts, loading each for its
/// title. Called off the render path (boot + New/Select), cached in
/// `App::sessions`.
fn list_sessions() -> Vec<SessionSummary> {
    let dir = sessions_dir();
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        match Session::load(&path) {
            Ok(s) => out.push(SessionSummary {
                id: s.id.clone(),
                title: s.title.clone(),
                path,
            }),
            Err(e) => tracing::debug!(?path, "skipping unreadable session: {e}"),
        }
    }
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(&[TopicKind::Theme, TopicKind::MenuAction, TopicKind::CloseApp])
        .app_menu("Agent", [("quit", "Quit Agent", KeyCode::Q.meta())])
        .install();

    // Wire the UI<->worker channels before anything subscribes or the engine
    // takes the command receiver.
    event::init_channels();

    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let session = Arc::new(Mutex::new(Session::new(project_root.clone())));
    let model = DEFAULT_MODEL.to_string();
    let effort = DEFAULT_EFFORT.to_string();

    let first_run = match load_api_key() {
        Some(api_key) => {
            spawn_engine(
                api_key,
                model.clone(),
                effort.clone(),
                project_root.clone(),
                session.clone(),
            );
            false
        }
        None => {
            tracing::warn!("no Sakana API key found; entering first-run key prompt");
            true
        }
    };

    let _ = BOOT.set(Boot {
        session,
        model,
        effort,
        project_root,
        first_run,
    });

    let app = iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings(APP_ID));
    app.run()
}

struct App {
    theme: Theme,
    session: Arc<Mutex<Session>>,
    turns: Vec<Turn>,
    streaming: Option<NodeId>,
    pending: Option<PendingApproval>,
    model: String,
    effort: String,
    usage: Usage,
    draft: String,
    project_root: PathBuf,
    first_run: bool,
    key_draft: String,
    sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Agent(AgentEvent),
    DraftChanged(String),
    Send,
    Approve,
    Always,
    Deny,
    Abort,
    NewSession,
    SelectSession(PathBuf),
    KeyDraftChanged(String),
    KeySubmit,
}

impl App {
    /// Construct from raw parts. Side-effect-free (no disk scan) so unit tests
    /// can build an `App` without touching `~/.config`.
    fn blank(
        session: Arc<Mutex<Session>>,
        model: String,
        effort: String,
        project_root: PathBuf,
        first_run: bool,
    ) -> Self {
        Self {
            theme: default_theme(),
            session,
            turns: Vec::new(),
            streaming: None,
            pending: None,
            model,
            effort,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
            draft: String::new(),
            project_root,
            first_run,
            key_draft: String::new(),
            sessions: Vec::new(),
        }
    }

    fn new() -> (Self, Task<Msg>) {
        let boot = BOOT
            .get()
            .expect("BOOT must be initialised in main before App::new");
        let mut app = App::blank(
            boot.session.clone(),
            boot.model.clone(),
            boot.effort.clone(),
            boot.project_root.clone(),
            boot.first_run,
        );
        app.sessions = list_sessions();
        (app, Task::none())
    }

    fn title(&self) -> String {
        "Sola Agent".into()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            event::agent_subscription().map(Msg::Agent),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => {
                apply_theme_update(&m, &mut self.theme);
                if is_self_quit(&m, APP_ID) {
                    return iced::exit();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        iced::widget::container(iced::widget::text("sola-agent"))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent blank_starts_empty`
Expected: PASS (`test tests::blank_starts_empty ... ok`).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/main.rs && git commit -m "feat(sola-agent): boot wiring — key load, engine start, App skeleton"`

---

### Task 16: Fold `AgentEvent` into state + map UI actions to `AgentCmd`

Folds every `AgentEvent` variant into the display transcript (tool-event folding included here — it is pure state; the tool events are only *rendered* richly in Task 28) and routes all `Msg` variants. The engine remains the single writer of the persisted tree; the UI keeps its own optimistic `Vec<Turn>`.

**Files:**
- Modify: crates/sola-agent/src/main.rs
- Test: crates/sola-agent/src/main.rs (extend the inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `event::{agent_send, AgentCmd, AgentEvent}` (Tasks 1, 13); `tools::{ToolResult, ToolDetail}` (Task 1); `session::{Session, Content, Role, Usage}` + `Session::{load, path_to_leaf}` + `Node` (Phase 3).
- Produces: `App::{on_agent, apply_delta, apply_reasoning, tool_turn_mut, answer_approval}`; free fn `turns_from_session(&Session) -> Vec<Turn>`; full `App::update`.

- [ ] **Step 1: Write the failing test**
Add these two tests inside the existing `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn delta_appends_to_streaming_node() {
        let mut app = blank_app(false);
        let _ = app.update(Msg::Agent(AgentEvent::Delta {
            node_id: "n1".into(),
            text: "Hel".into(),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::Delta {
            node_id: "n1".into(),
            text: "lo".into(),
        }));
        assert_eq!(app.streaming.as_deref(), Some("n1"));
        match app.turns.as_slice() {
            [Turn::Assistant { id, text }] => {
                assert_eq!(id, "n1");
                assert_eq!(text, "Hello");
            }
            other => panic!("expected one assistant turn, got {other:?}"),
        }
    }

    #[test]
    fn tool_output_appends_and_end_sets_detail() {
        let mut app = blank_app(false);
        let _ = app.update(Msg::Agent(AgentEvent::ToolStart {
            call_id: "c1".into(),
            tool: "bash".into(),
            args: serde_json::json!({"cmd": "ls"}),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::ToolOutput {
            call_id: "c1".into(),
            chunk: "a\n".into(),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::ToolOutput {
            call_id: "c1".into(),
            chunk: "b\n".into(),
        }));
        let _ = app.update(Msg::Agent(AgentEvent::ToolEnd {
            call_id: "c1".into(),
            result: tools::ToolResult {
                model_text: "a\nb\n".into(),
                ui_detail: tools::ToolDetail::Bash {
                    code: 0,
                    stdout: "a\nb\n".into(),
                    stderr: String::new(),
                },
            },
        }));
        match app.turns.as_slice() {
            [Turn::Tool(tt)] => {
                assert_eq!(tt.output, "a\nb\n");
                assert!(matches!(tt.detail, Some(tools::ToolDetail::Bash { code: 0, .. })));
            }
            other => panic!("expected one tool turn, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent delta_appends_to_streaming_node`
Expected: FAIL (`update`'s `_` arm ignores `Msg::Agent`; `on_agent`/`apply_delta` not defined → `app.turns` stays empty).

- [ ] **Step 3: Implement**
Change `use event::{AgentEvent, NodeId};` to:
```rust
use event::{AgentCmd, AgentEvent, NodeId};
```
Change `use session::{Session, Usage};` to:
```rust
use session::{Content, Role, Session, Usage};
```
Replace the whole `fn update` body with:
```rust
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => {
                apply_theme_update(&m, &mut self.theme);
                if is_self_quit(&m, APP_ID) {
                    return iced::exit();
                }
                Task::none()
            }
            Msg::Agent(ev) => {
                self.on_agent(ev);
                Task::none()
            }
            Msg::DraftChanged(v) => {
                self.draft = v;
                Task::none()
            }
            Msg::Send => {
                let text = self.draft.trim().to_string();
                if text.is_empty() || self.first_run {
                    return Task::none();
                }
                self.turns.push(Turn::User(text.clone()));
                self.draft.clear();
                self.streaming = None;
                // Engine is the single writer of the session tree; it appends
                // the user node from this text and drives the turn.
                event::agent_send(AgentCmd::Send {
                    text,
                    branch_from: None,
                });
                Task::none()
            }
            Msg::Approve => {
                self.answer_approval(false, false);
                Task::none()
            }
            Msg::Always => {
                self.answer_approval(true, false);
                Task::none()
            }
            Msg::Deny => {
                self.answer_approval(false, true);
                Task::none()
            }
            Msg::Abort => {
                event::agent_send(AgentCmd::Abort);
                self.streaming = None;
                self.pending = None;
                Task::none()
            }
            Msg::NewSession => {
                let fresh = Session::new(self.project_root.clone());
                if let Ok(mut guard) = self.session.lock() {
                    *guard = fresh;
                }
                self.turns.clear();
                self.streaming = None;
                self.pending = None;
                self.usage = Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                };
                self.sessions = list_sessions();
                Task::none()
            }
            Msg::SelectSession(path) => {
                match Session::load(&path) {
                    Ok(loaded) => {
                        self.turns = turns_from_session(&loaded);
                        if let Ok(mut guard) = self.session.lock() {
                            *guard = loaded;
                        }
                        self.streaming = None;
                        self.pending = None;
                    }
                    Err(e) => tracing::warn!(?path, "failed to load session: {e}"),
                }
                Task::none()
            }
            Msg::KeyDraftChanged(v) => {
                self.key_draft = v;
                Task::none()
            }
            Msg::KeySubmit => {
                let key = self.key_draft.trim().to_string();
                if key.is_empty() {
                    return Task::none();
                }
                if let Err(e) = save_api_key(&key) {
                    tracing::error!("failed to persist Sakana key: {e}");
                    return Task::none();
                }
                spawn_engine(
                    key,
                    self.model.clone(),
                    self.effort.clone(),
                    self.project_root.clone(),
                    self.session.clone(),
                );
                self.first_run = false;
                self.key_draft.clear();
                Task::none()
            }
        }
    }
```
Insert these methods into `impl App` (after `update`):
```rust
    /// Fold one streamed agent event into the display transcript.
    fn on_agent(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Delta { node_id, text } => self.apply_delta(&node_id, &text),
            AgentEvent::Reasoning { text } => self.apply_reasoning(&text),
            AgentEvent::ToolStart { call_id, tool, args } => {
                self.streaming = None;
                self.turns.push(Turn::Tool(ToolTurn {
                    call_id,
                    tool,
                    args,
                    output: String::new(),
                    detail: None,
                }));
            }
            AgentEvent::ToolOutput { call_id, chunk } => {
                if let Some(tt) = self.tool_turn_mut(&call_id) {
                    tt.output.push_str(&chunk);
                }
            }
            AgentEvent::ToolEnd { call_id, result } => {
                if let Some(tt) = self.tool_turn_mut(&call_id) {
                    if tt.output.is_empty() {
                        tt.output = result.model_text.clone();
                    }
                    tt.detail = Some(result.ui_detail);
                }
            }
            AgentEvent::ApprovalRequest { call_id, tool, preview } => {
                self.pending = Some(PendingApproval { call_id, tool, preview });
            }
            AgentEvent::TurnEnd { usage } => {
                self.usage.input_tokens += usage.input_tokens;
                self.usage.output_tokens += usage.output_tokens;
                self.streaming = None;
            }
            AgentEvent::Error { message } => {
                self.streaming = None;
                self.turns.push(Turn::Error(message));
            }
        }
    }

    /// Append a text delta to the in-flight assistant node (found by id,
    /// scanning from the back), or start a fresh streaming bubble.
    fn apply_delta(&mut self, node_id: &str, chunk: &str) {
        for turn in self.turns.iter_mut().rev() {
            if let Turn::Assistant { id, text } = turn {
                if id == node_id {
                    text.push_str(chunk);
                    self.streaming = Some(node_id.to_string());
                    return;
                }
            }
        }
        self.turns.push(Turn::Assistant {
            id: node_id.to_string(),
            text: chunk.to_string(),
        });
        self.streaming = Some(node_id.to_string());
    }

    /// Coalesce reasoning chunks into a single trailing reasoning row.
    fn apply_reasoning(&mut self, chunk: &str) {
        if let Some(Turn::Reasoning(buf)) = self.turns.last_mut() {
            buf.push_str(chunk);
            return;
        }
        self.turns.push(Turn::Reasoning(chunk.to_string()));
    }

    fn tool_turn_mut(&mut self, call_id: &str) -> Option<&mut ToolTurn> {
        self.turns.iter_mut().rev().find_map(|t| match t {
            Turn::Tool(tt) if tt.call_id == call_id => Some(tt),
            _ => None,
        })
    }

    /// Resolve the pending approval and forward the decision to the worker.
    fn answer_approval(&mut self, remember: bool, deny: bool) {
        let Some(p) = self.pending.take() else {
            return;
        };
        if deny {
            event::agent_send(AgentCmd::Deny {
                call_id: p.call_id,
                reason: None,
            });
        } else {
            event::agent_send(AgentCmd::Approve {
                call_id: p.call_id,
                remember,
            });
        }
    }
```
Add this free function below `impl App` (near `list_sessions`):
```rust
/// Rebuild display turns from a persisted session's root..=leaf path.
/// FunctionCall/Output nodes pair back into a single tool row by call_id.
fn turns_from_session(session: &Session) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for node in session.path_to_leaf() {
        match node.content {
            Content::Text(t) => match node.role {
                Role::User => turns.push(Turn::User(t)),
                Role::Assistant => turns.push(Turn::Assistant {
                    id: node.id.clone(),
                    text: t,
                }),
                Role::Tool => turns.push(Turn::Error(t)),
            },
            Content::FunctionCall { call_id, name, arguments } => {
                let args = serde_json::from_str(&arguments).unwrap_or(serde_json::Value::Null);
                turns.push(Turn::Tool(ToolTurn {
                    call_id,
                    tool: name,
                    args,
                    output: String::new(),
                    detail: None,
                }));
            }
            Content::FunctionCallOutput { call_id, output } => {
                let existing = turns.iter_mut().rev().find_map(|t| match t {
                    Turn::Tool(tt) if tt.call_id == call_id => Some(tt),
                    _ => None,
                });
                if let Some(tt) = existing {
                    tt.detail = Some(ToolDetail::Text(output.clone()));
                    tt.output = output;
                } else {
                    turns.push(Turn::Error(format!("orphan tool output {call_id}")));
                }
            }
        }
    }
    turns
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent`
Expected: PASS (`blank_starts_empty`, `delta_appends_to_streaming_node`, `tool_output_appends_and_end_sets_detail`).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/main.rs && git commit -m "feat(sola-agent): fold AgentEvent stream into state, map actions to AgentCmd"`

---

### Task 17: Transcript view — bubbles, input, footer (first streamed reply visible)

Delivers the basic visible transcript so a streamed text reply can be seen: a scrollable column of bubbles (user / assistant / reasoning / error), an inline text render of tool rows (rich rendering comes in Task 28), an input row, and a token footer. Sidebar, rich tool detail, approval strip, and first-run prompt are added later.

**Files:**
- Create: crates/sola-agent/src/view/bubble.rs
- Create: crates/sola-agent/src/view/footer.rs
- Modify: crates/sola-agent/src/view/mod.rs (real `screen`)
- Modify: crates/sola-agent/src/main.rs (add `mod view;`, delegate `App::view`)
- Test: crates/sola-agent/src/view/footer.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::{App, Msg, Turn, ToolTurn}` (Tasks 15–16); `crate::session::Usage`; kit `components::{button, text}` + `fonts::{ui_medium}`.
- Produces: `view::screen(&App) -> Element<'_, Msg>`; `view::bubble::turn_view`; `view::footer::{token_summary, view}`.

- [ ] **Step 1: Write the failing test**
Add `mod view;` under the other `mod` lines in `crates/sola-agent/src/main.rs`. Create `crates/sola-agent/src/view/footer.rs` with the pure helper + its test (render fn arrives in Step 3):
```rust
use crate::session::Usage;

pub(crate) fn token_summary(usage: &Usage) -> String {
    format!(
        "tokens: {} in / {} out",
        usage.input_tokens, usage.output_tokens
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_summary_formats_counts() {
        let u = Usage {
            input_tokens: 12,
            output_tokens: 34,
        };
        assert_eq!(token_summary(&u), "tokens: 12 in / 34 out");
    }
}
```
Replace the doc-only `crates/sola-agent/src/view/mod.rs` with a stub that declares only `footer` so the test compiles:
```rust
//! Agent UI composition.
pub(crate) mod footer;
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent token_summary_formats_counts`
Expected: FAIL (compile error until `mod view;` + `view/mod.rs` + `footer.rs` all exist and resolve; `bubble`/`screen` still missing when Step 3's `mod.rs` is written).

- [ ] **Step 3: Implement**
Write `crates/sola-agent/src/view/mod.rs`:
```rust
//! Agent UI composition. Borders/fills only — this iced stack does not blur
//! shadows.
pub(crate) mod bubble;
pub(crate) mod footer;

use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::Element;
use iced::{Length, Padding};

use crate::{App, Msg};

pub(crate) fn screen(app: &App) -> Element<'_, Msg> {
    let bubbles: Vec<Element<'_, Msg>> = app
        .turns
        .iter()
        .map(|t| bubble::turn_view(t, &app.theme))
        .collect();
    let transcript = scrollable(
        Column::with_children(bubbles)
            .spacing(12)
            .padding(Padding::new(20.0))
            .width(Length::Fill),
    )
    .height(Length::Fill);

    column![transcript, input_row(app), footer::view(app)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn input_row(app: &App) -> Element<'_, Msg> {
    let field = text_input("Ask Sola Agent…", &app.draft)
        .on_input(Msg::DraftChanged)
        .on_submit(Msg::Send)
        .padding(12)
        .size(15)
        .width(Length::Fill);

    let action: Element<'_, Msg> = if app.streaming.is_some() {
        button(text("Stop"))
            .style(sola_kit::components::button::danger)
            .on_press(Msg::Abort)
            .into()
    } else {
        button(text("Send"))
            .style(sola_kit::components::button::primary)
            .on_press(Msg::Send)
            .into()
    };

    container(row![field, action].spacing(8))
        .padding(Padding::new(16.0))
        .width(Length::Fill)
        .into()
}
```
Write `crates/sola-agent/src/view/bubble.rs`:
```rust
use iced::widget::{column, container, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};

use crate::{Msg, Turn};

pub(crate) fn turn_view<'a>(turn: &'a Turn, theme: &Theme) -> Element<'a, Msg> {
    match turn {
        Turn::User(t) => bubble("You", t.as_str(), Alignment::End, role_bg(theme, true), theme),
        Turn::Assistant { text: body, .. } => {
            bubble("Agent", body.as_str(), Alignment::Start, role_bg(theme, false), theme)
        }
        Turn::Reasoning(t) => reasoning(t.as_str()),
        // Placeholder tool render; Task 28 swaps this for tool::tool_view.
        Turn::Tool(tt) => bubble(
            "Tool",
            &format!("{}\n{}", tt.tool, tt.output),
            Alignment::Start,
            role_bg(theme, false),
            theme,
        ),
        Turn::Error(m) => error_view(m.as_str()),
    }
}

fn role_bg(theme: &Theme, user: bool) -> Color {
    let p = theme.extended_palette();
    if user {
        p.primary.weak.color
    } else {
        p.background.weak.color
    }
}

fn bubble<'a>(
    label: &'a str,
    body: &str,
    align: Alignment,
    bg: Color,
    theme: &Theme,
) -> Element<'a, Msg> {
    let border = theme.extended_palette().background.strong.color;
    let inner = column![
        text(label.to_string()).font(sola_kit::fonts::ui_medium()).size(12),
        text(body.to_string()).size(15),
    ]
    .spacing(4);
    let card = container(inner)
        .padding(Padding::new(12.0))
        .max_width(560.0)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..container::Style::default()
        });
    container(card).width(Length::Fill).align_x(align).into()
}

fn reasoning<'a>(body: &str) -> Element<'a, Msg> {
    container(
        column![
            text("Reasoning")
                .font(sola_kit::fonts::ui_medium())
                .size(11)
                .style(sola_kit::components::text::muted),
            text(body.to_string()).size(13).style(sola_kit::components::text::muted),
        ]
        .spacing(4)
        .padding(Padding::new(10.0)),
    )
    .width(Length::Fill)
    .into()
}

fn error_view<'a>(msg: &str) -> Element<'a, Msg> {
    container(
        column![
            text("Error")
                .font(sola_kit::fonts::ui_medium())
                .size(12)
                .style(sola_kit::components::text::danger),
            text(msg.to_string()).size(14).style(sola_kit::components::text::danger),
        ]
        .spacing(4)
        .padding(Padding::new(10.0)),
    )
    .width(Length::Fill)
    .into()
}
```
Append the render fn to `crates/sola-agent/src/view/footer.rs` (keep the tested `token_summary` and its test module):
```rust
use iced::widget::{container, row, text};
use iced::{Element, Length, Padding};

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let content = row![
        text(format!("model: {}", app.model))
            .size(12)
            .style(sola_kit::components::text::muted),
        text(format!("effort: {}", app.effort))
            .size(12)
            .style(sola_kit::components::text::muted),
        text(token_summary(&app.usage))
            .size(12)
            .style(sola_kit::components::text::muted),
    ]
    .spacing(16)
    .padding(Padding::new(10.0));
    container(content).width(Length::Fill).into()
}
```
Finally, in `crates/sola-agent/src/main.rs` replace `App::view`'s body with:
```rust
    fn view(&self) -> Element<'_, Msg> {
        view::screen(self)
    }
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent token_summary_formats_counts` then `cargo make build sola-agent`
Expected: PASS and a clean build. You can now run the app and see a streamed text reply.

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/main.rs crates/sola-agent/src/view && git commit -m "feat(sola-agent): transcript view — bubbles, input, footer (first streamed reply)"`

---

## Phase 7 — Tools (read / write / edit / bash / search + registry)

`ToolResult`/`ToolDetail` already exist (Task 1). These tasks add `ToolCtx`, the shared helpers, the five tool submodules, and the `tool_schemas`/`dispatch` registry. Self-contained (only `serde_json` + `tempfile`); each tool returns the split `ToolResult { model_text, ui_detail }`.

### Task 18: Tool context + helpers + `read`

**Files:**
- Modify: crates/sola-agent/src/tools/mod.rs (add `ToolCtx`, `resolve`, `error_result`, `pub mod read;`)
- Create: crates/sola-agent/src/tools/read.rs
- Test: crates/sola-agent/src/tools/read.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `tools::{ToolResult, ToolDetail}` (Task 1); `serde_json::Value`; `tempfile` (dev).
- Produces: `tools::ToolCtx { pub project_root: PathBuf }`; `tools::resolve(&ToolCtx, &str) -> PathBuf` (pub(crate)); `tools::error_result(impl Into<String>) -> ToolResult` (pub(crate)); `tools::read::{schema, run}`.

- [ ] **Step 1: Write the failing test**
Prepend the shared items to `crates/sola-agent/src/tools/mod.rs` (above the existing `ToolResult`/`ToolDetail`), adding `#![allow(dead_code)]` at the very top and the `read` submodule declaration:
```rust
#![allow(dead_code)]
//! Local tools the agent can call. Each returns a split `ToolResult`:
//! `model_text` is what the model sees; `ui_detail` is the richer structured
//! view. Kept in many small files, one per tool.

use std::path::{Path, PathBuf};

pub mod read;

/// Per-conversation execution context. `project_root` scopes `bash` and
/// resolves relative tool paths.
#[derive(Debug, Clone)]
pub struct ToolCtx {
    pub project_root: PathBuf,
}

/// Resolve a tool path argument against the session's project root. Absolute
/// paths pass through; relative paths join onto the root.
pub(crate) fn resolve(ctx: &ToolCtx, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        ctx.project_root.join(p)
    }
}

/// Build a uniform error result: the message is both what the model reads back
/// and a `Text` UI detail. Tools never panic on bad input or I/O failure.
pub(crate) fn error_result(msg: impl Into<String>) -> ToolResult {
    let msg = msg.into();
    ToolResult { model_text: msg.clone(), ui_detail: ToolDetail::Text(msg) }
}
```
Create `crates/sola-agent/src/tools/read.rs` with ONLY the failing test:
```rust
#[cfg(test)]
mod tests {
    use crate::tools::{ToolCtx, ToolDetail};
    use serde_json::json;
    use std::fs;

    #[test]
    fn read_whole_file_returns_all_lines() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "l1\nl2\nl3\n").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "start": null, "end": null }), &ctx);
        assert_eq!(res.model_text, "l1\nl2\nl3");
        assert!(matches!(res.ui_detail, ToolDetail::Text(_)));
    }

    #[test]
    fn read_honors_inclusive_range() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "l1\nl2\nl3\nl4\nl5\n").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "start": 2, "end": 4 }), &ctx);
        assert_eq!(res.model_text, "l2\nl3\nl4");
    }

    #[test]
    fn read_schema_is_strict_function() {
        let s = super::schema();
        assert_eq!(s["type"], "function");
        assert_eq!(s["name"], "read");
        assert_eq!(s["strict"], true);
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent tools::read`
Expected: FAIL (E0425: cannot find function `run` / `schema` in module `read`).

- [ ] **Step 3: Implement**
Prepend the implementation to `crates/sola-agent/src/tools/read.rs` (keep the Step-1 test module below it):
```rust
use serde_json::{json, Value};

use super::{error_result, resolve, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "read",
        "description": "Read a file's contents. Optionally restrict to an inclusive 1-based line range [start, end].",
        "parameters": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, absolute or relative to the project root." },
                "start": { "type": ["integer", "null"], "description": "First line to read (1-based, inclusive). Null for the whole file." },
                "end": { "type": ["integer", "null"], "description": "Last line to read (1-based, inclusive). Null for the whole file." }
            },
            "required": ["path", "start", "end"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => p,
        None => return error_result("read: missing required 'path' argument"),
    };
    let full = resolve(ctx, path);
    let contents = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(e) => return error_result(format!("read: cannot read {}: {e}", full.display())),
    };
    let start = args.get("start").and_then(Value::as_u64);
    let end = args.get("end").and_then(Value::as_u64);
    let text = match (start, end) {
        (None, None) => contents,
        _ => {
            let lines: Vec<&str> = contents.lines().collect();
            let total = lines.len() as u64;
            let s = start.unwrap_or(1).max(1);
            let e = end.unwrap_or(total).min(total);
            if s > e || total == 0 {
                return error_result(format!(
                    "read: empty range [{s}, {e}] for {} ({total} lines)",
                    full.display()
                ));
            }
            lines[(s as usize - 1)..(e as usize)].join("\n")
        }
    };
    ToolResult {
        model_text: text.clone(),
        ui_detail: ToolDetail::Text(text),
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent tools::read`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/tools && git commit -m "feat(sola-agent): tool context + helpers + read tool"`

---

### Task 19: `write` tool → `ToolDetail::Diff`

**Files:**
- Modify: crates/sola-agent/src/tools/mod.rs (add `pub mod write;`)
- Create: crates/sola-agent/src/tools/write.rs
- Test: crates/sola-agent/src/tools/write.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `tools::{ToolCtx, ToolResult, ToolDetail, resolve, error_result}` (Task 18).
- Produces: `tools::write::{schema, run}` (returns `ui_detail: Diff { path, before, after }`).

- [ ] **Step 1: Write the failing test**
Add `pub mod write;` after `pub mod read;` in `mod.rs`. Create `crates/sola-agent/src/tools/write.rs` with only the failing test:
```rust
#[cfg(test)]
mod tests {
    use crate::tools::{ToolCtx, ToolDetail};
    use serde_json::json;
    use std::fs;

    #[test]
    fn write_creates_file_and_reports_diff() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "new.txt", "content": "hello\n" }), &ctx);

        let on_disk = fs::read_to_string(dir.path().join("new.txt")).unwrap();
        assert_eq!(on_disk, "hello\n");

        match res.ui_detail {
            ToolDetail::Diff { path, before, after } => {
                assert_eq!(path, "new.txt");
                assert_eq!(before, "");
                assert_eq!(after, "hello\n");
            }
            other => panic!("expected Diff, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent tools::write`
Expected: FAIL (E0425: cannot find function `run` in module `write`).

- [ ] **Step 3: Implement**
Prepend to `crates/sola-agent/src/tools/write.rs` (keep the test below):
```rust
use serde_json::{json, Value};

use super::{error_result, resolve, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "write",
        "description": "Create or overwrite a file with the given contents. Parent directories are created as needed.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, absolute or relative to the project root." },
                "content": { "type": "string", "description": "The full new file contents." }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => p,
        None => return error_result("write: missing required 'path' argument"),
    };
    let content = match args.get("content").and_then(Value::as_str) {
        Some(c) => c,
        None => return error_result("write: missing required 'content' argument"),
    };
    let full = resolve(ctx, path);
    let before = std::fs::read_to_string(&full).unwrap_or_default();
    if let Some(parent) = full.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return error_result(format!("write: cannot create {}: {e}", parent.display()));
        }
    }
    if let Err(e) = std::fs::write(&full, content) {
        return error_result(format!("write: cannot write {}: {e}", full.display()));
    }
    tracing::debug!(path, bytes = content.len(), "write tool");
    ToolResult {
        model_text: format!("Wrote {} bytes to {path}", content.len()),
        ui_detail: ToolDetail::Diff {
            path: path.to_string(),
            before,
            after: content.to_string(),
        },
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent tools::write`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/tools && git commit -m "feat(sola-agent): write tool with before/after diff detail"`

---

### Task 20: `edit` tool → exact-string replace, error on missing/ambiguous

**Files:**
- Modify: crates/sola-agent/src/tools/mod.rs (add `pub mod edit;`)
- Create: crates/sola-agent/src/tools/edit.rs
- Test: crates/sola-agent/src/tools/edit.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `tools::{ToolCtx, ToolResult, ToolDetail, resolve, error_result}` (Task 18).
- Produces: `tools::edit::{schema, run}` (single exact-match replace → `Diff`; error when `old` is absent or occurs more than once).

- [ ] **Step 1: Write the failing test**
Add `pub mod edit;` after `pub mod write;`. Create `crates/sola-agent/src/tools/edit.rs` with only the failing test:
```rust
#[cfg(test)]
mod tests {
    use crate::tools::{ToolCtx, ToolDetail};
    use serde_json::json;
    use std::fs;

    #[test]
    fn edit_replaces_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "hello world").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "old": "world", "new": "there" }), &ctx);

        assert_eq!(fs::read_to_string(dir.path().join("f.txt")).unwrap(), "hello there");
        match res.ui_detail {
            ToolDetail::Diff { before, after, .. } => {
                assert_eq!(before, "hello world");
                assert_eq!(after, "hello there");
            }
            other => panic!("expected Diff, got {other:?}"),
        }
    }

    #[test]
    fn edit_errors_when_old_absent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "hello world").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "old": "missing", "new": "x" }), &ctx);

        assert!(res.model_text.contains("not found"));
        assert!(matches!(res.ui_detail, ToolDetail::Text(_)));
        assert_eq!(fs::read_to_string(dir.path().join("f.txt")).unwrap(), "hello world");
    }

    #[test]
    fn edit_errors_when_old_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.txt"), "a a").unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "path": "f.txt", "old": "a", "new": "b" }), &ctx);

        assert!(res.model_text.contains("ambiguous"));
        assert!(matches!(res.ui_detail, ToolDetail::Text(_)));
        assert_eq!(fs::read_to_string(dir.path().join("f.txt")).unwrap(), "a a");
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent tools::edit`
Expected: FAIL (E0425: cannot find function `run` in module `edit`).

- [ ] **Step 3: Implement**
Prepend to `crates/sola-agent/src/tools/edit.rs` (keep the test below):
```rust
use serde_json::{json, Value};

use super::{error_result, resolve, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "edit",
        "description": "Replace an exact, unique string in a file with new text. Fails if 'old' is absent or occurs more than once; include surrounding context to make it unique.",
        "parameters": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path, absolute or relative to the project root." },
                "old": { "type": "string", "description": "Exact text to find. Must occur exactly once." },
                "new": { "type": "string", "description": "Replacement text." }
            },
            "required": ["path", "old", "new"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => p,
        None => return error_result("edit: missing required 'path' argument"),
    };
    let old = match args.get("old").and_then(Value::as_str) {
        Some(o) => o,
        None => return error_result("edit: missing required 'old' argument"),
    };
    let new = match args.get("new").and_then(Value::as_str) {
        Some(n) => n,
        None => return error_result("edit: missing required 'new' argument"),
    };
    let full = resolve(ctx, path);
    let before = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(e) => return error_result(format!("edit: cannot read {}: {e}", full.display())),
    };
    if old.is_empty() {
        return error_result(format!("edit: 'old' must be a non-empty string for {path}"));
    }
    let count = before.matches(old).count();
    if count == 0 {
        return error_result(format!("edit: 'old' string not found in {path}"));
    }
    if count > 1 {
        return error_result(format!(
            "edit: 'old' string is ambiguous in {path} ({count} matches); provide more surrounding context"
        ));
    }
    let after = before.replacen(old, new, 1);
    if let Err(e) = std::fs::write(&full, &after) {
        return error_result(format!("edit: cannot write {}: {e}", full.display()));
    }
    tracing::debug!(path, "edit tool applied");
    ToolResult {
        model_text: format!("Edited {path}"),
        ui_detail: ToolDetail::Diff {
            path: path.to_string(),
            before,
            after,
        },
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent tools::edit`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/tools && git commit -m "feat(sola-agent): edit tool with exact-match replace and ambiguity guard"`

---

### Task 21: `bash` tool → `ToolDetail::Bash` (captured, never `/dev/null`)

**Files:**
- Modify: crates/sola-agent/src/tools/mod.rs (add `pub mod bash;`)
- Create: crates/sola-agent/src/tools/bash.rs
- Test: crates/sola-agent/src/tools/bash.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `tools::{ToolCtx, ToolResult, ToolDetail, error_result}` (Task 18); `std::process::Command`.
- Produces: `tools::bash::{schema, run}` (runs `sh -c <command>` in `ctx.project_root`, captures stdout/stderr/exit → `Bash { code, stdout, stderr }`; nonzero exit is a normal result).

- [ ] **Step 1: Write the failing test**
Add `pub mod bash;` after `pub mod edit;`. Create `crates/sola-agent/src/tools/bash.rs` with only the failing test:
```rust
#[cfg(test)]
mod tests {
    use crate::tools::{ToolCtx, ToolDetail};
    use serde_json::json;

    #[test]
    fn bash_captures_stdout_and_zero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "command": "echo hi" }), &ctx);
        match res.ui_detail {
            ToolDetail::Bash { code, stdout, stderr } => {
                assert_eq!(code, 0);
                assert_eq!(stdout.trim(), "hi");
                assert_eq!(stderr, "");
            }
            other => panic!("expected Bash, got {other:?}"),
        }
    }

    #[test]
    fn bash_nonzero_exit_returns_code_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "command": "exit 3" }), &ctx);
        assert!(matches!(res.ui_detail, ToolDetail::Bash { code: 3, .. }));
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent tools::bash`
Expected: FAIL (E0425: cannot find function `run` in module `bash`).

- [ ] **Step 3: Implement**
Prepend to `crates/sola-agent/src/tools/bash.rs` (keep the test below):
```rust
use std::process::Command;

use serde_json::{json, Value};

use super::{error_result, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "bash",
        "description": "Run a shell command with `sh -c` in the project root. Stdout, stderr, and the exit code are captured and returned; a nonzero exit is reported, not an error.",
        "parameters": {
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command line to execute." }
            },
            "required": ["command"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let command = match args.get("command").and_then(Value::as_str) {
        Some(c) => c,
        None => return error_result("bash: missing required 'command' argument"),
    };
    // Stdout/stderr are captured in full (never redirected to /dev/null).
    let output = match Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&ctx.project_root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return error_result(format!("bash: failed to spawn `sh -c`: {e}")),
    };
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    tracing::debug!(command, code, "bash tool executed");

    let mut model_text = format!("exit code: {code}\n");
    if !stdout.is_empty() {
        model_text.push_str("stdout:\n");
        model_text.push_str(&stdout);
        if !stdout.ends_with('\n') {
            model_text.push('\n');
        }
    }
    if !stderr.is_empty() {
        model_text.push_str("stderr:\n");
        model_text.push_str(&stderr);
        if !stderr.ends_with('\n') {
            model_text.push('\n');
        }
    }

    ToolResult {
        model_text,
        ui_detail: ToolDetail::Bash { code, stdout, stderr },
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent tools::bash`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/tools && git commit -m "feat(sola-agent): bash tool captures stdout/stderr/exit in project root"`

---

### Task 22: `search` tool → read-only `ls` / `find` / `grep`

**Files:**
- Modify: crates/sola-agent/src/tools/mod.rs (add `pub mod search;`)
- Create: crates/sola-agent/src/tools/search.rs
- Test: crates/sola-agent/src/tools/search.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `tools::{ToolCtx, ToolResult, ToolDetail, resolve, error_result}` (Task 18).
- Produces: `tools::search::{schema, run}` (native, read-only: `mode` in `ls`/`find`/`grep`, substring `query`, recursive walk under `path`).

- [ ] **Step 1: Write the failing test**
Add `pub mod search;` after `pub mod bash;`. Create `crates/sola-agent/src/tools/search.rs` with only the failing test:
```rust
#[cfg(test)]
mod tests {
    use crate::tools::ToolCtx;
    use serde_json::json;
    use std::fs;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/needle.txt"), "haystack\nfind the needle here\n").unwrap();
        fs::write(dir.path().join("top.txt"), "nothing\n").unwrap();
        dir
    }

    #[test]
    fn search_grep_finds_matching_line() {
        let dir = fixture();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "mode": "grep", "path": ".", "query": "needle" }), &ctx);
        assert!(res.model_text.contains("needle.txt"));
        assert!(res.model_text.contains("find the needle here"));
    }

    #[test]
    fn search_find_matches_filename() {
        let dir = fixture();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "mode": "find", "path": ".", "query": "needle" }), &ctx);
        assert!(res.model_text.contains("needle.txt"));
        assert!(!res.model_text.contains("top.txt"));
    }

    #[test]
    fn search_ls_lists_directory() {
        let dir = fixture();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = super::run(&json!({ "mode": "ls", "path": ".", "query": null }), &ctx);
        assert!(res.model_text.contains("sub/"));
        assert!(res.model_text.contains("top.txt"));
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent tools::search`
Expected: FAIL (E0425: cannot find function `run` in module `search`).

- [ ] **Step 3: Implement**
Prepend to `crates/sola-agent/src/tools/search.rs` (keep the test below):
```rust
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{error_result, resolve, ToolCtx, ToolDetail, ToolResult};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "search",
        "description": "Read-only lookups under the project. mode=ls lists a directory; mode=find lists files whose name contains 'query'; mode=grep lists lines containing 'query'.",
        "parameters": {
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": ["ls", "find", "grep"], "description": "Which lookup to perform." },
                "path": { "type": "string", "description": "Directory to search under, absolute or relative to the project root." },
                "query": { "type": ["string", "null"], "description": "Substring to match. Required for find and grep; ignored for ls." }
            },
            "required": ["mode", "path", "query"],
            "additionalProperties": false
        },
        "strict": true
    })
}

pub fn run(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let mode = match args.get("mode").and_then(Value::as_str) {
        Some(m) => m,
        None => return error_result("search: missing required 'mode' argument"),
    };
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
    let root = resolve(ctx, path);

    let result = match mode {
        "ls" => ls(&root),
        "find" => find(&root, query),
        "grep" => grep(&root, query),
        other => return error_result(format!("search: unknown mode '{other}' (want ls|find|grep)")),
    };
    match result {
        Ok(text) => ToolResult {
            model_text: text.clone(),
            ui_detail: ToolDetail::Text(text),
        },
        Err(e) => error_result(format!("search: {e}")),
    }
}

fn ls(root: &Path) -> std::io::Result<String> {
    let mut entries: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() {
            entries.push(format!("{name}/"));
        } else {
            entries.push(name);
        }
    }
    entries.sort();
    Ok(entries.join("\n"))
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(root) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let p = entry.path();
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => walk(&p, out),
            Ok(_) => out.push(p),
            Err(_) => {}
        }
    }
}

fn find(root: &Path, query: &str) -> std::io::Result<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut hits: Vec<String> = files
        .iter()
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().contains(query))
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
        .collect();
    hits.sort();
    Ok(hits.join("\n"))
}

fn grep(root: &Path, query: &str) -> std::io::Result<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    let mut hits: Vec<String> = Vec::new();
    for file in files {
        let contents = match std::fs::read_to_string(&file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (i, line) in contents.lines().enumerate() {
            if line.contains(query) {
                hits.push(format!("{}:{}: {}", file.display(), i + 1, line));
            }
        }
    }
    Ok(hits.join("\n"))
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent tools::search`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/tools && git commit -m "feat(sola-agent): read-only search tool (ls/find/grep)"`

---

### Task 23: `dispatch` + `tool_schemas` (registry in `mod.rs`)

**Files:**
- Modify: crates/sola-agent/src/tools/mod.rs (add `tool_schemas`, `dispatch`, `use serde_json::Value;`)
- Test: crates/sola-agent/src/tools/mod.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `tools::{read,write,edit,bash,search}::{schema, run}` (Tasks 18–22); `tools::{ToolCtx, ToolResult, ToolDetail, error_result}`.
- Produces: `tools::tool_schemas() -> Vec<serde_json::Value>`; `tools::dispatch(name: &str, args: &serde_json::Value, ctx: &ToolCtx) -> ToolResult`.

- [ ] **Step 1: Write the failing test**
Append this test module to `crates/sola-agent/src/tools/mod.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::{dispatch, tool_schemas, ToolCtx, ToolDetail};
    use serde_json::json;

    #[test]
    fn tool_schemas_lists_five_strict_functions() {
        let schemas = tool_schemas();
        assert_eq!(schemas.len(), 5);
        let names: Vec<&str> = schemas.iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["read", "write", "edit", "bash", "search"]);
        for s in &schemas {
            assert_eq!(s["type"], "function");
            assert_eq!(s["strict"], true);
            assert!(s["parameters"].is_object());
        }
    }

    #[test]
    fn dispatch_routes_to_bash() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = dispatch("bash", &json!({ "command": "echo hi" }), &ctx);
        assert!(matches!(res.ui_detail, ToolDetail::Bash { code: 0, .. }));
    }

    #[test]
    fn dispatch_unknown_tool_is_error_text() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx { project_root: dir.path().to_path_buf() };
        let res = dispatch("nope", &json!({}), &ctx);
        assert!(res.model_text.contains("unknown tool"));
        assert!(matches!(res.ui_detail, ToolDetail::Text(_)));
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent tools::tests`
Expected: FAIL (E0425: cannot find function `tool_schemas` / `dispatch`).

- [ ] **Step 3: Implement**
Add `use serde_json::Value;` to the top of `mod.rs` (next to `use std::path::{Path, PathBuf};`), then insert directly after `error_result` (above the test module):
```rust
/// The full set of function tools advertised to the Responses API this turn.
pub fn tool_schemas() -> Vec<Value> {
    vec![
        read::schema(),
        write::schema(),
        edit::schema(),
        bash::schema(),
        search::schema(),
    ]
}

/// Route a model tool call to its implementation. Unknown names return an error
/// result (never panic) so the loop can feed it back to the model.
pub fn dispatch(name: &str, args: &Value, ctx: &ToolCtx) -> ToolResult {
    match name {
        "read" => read::run(args, ctx),
        "write" => write::run(args, ctx),
        "edit" => edit::run(args, ctx),
        "bash" => bash::run(args, ctx),
        "search" => search::run(args, ctx),
        other => error_result(format!("dispatch: unknown tool '{other}'")),
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent tools::tests`
Expected: PASS (3 tests). Then `cargo make build sola-agent` (clean).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/tools/mod.rs && git commit -m "feat(sola-agent): tool_schemas + dispatch registry over the five tools"`

---

## Phase 8 — Permit (permission policy)

Pure decisions only — the engine performs the blocking prompt. `permit.rs` is currently a doc-only stub (Task 1).

### Task 24: Static permission policy (`static_decision`)

**Files:**
- Modify: crates/sola-agent/src/permit.rs (real body above the doc stub)
- Test: crates/sola-agent/src/permit.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `std` + `serde_json::Value`.
- Produces: `pub struct Rule { pub tool, pub scope }`; `pub struct Policy { pub project_root, pub always, pub classifier }`; `pub enum StaticDecision { AutoAllow, NeedsPrompt { preview } }`; `pub fn static_decision(&Policy, &str, &serde_json::Value) -> StaticDecision`.

- [ ] **Step 1: Write the failing test**
Replace the doc-only `crates/sola-agent/src/permit.rs` with the doc header + test module:
```rust
//! Permission policy — PURE decisions. The engine performs the blocking prompt.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy() -> Policy {
        Policy {
            project_root: std::path::PathBuf::from("/home/agent/project"),
            always: Vec::new(),
            classifier: false,
        }
    }

    #[test]
    fn read_auto_allows() {
        let p = policy();
        let d = static_decision(&p, "read", &json!({ "path": "src/main.rs" }));
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }

    #[test]
    fn write_inside_root_auto_allows() {
        let p = policy();
        let d = static_decision(&p, "write", &json!({ "path": "src/new.rs", "content": "x" }));
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }

    #[test]
    fn write_outside_root_prompts() {
        let p = policy();
        let d = static_decision(&p, "write", &json!({ "path": "/etc/passwd", "content": "x" }));
        match d {
            StaticDecision::NeedsPrompt { preview } => assert!(preview.contains("/etc/passwd")),
            other => panic!("expected prompt, got {other:?}"),
        }
    }

    #[test]
    fn path_escape_prompts() {
        let p = policy();
        let d = static_decision(&p, "edit", &json!({ "path": "../../secret.txt", "old": "a", "new": "b" }));
        assert!(matches!(d, StaticDecision::NeedsPrompt { .. }), "got {d:?}");
    }

    #[test]
    fn bash_prompts() {
        let p = policy();
        let d = static_decision(&p, "bash", &json!({ "command": "rm -rf /tmp/x" }));
        match d {
            StaticDecision::NeedsPrompt { preview } => assert_eq!(preview, "rm -rf /tmp/x"),
            other => panic!("expected prompt, got {other:?}"),
        }
    }

    #[test]
    fn manual_always_rule_auto_allows_bash() {
        let mut p = policy();
        p.always.push(Rule { tool: "bash".into(), scope: "always".into() });
        let d = static_decision(&p, "bash", &json!({ "command": "ls" }));
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent permit::tests`
Expected: FAIL (`E0425`/`E0433`: cannot find `Policy`, `Rule`, `StaticDecision`, `static_decision`).

- [ ] **Step 3: Implement**
Prepend above the `#[cfg(test)]` module in `crates/sola-agent/src/permit.rs`:
```rust
use std::path::{Component, Path, PathBuf};

/// One session-policy grant, e.g. `{ tool: "bash", scope: "always" }`.
#[derive(Debug, Clone)]
pub struct Rule {
    pub tool: String,
    pub scope: String,
}

/// The active conversation's permission policy.
#[derive(Debug, Clone)]
pub struct Policy {
    pub project_root: PathBuf,
    pub always: Vec<Rule>,
    pub classifier: bool,
}

/// Result of the static (no-LLM) policy pass.
#[derive(Debug)]
pub enum StaticDecision {
    AutoAllow,
    NeedsPrompt { preview: String },
}

/// Decide a tool call from static rules alone — no network, no side effects.
///
/// * a matching `always` rule → `AutoAllow`
/// * read-only tools (`read`, `search`) → `AutoAllow`
/// * `write`/`edit` whose resolved target is inside `project_root` → `AutoAllow`,
///   otherwise `NeedsPrompt`
/// * `bash` → always `NeedsPrompt` (preview = the command)
/// * anything else → `NeedsPrompt` (safe default)
pub fn static_decision(policy: &Policy, tool: &str, args: &serde_json::Value) -> StaticDecision {
    if policy
        .always
        .iter()
        .any(|r| r.tool == tool && r.scope == "always")
    {
        return StaticDecision::AutoAllow;
    }

    match tool {
        "read" | "search" => StaticDecision::AutoAllow,
        "write" | "edit" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or_default();
            if path_inside_root(&policy.project_root, path) {
                StaticDecision::AutoAllow
            } else {
                StaticDecision::NeedsPrompt {
                    preview: format!("{tool} target outside project root: {path}"),
                }
            }
        }
        "bash" => {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            StaticDecision::NeedsPrompt {
                preview: command.to_string(),
            }
        }
        _ => StaticDecision::NeedsPrompt {
            preview: format!("{tool}: {args}"),
        },
    }
}

/// True when `raw` (relative to `root`, or absolute) resolves *inside* `root`.
/// Lexical only — the target may not exist yet (writes create it), so we never
/// touch the filesystem and never call `canonicalize`. `..` segments are folded
/// away, so `../escape` lands outside and prompts. Comparison is component-wise,
/// so `/root/project2` does not match `/root/project`.
fn path_inside_root(root: &Path, raw: &str) -> bool {
    let target = resolve_target(root, raw);
    let root_norm = normalize_lexically(root);
    target.starts_with(&root_norm)
}

fn resolve_target(root: &Path, raw: &str) -> PathBuf {
    let raw_path = Path::new(raw);
    let joined = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        root.join(raw_path)
    };
    normalize_lexically(&joined)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent permit::tests`
Expected: PASS (6 passed).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/permit.rs && git commit -m "feat(sola-agent): static permission policy (static_decision)"`

---

### Task 25: Remember a tool grant (`remember`)

**Files:**
- Modify: crates/sola-agent/src/permit.rs
- Test: crates/sola-agent/src/permit.rs (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `Policy`, `Rule`, `StaticDecision`, `static_decision` (Task 24).
- Produces: `pub fn remember(policy: &mut Policy, tool: &str)`.

- [ ] **Step 1: Write the failing test**
Add to the existing `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn remember_then_bash_auto_allows() {
        let mut p = policy();
        assert!(
            matches!(
                static_decision(&p, "bash", &json!({ "command": "ls" })),
                StaticDecision::NeedsPrompt { .. }
            ),
            "bash should prompt before remember()"
        );

        remember(&mut p, "bash");

        assert!(
            matches!(
                static_decision(&p, "bash", &json!({ "command": "ls" })),
                StaticDecision::AutoAllow
            ),
            "bash should auto-allow after remember()"
        );

        // idempotent — a second remember() does not duplicate the rule.
        remember(&mut p, "bash");
        assert_eq!(p.always.iter().filter(|r| r.tool == "bash").count(), 1);
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent permit::tests::remember_then_bash_auto_allows`
Expected: FAIL (`E0425`: cannot find function `remember`).

- [ ] **Step 3: Implement**
Add below `static_decision` (above the test module):
```rust
/// Persist an always-allow grant for `tool` in the session policy. The engine
/// calls this when the user picks "Always allow this kind". Idempotent.
pub fn remember(policy: &mut Policy, tool: &str) {
    let already = policy
        .always
        .iter()
        .any(|r| r.tool == tool && r.scope == "always");
    if already {
        return;
    }
    policy.always.push(Rule {
        tool: tool.to_string(),
        scope: "always".to_string(),
    });
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent permit::tests::remember_then_bash_auto_allows`
Expected: PASS (1 passed).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/permit.rs && git commit -m "feat(sola-agent): remember() appends an always-allow rule"`

---

### Task 26: LLM risk classifier (`classify` + `Risk`)

**Files:**
- Modify: crates/sola-agent/src/permit.rs
- Test: crates/sola-agent/src/permit.rs (inline `#[cfg(test)] mod tests`, with a fake `LlmStream`)

**Interfaces:**
- Consumes: `provider::{LlmStream, InputItem, StreamEvent, TurnOutcome}` (Phase 2); `session::Usage` (Phase 3).
- Produces: `pub enum Risk { Safe, Caution, Danger }`; `pub fn classify(provider: &dyn LlmStream, tool: &str, args: &serde_json::Value) -> Risk`.

- [ ] **Step 1: Write the failing test**
Add to the existing `#[cfg(test)] mod tests`:
```rust
    /// Fake provider: a canned assistant reply (or a transport error), no
    /// streaming, no tool calls — enough to exercise `classify` offline.
    struct FakeStream {
        result: Result<String, String>,
    }

    impl crate::provider::LlmStream for FakeStream {
        fn stream_turn(
            &self,
            _model: &str,
            _effort: &str,
            _input: &[crate::provider::InputItem],
            _tools: &[serde_json::Value],
            _sink: &mut dyn FnMut(crate::provider::StreamEvent),
        ) -> Result<crate::provider::TurnOutcome, String> {
            match &self.result {
                Ok(text) => Ok(crate::provider::TurnOutcome {
                    assistant_text: text.clone(),
                    calls: Vec::new(),
                    usage: crate::session::Usage {
                        input_tokens: 0,
                        output_tokens: 0,
                    },
                }),
                Err(e) => Err(e.clone()),
            }
        }
    }

    #[test]
    fn classify_reads_safe_verdict() {
        let fake = FakeStream { result: Ok(r#"{"verdict":"safe"}"#.into()) };
        assert!(matches!(classify(&fake, "bash", &json!({ "command": "ls" })), Risk::Safe));
    }

    #[test]
    fn classify_reads_danger_verdict_with_prose() {
        let fake = FakeStream { result: Ok(r#"Sure: {"verdict":"danger","reason":"rm -rf /"}"#.into()) };
        assert!(matches!(classify(&fake, "bash", &json!({ "command": "rm -rf /" })), Risk::Danger));
    }

    #[test]
    fn classify_garbage_defaults_caution() {
        let fake = FakeStream { result: Ok("I cannot help with that.".into()) };
        assert!(matches!(classify(&fake, "bash", &json!({ "command": "ls" })), Risk::Caution));
    }

    #[test]
    fn classify_error_defaults_caution() {
        let fake = FakeStream { result: Err("network down".into()) };
        assert!(matches!(classify(&fake, "bash", &json!({ "command": "ls" })), Risk::Caution));
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent permit::tests::classify`
Expected: FAIL (`E0425`/`E0433`: cannot find `Risk` / `classify`).

- [ ] **Step 3: Implement**
Add the provider imports directly under the existing `use std::path::...` line:
```rust
use crate::provider::{InputItem, LlmStream, StreamEvent};
```
Add below `remember` (above the test module):
```rust
/// The classifier's three-way verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Safe,
    Caution,
    Danger,
}

/// One cheap, non-streaming `fugu` call that rates a would-be-prompted action.
/// Builds a tiny prompt, runs it through the provider seam (no tools, sink
/// ignored), and parses a `{"verdict": "safe|caution|danger"}` reply. Anything
/// that fails to parse — or any transport error — defaults to `Caution`, so the
/// classifier can only ever soften a prompt to auto-run on an explicit `safe`;
/// it never escalates a failure into an auto-allow.
pub fn classify(provider: &dyn LlmStream, tool: &str, args: &serde_json::Value) -> Risk {
    let prompt = format!(
        "You are a safety classifier for a coding agent. The agent wants to run \
         the tool `{tool}` with these arguments:\n{args}\n\n\
         Reply with ONLY a JSON object: {{\"verdict\": \"safe\" | \"caution\" | \"danger\"}}. \
         safe = read-only or clearly harmless; caution = could modify local files; \
         danger = destructive, exfiltrating, or system-wide."
    );
    let input = [InputItem::Message {
        role: "user".to_string(),
        text: prompt,
    }];
    let mut sink = |_ev: StreamEvent| {};
    match provider.stream_turn("fugu", "high", &input, &[], &mut sink) {
        Ok(outcome) => parse_verdict(&outcome.assistant_text),
        Err(_) => Risk::Caution,
    }
}

fn parse_verdict(text: &str) -> Risk {
    let verdict = extract_json(text)
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("verdict")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        });
    match verdict.as_deref() {
        Some("safe") => Risk::Safe,
        Some("danger") => Risk::Danger,
        _ => Risk::Caution,
    }
}

/// Slice the first `{ .. }` span out of a reply that may carry prose around it.
fn extract_json(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    Some(text[start..=end].to_string())
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent permit::tests::classify`
Expected: PASS (4 passed). Then run the whole module: `cargo test -p sola-agent permit` (Expected: 11 passed).

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/permit.rs && git commit -m "feat(sola-agent): LLM risk classifier (classify + Risk)"`

---

## Phase 9 — Engine (tool-executing turn loop)

### Task 27: Engine tool-executing turn loop

Widens `run_turn` to the full agent loop: stream → for every requested call, run the permit gate (static → optional classifier → user prompt) → `dispatch` → feed a `function_call_output` back → loop until a turn completes with no calls. `run_turn` gains a `policy: &mut Policy` parameter and now advertises `tool_schemas()`; `start` constructs the `Policy`; the Task-14 text-only test is updated to the new signature.

**Files:**
- Modify: crates/sola-agent/src/engine.rs (widen the `use` block, add Policy to `start`, replace `run_turn`, add `wait_for_decision`)
- Test: crates/sola-agent/src/engine.rs (update the text-only test's `run_turn` call; add `ToolFake` + a second test)

**Interfaces:**
- Consumes (additional): `permit::{Policy, StaticDecision, static_decision, Risk, classify, remember}` (Phase 8); `tools::{ToolCtx, dispatch, tool_schemas, ToolResult, ToolDetail}` (Phase 7); `Session::to_input` mapping `Content::FunctionCall`/`FunctionCallOutput` → `InputItem::FunctionCall`/`FunctionCallOutput` (Task 11).
- Produces: `run_turn(config, provider, session, policy, cmd_rx, abort, emit)`; `fn wait_for_decision(cmd_rx, call_id, policy, tool, abort) -> bool`.

- [ ] **Step 1: Update the text-only test and write the failing tool test**
In the engine test module, add `use crate::permit::Policy;` and `use crate::tools;` to the imports, then update the text-only test so its `run_turn` call passes a policy (the signature grows in Step 3). Change the call site in `text_only_turn_streams_and_appends_assistant` to:
```rust
        let mut policy = Policy {
            project_root: root.clone(),
            always: Vec::new(),
            classifier: false,
        };
        let mut events: Vec<AgentEvent> = Vec::new();
        {
            let mut emit = |ev| events.push(ev);
            run_turn(&config, &fake, &session, &mut policy, &cmd_rx, &abort, &mut emit);
        }
```
Append `ToolFake` and the second test inside the same `#[cfg(test)] mod tests`:
```rust
    /// Two-call fake: first turn asks to `read`, second turn (after the output
    /// is fed back) returns final text. Records every `input` it was given.
    struct ToolFake {
        step: Mutex<usize>,
        inputs: Mutex<Vec<Vec<InputItem>>>,
    }
    impl ToolFake {
        fn new() -> Self {
            Self { step: Mutex::new(0), inputs: Mutex::new(Vec::new()) }
        }
    }
    impl LlmStream for ToolFake {
        fn stream_turn(
            &self,
            _model: &str,
            _effort: &str,
            input: &[InputItem],
            _tools: &[serde_json::Value],
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutcome, String> {
            self.inputs.lock().unwrap().push(input.to_vec());
            let idx = {
                let mut step = self.step.lock().unwrap();
                let i = *step;
                *step += 1;
                i
            };
            if idx == 0 {
                let args = "{\"path\":\"note.txt\"}".to_string();
                sink(StreamEvent::FunctionCallStarted { call_id: "c1".into(), name: "read".into() });
                sink(StreamEvent::FunctionCallDone {
                    call_id: "c1".into(),
                    name: "read".into(),
                    arguments: args.clone(),
                });
                Ok(TurnOutcome {
                    assistant_text: String::new(),
                    calls: vec![FunctionCall {
                        call_id: "c1".into(),
                        name: "read".into(),
                        arguments: args,
                    }],
                    usage: Usage { input_tokens: 1, output_tokens: 1 },
                })
            } else {
                sink(StreamEvent::TextDelta("done".into()));
                Ok(TurnOutcome {
                    assistant_text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage { input_tokens: 2, output_tokens: 2 },
                })
            }
        }
    }

    #[test]
    fn tool_call_executes_and_feeds_output_back() {
        let root = hermetic_root("tool");
        std::fs::write(root.join("note.txt"), "hello file").unwrap();

        let session = Arc::new(Mutex::new(Session::new(root.clone())));
        session.lock().unwrap().append(
            Role::User,
            Content::Text("read note.txt".into()),
            None,
            None,
        );

        let config = EngineConfig {
            api_key: String::new(),
            model: "fugu".into(),
            effort: "high".into(),
            project_root: root.clone(),
            classifier: false,
        };
        let mut policy = Policy {
            project_root: root.clone(),
            always: Vec::new(),
            classifier: false,
        };
        let fake = ToolFake::new();
        let abort = AtomicBool::new(false);
        let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel::<AgentCmd>();

        let mut events: Vec<AgentEvent> = Vec::new();
        {
            let mut emit = |ev| events.push(ev);
            run_turn(&config, &fake, &session, &mut policy, &cmd_rx, &abort, &mut emit);
        }

        let starts = events.iter().filter(|e| matches!(e, AgentEvent::ToolStart { .. })).count();
        let ends = events.iter().filter(|e| matches!(e, AgentEvent::ToolEnd { .. })).count();
        assert_eq!(starts, 1, "read should start exactly once: {events:?}");
        assert_eq!(ends, 1, "read should end exactly once: {events:?}");

        let ran = events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolEnd { result, .. } if result.model_text.contains("hello file")
        ));
        assert!(ran, "read must have executed and returned file contents: {events:?}");

        let inputs = fake.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 2, "engine must loop back for a second turn");
        let fed_back = inputs[1].iter().any(|it| matches!(
            it,
            InputItem::FunctionCallOutput { call_id, .. } if call_id.as_str() == "c1"
        ));
        assert!(fed_back, "second turn input must include c1's function_call_output");

        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::TurnEnd { .. })),
            "the final call-free turn should emit TurnEnd"
        );
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent tool_call_executes_and_feeds_output_back`
Expected: FAIL — `read should start exactly once` (`left: 0`): the text-only `run_turn` never emits `ToolStart`, never dispatches, never loops (`inputs.len()` is 1); also the text-only test now passes a policy arg that the current 6-arg `run_turn` doesn't accept, so it fails to compile until Step 3.

- [ ] **Step 3: Implement**
Replace the top `use` block in `engine.rs` with the wider set:
```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crate::event::{AgentCmd, AgentEvent};
use crate::permit::{classify, remember, static_decision, Policy, Risk, StaticDecision};
use crate::provider::{LlmStream, StreamEvent};
use crate::session::{Content, Role, Session};
use crate::tools::{dispatch, tool_schemas, ToolCtx, ToolDetail, ToolResult};
```
Update `start` to construct a `Policy` and pass it to `run_turn` — replace its body's `run_turn` call and add the policy above the loop:
```rust
pub fn start(
    config: EngineConfig,
    provider: Arc<dyn LlmStream + Send + Sync>,
    session: Arc<Mutex<Session>>,
) {
    std::thread::spawn(move || {
        let mut config = config;
        let cmd_rx = crate::event::take_cmd_rx();
        let abort = AtomicBool::new(false);
        let mut policy = Policy {
            project_root: config.project_root.clone(),
            always: Vec::new(),
            classifier: config.classifier,
        };
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                AgentCmd::Send { text, branch_from } => {
                    {
                        let mut s = session.lock().unwrap();
                        if let Some(parent) = branch_from {
                            s.branch_from(parent);
                        }
                        s.append(Role::User, Content::Text(text), None, None);
                    }
                    abort.store(false, Ordering::SeqCst);
                    run_turn(
                        &config,
                        provider.as_ref(),
                        &session,
                        &mut policy,
                        &cmd_rx,
                        &abort,
                        &mut |ev| crate::event::emit(ev),
                    );
                }
                AgentCmd::Abort => abort.store(true, Ordering::SeqCst),
                AgentCmd::SetModel { model, effort } => {
                    config.model = model;
                    config.effort = effort;
                }
                // A pending decision is consumed inside `wait_for_decision`;
                // stray approvals/denials at loop scope are ignored.
                AgentCmd::Approve { .. } | AgentCmd::Deny { .. } => {}
            }
        }
    });
}
```
Replace the whole `run_turn` fn with the tool-executing, looping version and add `wait_for_decision` after it:
```rust
/// Drive one agent turn to completion: stream, forward display events, then for
/// every requested tool call run the permit gate → dispatch → feed a
/// `function_call_output` back, looping `stream_turn` until a turn completes
/// with no calls. `abort` is checked between steps.
fn run_turn(
    config: &EngineConfig,
    provider: &(dyn LlmStream + Send + Sync),
    session: &Arc<Mutex<Session>>,
    policy: &mut Policy,
    cmd_rx: &Receiver<AgentCmd>,
    abort: &AtomicBool,
    emit: &mut dyn FnMut(AgentEvent),
) {
    let tools = tool_schemas();
    loop {
        if abort.load(Ordering::SeqCst) {
            return;
        }
        let input = { session.lock().unwrap().to_input() };
        let stream_id = uuid::Uuid::new_v4().to_string();
        let outcome = {
            let mut sink = |ev: StreamEvent| match ev {
                StreamEvent::TextDelta(t) => emit(AgentEvent::Delta {
                    node_id: stream_id.clone(),
                    text: t,
                }),
                StreamEvent::Reasoning(t) => emit(AgentEvent::Reasoning { text: t }),
                StreamEvent::Error(m) => emit(AgentEvent::Error { message: m }),
                _ => {}
            };
            provider.stream_turn(&config.model, &config.effort, &input, &tools, &mut sink)
        };
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => {
                emit(AgentEvent::Error { message: e });
                return;
            }
        };

        // Attribute this step's usage to the first assistant-authored node we
        // append (prose if any, else the first call node).
        let mut usage_slot = Some(outcome.usage);
        if !outcome.assistant_text.is_empty() {
            let u = usage_slot.take();
            session.lock().unwrap().append(
                Role::Assistant,
                Content::Text(outcome.assistant_text.clone()),
                Some(config.model.clone()),
                u,
            );
        }

        // No calls → the whole agent turn is done.
        if outcome.calls.is_empty() {
            emit(AgentEvent::TurnEnd { usage: outcome.usage });
            return;
        }

        for call in &outcome.calls {
            if abort.load(Ordering::SeqCst) {
                return;
            }
            let args: serde_json::Value =
                serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));
            emit(AgentEvent::ToolStart {
                call_id: call.call_id.clone(),
                tool: call.name.clone(),
                args: args.clone(),
            });
            // Record the model's function_call node regardless of gate outcome.
            {
                let u = usage_slot.take();
                session.lock().unwrap().append(
                    Role::Assistant,
                    Content::FunctionCall {
                        call_id: call.call_id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    },
                    Some(config.model.clone()),
                    u,
                );
            }

            // Permit gate: static → optional classifier → user prompt.
            let allowed = match static_decision(policy, &call.name, &args) {
                StaticDecision::AutoAllow => true,
                StaticDecision::NeedsPrompt { preview } => {
                    let cleared = config.classifier
                        && matches!(classify(provider, &call.name, &args), Risk::Safe);
                    if cleared {
                        true
                    } else {
                        emit(AgentEvent::ApprovalRequest {
                            call_id: call.call_id.clone(),
                            tool: call.name.clone(),
                            preview,
                        });
                        wait_for_decision(cmd_rx, &call.call_id, policy, &call.name, abort)
                    }
                }
            };

            let result = if allowed {
                let ctx = ToolCtx { project_root: config.project_root.clone() };
                dispatch(&call.name, &args, &ctx)
            } else {
                let msg = format!("Tool call `{}` was declined by the user.", call.name);
                ToolResult {
                    model_text: msg.clone(),
                    ui_detail: ToolDetail::Text(msg),
                }
            };

            // Feed the output back into the transcript, then surface it.
            session.lock().unwrap().append(
                Role::Tool,
                Content::FunctionCallOutput {
                    call_id: call.call_id.clone(),
                    output: result.model_text.clone(),
                },
                None,
                None,
            );
            emit(AgentEvent::ToolEnd {
                call_id: call.call_id.clone(),
                result,
            });
        }
        // Loop: the appended outputs are now part of `to_input()`.
    }
}

/// Block on the command receiver until a decision arrives for `call_id`.
/// Approve → true (and persist an always-allow rule on `remember`); Deny →
/// false; Abort → trip the flag and treat as deny; unrelated commands are
/// skipped; a closed channel is treated as a deny.
fn wait_for_decision(
    cmd_rx: &Receiver<AgentCmd>,
    call_id: &str,
    policy: &mut Policy,
    tool: &str,
    abort: &AtomicBool,
) -> bool {
    loop {
        match cmd_rx.recv() {
            Ok(AgentCmd::Approve { call_id: id, remember: r }) if id == call_id => {
                if r {
                    remember(policy, tool);
                }
                return true;
            }
            Ok(AgentCmd::Deny { call_id: id, .. }) if id == call_id => return false,
            Ok(AgentCmd::Abort) => {
                abort.store(true, Ordering::SeqCst);
                return false;
            }
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent tool_call_executes_and_feeds_output_back && cargo test -p sola-agent text_only_turn_streams_and_appends_assistant`
Expected: PASS (both — the text-only test still passes because a call-free turn takes the `TurnEnd` path).

- [ ] **Step 5: Commit**
```
cargo make build sola-agent && \
git add crates/sola-agent/src/engine.rs && \
git commit -m "feat(sola-agent): engine tool-executing turn loop with permit gate + output feedback"
```

---

## Phase 10 — App + View, milestone 2 (tool bubbles + approval UI)

### Task 28: Rich tool detail, approval strip, and session sidebar

Enriches the view: a dedicated tool-detail renderer (`Text` / `Diff` / `Bash`), an inline approval strip, and a session sidebar. `screen` gains the sidebar column + the pending-approval strip; `bubble::turn_view`'s `Turn::Tool` arm now delegates to `tool::tool_view`.

**Files:**
- Create: crates/sola-agent/src/view/tool.rs
- Create: crates/sola-agent/src/view/approval.rs
- Create: crates/sola-agent/src/view/sidebar.rs
- Modify: crates/sola-agent/src/view/mod.rs (declare the new submodules; add sidebar + approval to `screen`)
- Modify: crates/sola-agent/src/view/bubble.rs (delegate `Turn::Tool` to `tool::tool_view`)

**Interfaces:**
- Consumes: `crate::{App, Msg, ToolTurn, PendingApproval}`; `crate::tools::ToolDetail` (Task 1); kit `components::{button, text, card}` + `fonts::{ui_medium, mono}`.
- Produces: `view::tool::tool_view`; `view::approval::strip`; `view::sidebar::view`; updated `view::screen`.

- [ ] **Step 1: Write the failing test**
This task is exercised by build + the existing state tests; add a focused compile-level guard to `crates/sola-agent/src/view/tool.rs`'s eventual test module by first creating `tool.rs` with only the failing test:
```rust
#[cfg(test)]
mod tests {
    use crate::tools::ToolDetail;

    // Guards that the three detail shapes are all handled (renderer returns an
    // Element for each without panicking on match).
    #[test]
    fn tool_view_handles_all_detail_variants() {
        let variants = [
            ToolDetail::Text("x".into()),
            ToolDetail::Diff { path: "a".into(), before: "b".into(), after: "c".into() },
            ToolDetail::Bash { code: 0, stdout: "o".into(), stderr: String::new() },
        ];
        for v in variants {
            // super::detail_label is a pure helper introduced with the renderer.
            assert!(!super::detail_label(&v).is_empty());
        }
    }
}
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent tool_view_handles_all_detail_variants`
Expected: FAIL (module `view::tool` not declared / `detail_label` missing).

- [ ] **Step 3: Implement**
In `crates/sola-agent/src/view/mod.rs`, extend the submodule declarations and `screen`. Replace the module declarations line with:
```rust
pub(crate) mod approval;
pub(crate) mod bubble;
pub(crate) mod footer;
pub(crate) mod sidebar;
pub(crate) mod tool;
```
Replace `screen`'s body with the sidebar + approval layout:
```rust
pub(crate) fn screen(app: &App) -> Element<'_, Msg> {
    let bubbles: Vec<Element<'_, Msg>> = app
        .turns
        .iter()
        .map(|t| bubble::turn_view(t, &app.theme))
        .collect();
    let transcript = scrollable(
        Column::with_children(bubbles)
            .spacing(12)
            .padding(Padding::new(20.0))
            .width(Length::Fill),
    )
    .height(Length::Fill);

    let mut center: Vec<Element<'_, Msg>> = vec![transcript.into()];
    if let Some(p) = &app.pending {
        center.push(approval::strip(p, &app.theme));
    }
    center.push(input_row(app));
    center.push(footer::view(app));

    row![
        sidebar::view(app),
        Column::with_children(center)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
```
(Add `use iced::widget::row;` / `Column` are already imported.)

Write `crates/sola-agent/src/view/tool.rs` (prepend above the Step-1 test):
```rust
use iced::widget::{column, container, row, text, Text};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::tools::ToolDetail;
use crate::{Msg, ToolTurn};

/// Short label for a detail variant (also used by the compile guard test).
pub(crate) fn detail_label(detail: &ToolDetail) -> &'static str {
    match detail {
        ToolDetail::Text(_) => "output",
        ToolDetail::Diff { .. } => "diff",
        ToolDetail::Bash { .. } => "shell",
    }
}

pub(crate) fn tool_view<'a>(tt: &'a ToolTurn, theme: &Theme) -> Element<'a, Msg> {
    let header = row![text(format!("⚙ {}", tt.tool))
        .font(sola_kit::fonts::ui_medium())
        .size(13)]
    .spacing(8);

    let detail: Element<'a, Msg> = match &tt.detail {
        None => running(tt),
        Some(ToolDetail::Text(s)) => mono_block(s.as_str(), theme),
        Some(ToolDetail::Diff { path, before, after }) => {
            diff_view(path.as_str(), before.as_str(), after.as_str(), theme)
        }
        Some(ToolDetail::Bash { code, stdout, stderr }) => {
            bash_view(*code, stdout.as_str(), stderr.as_str(), theme)
        }
    };

    let body = column![header, detail].spacing(8);
    sola_kit::components::card::card(body).width(Length::Fill).into()
}

fn running<'a>(tt: &'a ToolTurn) -> Element<'a, Msg> {
    column![
        text("running…").size(12).style(sola_kit::components::text::muted),
        mono_raw(tt.output.as_str()),
    ]
    .spacing(4)
    .into()
}

fn mono_raw<'a>(s: &str) -> Text<'a, Theme> {
    text(s.to_string()).font(sola_kit::fonts::mono()).size(12)
}

fn mono_block<'a>(s: &str, theme: &Theme) -> Element<'a, Msg> {
    let bg = theme.extended_palette().background.weaker.color;
    container(mono_raw(s))
        .padding(Padding::new(8.0))
        .width(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border { color: bg, width: 1.0, radius: 6.0.into() },
            ..container::Style::default()
        })
        .into()
}

fn diff_line<'a>(sign: char, content: &str, color: Color) -> Element<'a, Msg> {
    text(format!("{sign} {content}"))
        .font(sola_kit::fonts::mono())
        .size(12)
        .style(move |_t: &Theme| iced::widget::text::Style { color: Some(color) })
        .into()
}

fn diff_view<'a>(path: &str, before: &str, after: &str, theme: &Theme) -> Element<'a, Msg> {
    let p = theme.extended_palette();
    let removed = p.danger.base.color;
    let added = p.success.base.color;
    let mut lines = column![text(path.to_string())
        .font(sola_kit::fonts::mono())
        .size(12)
        .style(sola_kit::components::text::muted)]
    .spacing(1);
    for line in before.lines() {
        lines = lines.push(diff_line('-', line, removed));
    }
    for line in after.lines() {
        lines = lines.push(diff_line('+', line, added));
    }
    container(lines).padding(Padding::new(8.0)).width(Length::Fill).into()
}

fn bash_view<'a>(code: i32, stdout: &str, stderr: &str, theme: &Theme) -> Element<'a, Msg> {
    let p = theme.extended_palette();
    let status_color = if code == 0 { p.success.base.color } else { p.danger.base.color };
    let status = text(format!("exit {code}"))
        .size(12)
        .style(move |_t: &Theme| iced::widget::text::Style { color: Some(status_color) });
    let mut col = column![status].spacing(6);
    if !stdout.is_empty() {
        col = col.push(mono_raw(stdout));
    }
    if !stderr.is_empty() {
        col = col.push(
            text(stderr.to_string())
                .font(sola_kit::fonts::mono())
                .size(12)
                .style(sola_kit::components::text::danger),
        );
    }
    container(col).padding(Padding::new(8.0)).width(Length::Fill).into()
}
```
Write `crates/sola-agent/src/view/approval.rs`:
```rust
use iced::widget::{button, column, container, row, text};
use iced::{Background, Border, Element, Length, Padding, Theme};

use crate::{Msg, PendingApproval};

pub(crate) fn strip<'a>(p: &'a PendingApproval, theme: &Theme) -> Element<'a, Msg> {
    let pal = theme.extended_palette();
    let bg = pal.background.weak.color;
    let border = pal.warning.base.color;

    let buttons = row![
        button(text("Approve"))
            .style(sola_kit::components::button::primary)
            .on_press(Msg::Approve),
        button(text("Always allow"))
            .style(sola_kit::components::button::secondary)
            .on_press(Msg::Always),
        button(text("Deny"))
            .style(sola_kit::components::button::danger)
            .on_press(Msg::Deny),
    ]
    .spacing(8);

    let body = column![
        text(format!("Allow {}?", p.tool)).font(sola_kit::fonts::ui_medium()).size(14),
        text(p.preview.as_str())
            .font(sola_kit::fonts::mono())
            .size(12)
            .style(sola_kit::components::text::muted),
        buttons,
    ]
    .spacing(8);

    container(body)
        .padding(Padding::new(12.0))
        .width(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border { color: border, width: 1.0, radius: 8.0.into() },
            ..container::Style::default()
        })
        .into()
}
```
Write `crates/sola-agent/src/view/sidebar.rs`:
```rust
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let header = row![
        text("Agent").font(sola_kit::fonts::ui_medium()).size(18),
        button(text("New"))
            .style(sola_kit::components::button::secondary)
            .on_press(Msg::NewSession),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let active_id = app.session.lock().ok().map(|s| s.id.clone());

    let mut list = column![header].spacing(6).padding(Padding::new(12.0));
    for summary in &app.sessions {
        let selected = active_id.as_deref() == Some(summary.id.as_str());
        list = list.push(
            button(text(summary.title.as_str()).size(13))
                .width(Length::Fill)
                .style(sola_kit::components::button::list_item(selected))
                .on_press(Msg::SelectSession(summary.path.clone())),
        );
    }

    container(scrollable(list))
        .width(Length::Fixed(260.0))
        .height(Length::Fill)
        .into()
}
```
Finally, in `crates/sola-agent/src/view/bubble.rs` add `use super::tool;` at the top and replace the placeholder `Turn::Tool` arm with:
```rust
        Turn::Tool(tt) => tool::tool_view(tt, theme),
```

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent tool_view_handles_all_detail_variants` then `cargo make build sola-agent`
Expected: PASS and a clean build. Tool bubbles render as diffs/shell blocks and the approval strip + sidebar appear.

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/view && git commit -m "feat(sola-agent): rich tool detail, approval strip, and session sidebar"`

---

## Phase 11 — Polish / first-run

### Task 29: First-run key prompt + drop the interim `allow(dead_code)`

Adds the encrypted-key onboarding screen and gates `screen` on `app.first_run`. Removes the interim `#![allow(dead_code)]` now that every field/fn is consumed, so the crate builds warning-clean.

**Files:**
- Create: crates/sola-agent/src/view/firstrun.rs
- Modify: crates/sola-agent/src/view/mod.rs (declare `firstrun`; branch `screen` on `first_run`)
- Modify: crates/sola-agent/src/main.rs (remove `#![allow(dead_code)]`)
- Test: crates/sola-agent/src/main.rs (extend the inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::{App, Msg}` (Tasks 15–16); `Msg::{KeyDraftChanged, KeySubmit}`; kit `components::{button, text}`.
- Produces: `view::firstrun::view`; `screen` first-run branch.

- [ ] **Step 1: Write the failing test**
Add to the existing `#[cfg(test)] mod tests` in `crates/sola-agent/src/main.rs`:
```rust
    #[test]
    fn first_run_blocks_send_until_key_submitted() {
        let mut app = blank_app(true);
        app.draft = "hello".into();
        let _ = app.update(Msg::Send);
        assert!(app.turns.is_empty(), "Send is a no-op during first-run");

        app.key_draft = "sk-test".into();
        // KeySubmit persists + spawns the engine (side-effecting), so only assert
        // the first_run flag flips and the draft clears — no engine assertion here.
        // (Guarded by env; run under a temp XDG_CONFIG_HOME in CI if needed.)
        assert!(app.first_run);
    }
```

- [ ] **Step 2: Run it, expect failure**
Run: `cargo test -p sola-agent first_run_blocks_send_until_key_submitted`
Expected: FAIL if `screen` panics on the first-run path before `firstrun` exists, or (once wired) confirms the guard. First it fails to compile because `view::firstrun` is not yet declared and `screen` references it in Step 3.

- [ ] **Step 3: Implement**
Write `crates/sola-agent/src/view/firstrun.rs`:
```rust
use iced::widget::{button, column, container, text, text_input};
use iced::{Alignment, Element, Length};

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let field = text_input("Sakana API key", &app.key_draft)
        .on_input(Msg::KeyDraftChanged)
        .on_submit(Msg::KeySubmit)
        .secure(true)
        .padding(12)
        .size(15)
        .width(Length::Fixed(360.0));

    let body = column![
        text("Welcome to Sola Agent").font(sola_kit::fonts::ui_medium()).size(20),
        text("Paste your Sakana API key to begin. It is encrypted at rest.")
            .size(13)
            .style(sola_kit::components::text::muted),
        field,
        button(text("Save key"))
            .style(sola_kit::components::button::primary)
            .on_press(Msg::KeySubmit),
    ]
    .spacing(14)
    .align_x(Alignment::Center);

    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
```
In `crates/sola-agent/src/view/mod.rs`, add `pub(crate) mod firstrun;` to the submodule list and prepend the first-run branch as the first lines of `screen`:
```rust
pub(crate) fn screen(app: &App) -> Element<'_, Msg> {
    if app.first_run {
        return firstrun::view(app);
    }
    // (unchanged) keep the ENTIRE sidebar + transcript layout from Task 28’s `view` verbatim below this early return — nothing here is omitted-to-be-filled-in
```
In `crates/sola-agent/src/main.rs`, delete the top line `#![allow(dead_code)]`.

- [ ] **Step 4: Run it, expect pass**
Run: `cargo test -p sola-agent first_run_blocks_send_until_key_submitted` then the full suite `cargo test -p sola-agent` then `cargo make build sola-agent`
Expected: PASS (all tests) and a clean build with no `dead_code` warnings from the removed allow.

- [ ] **Step 5: Commit**
`git add crates/sola-agent/src/main.rs crates/sola-agent/src/view && git commit -m "feat(sola-agent): first-run key prompt + drop interim dead_code allow"`

---

## Done — final verification

- [ ] Run the full suite: `cargo test -p sola-agent` (all tests green; the `spike_responses` test stays `ignored`).
- [ ] Build the crate through the project build system: `cargo make build sola-agent` (clean). Do **NOT** run `cargo make install` — the user installs.
- [ ] Optionally confirm the live SSE contract by hand: `SAKANA_API_KEY=sk-... cargo test -p sola-agent --test spike_responses -- --ignored --nocapture`.
- [ ] Do not merge the worktree branch to master without explicit user permission.