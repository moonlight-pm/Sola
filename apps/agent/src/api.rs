//! Streaming Anthropic Messages API client.
//!
//! Sends requests to the Messages API and yields parsed SSE events.
//! Designed to work through Junction (or any Anthropic-compatible proxy).

use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ── Types ────────────────────────────────────────────────────────────────────

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, #[serde(skip_serializing_if = "Option::is_none")] is_error: Option<bool> },
}

/// A tool definition sent to the API.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Events emitted during streaming.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    ToolUseStart { id: String, name: String },
    ToolInputDelta(String),
    ToolUseEnd,
    MessageEnd { stop_reason: String },
}

// ── Client ───────────────────────────────────────────────────────────────────

pub struct ApiClient {
    http: reqwest::Client,
    api_base: String,
    api_key: String,
    model: String,
}

impl ApiClient {
    pub fn new(api_base: String, api_key: String, model: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_base,
            api_key,
            model,
        }
    }

    pub fn from_env() -> Self {
        let api_key = std::env::var("JUNCTION_API_KEY")
            .unwrap_or_else(|_| "a626d79941d9c85a4f7015b3b69805f0df659406f1701ccd3aa07bb7123f50a3".into());
        let api_base = std::env::var("JUNCTION_URL")
            .unwrap_or_else(|_| "https://junction.moonlight.pm".into());
        let model = std::env::var("SOLA_AGENT_MODEL")
            .unwrap_or_else(|_| "claude-opus-4-6".into());
        Self::new(api_base, api_key, model)
    }

    /// Stream a message to the API. Sends events to `event_tx`.
    /// Returns the full assistant message (text + tool_use blocks) when done.
    pub async fn stream_message(
        &self,
        messages: &[Message],
        system: Option<&str>,
        tools: &[ToolDef],
        event_tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<Vec<ContentBlock>> {
        let mut body = json!({
            "model": self.model,
            "max_tokens": 16384,
            "stream": true,
            "messages": messages,
        });

        if let Some(sys) = system {
            body["system"] = json!(sys);
        }
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
        }

        let resp = self.http
            .post(format!("{}/v1/messages", self.api_base))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .body(body.to_string())
            .send()
            .await
            .context("API request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("API error {}: {}", status, text);
        }

        // Parse SSE stream
        let mut content_blocks: Vec<ContentBlock> = Vec::new();
        let mut current_text = String::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_input = String::new();
        let mut event_type = String::new();

        let mut byte_stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = byte_stream.next().await {
            let chunk = chunk.context("Stream read error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete lines
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.starts_with("event:") {
                    event_type = line[6..].trim().to_string();
                } else if line.starts_with("data:") {
                    let data = line[5..].trim();
                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        self.handle_sse_data(
                            &event_type,
                            &parsed,
                            &mut content_blocks,
                            &mut current_text,
                            &mut current_tool_id,
                            &mut current_tool_name,
                            &mut current_tool_input,
                            event_tx,
                        );
                    }
                }
                // blank line or ":" comment — ignore
            }
        }

        // Flush any remaining text
        if !current_text.is_empty() {
            content_blocks.push(ContentBlock::Text { text: current_text });
        }

        Ok(content_blocks)
    }

    fn handle_sse_data(
        &self,
        event_type: &str,
        data: &Value,
        content_blocks: &mut Vec<ContentBlock>,
        current_text: &mut String,
        current_tool_id: &mut String,
        current_tool_name: &mut String,
        current_tool_input: &mut String,
        event_tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) {
        // Use event_type from SSE, fall back to "type" field in data
        let etype = if event_type.is_empty() {
            data.get("type").and_then(|v| v.as_str()).unwrap_or("")
        } else {
            event_type
        };

        match etype {
            "content_block_start" => {
                if let Some(block) = data.get("content_block") {
                    match block.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {}
                        Some("tool_use") => {
                            // Flush accumulated text
                            if !current_text.is_empty() {
                                content_blocks.push(ContentBlock::Text {
                                    text: std::mem::take(current_text),
                                });
                            }
                            *current_tool_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            *current_tool_name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            current_tool_input.clear();
                            let _ = event_tx.send(StreamEvent::ToolUseStart {
                                id: current_tool_id.clone(),
                                name: current_tool_name.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }

            "content_block_delta" => {
                if let Some(delta) = data.get("delta") {
                    match delta.get("type").and_then(|v| v.as_str()) {
                        Some("text_delta") => {
                            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                current_text.push_str(text);
                                let _ = event_tx.send(StreamEvent::TextDelta(text.to_string()));
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(json) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                current_tool_input.push_str(json);
                                let _ = event_tx.send(StreamEvent::ToolInputDelta(json.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }

            "content_block_stop" => {
                if !current_tool_name.is_empty() {
                    let input: Value = serde_json::from_str(current_tool_input)
                        .unwrap_or(Value::Object(Default::default()));
                    content_blocks.push(ContentBlock::ToolUse {
                        id: std::mem::take(current_tool_id),
                        name: std::mem::take(current_tool_name),
                        input,
                    });
                    current_tool_input.clear();
                    let _ = event_tx.send(StreamEvent::ToolUseEnd);
                }
            }

            "message_delta" => {
                if let Some(delta) = data.get("delta") {
                    let stop_reason = delta.get("stop_reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("end_turn")
                        .to_string();
                    let _ = event_tx.send(StreamEvent::MessageEnd { stop_reason });
                }
            }

            "message_stop" => {
                // Final signal — stream is done
            }

            _ => {} // ping, message_start, etc. — ignore
        }
    }
}
