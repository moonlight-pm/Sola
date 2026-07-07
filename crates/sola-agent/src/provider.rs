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
