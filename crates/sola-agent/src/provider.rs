//! Sakana Fugu Responses client + the small test seam.
//!
//! Foundation defines the shared wire types and the `LlmStream` trait.
//! The real `SakanaProvider` impl (ureq streaming, SSE parse) plus
//! `build_request_body` and `parse_sse_event` land in the provider layer.

use std::io::BufRead;
use std::sync::Once;

use serde_json::{Value, json};

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
        let outcome = read_sse_stream(std::io::Cursor::new(fixture), &mut |_ev| {
            seen += 1;
        })
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
}
