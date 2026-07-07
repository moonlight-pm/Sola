//! Sakana Fugu Responses client + the small test seam.
//!
//! Foundation defines the shared wire types and the `LlmStream` trait.
//! The real `SakanaProvider` impl (ureq streaming, SSE parse) plus
//! `build_request_body` and `parse_sse_event` land in the provider layer.

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
