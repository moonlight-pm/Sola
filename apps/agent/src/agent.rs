//! Agent loop: send message → stream response → execute tools → repeat.
//!
//! Modeled after Pi's event-driven agent loop pattern.
//! Persists conversation history to disk after each turn.

use crate::api::{ApiClient, ContentBlock, Message, MessageContent, StreamEvent};
use crate::session::{SessionManager, SessionStatus};
use crate::{storage, tools};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub async fn run_session_message(
    session_id: String,
    text: String,
    working_dir: PathBuf,
    session_mgr: Arc<SessionManager>,
    event_tx: std::sync::mpsc::Sender<String>,
    cancel_token: tokio_util::sync::CancellationToken,
) {
    session_mgr
        .set_status(&session_id, SessionStatus::Running)
        .await;
    send_event(&event_tx, json!({
        "event": "session_state", "session_id": session_id, "status": "running"
    }));

    // Get session name for storage
    let session_name = {
        let sessions = session_mgr.sessions.read().await;
        sessions.get(&session_id).and_then(|s| s.name.clone())
    };

    match run_agent_loop(
        &session_id,
        &text,
        &working_dir,
        session_name.as_deref(),
        &event_tx,
        cancel_token,
    )
    .await
    {
        Ok(()) => {
            session_mgr.set_status(&session_id, SessionStatus::Idle).await;
            send_event(&event_tx, json!({
                "event": "session_state", "session_id": session_id, "status": "idle"
            }));
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            tracing::error!("Agent error for session {}: {}", session_id, msg);
            session_mgr
                .set_status(&session_id, SessionStatus::Error(msg.clone()))
                .await;
            send_event(&event_tx, json!({
                "event": "error", "session_id": session_id, "message": msg
            }));
        }
    }
}

fn send_event(tx: &std::sync::mpsc::Sender<String>, value: serde_json::Value) {
    let _ = tx.send(value.to_string());
}

async fn run_agent_loop(
    session_id: &str,
    text: &str,
    working_dir: &PathBuf,
    session_name: Option<&str>,
    event_tx: &std::sync::mpsc::Sender<String>,
    cancel_token: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    let client = ApiClient::from_env();
    let tool_defs = tools::tool_definitions();

    let system_prompt = format!(
        "You are a coding assistant running inside Sola, a Wayland desktop environment. \
         Your working directory is: {}\n\n\
         You have access to tools for bash execution, file operations, search, and web search. \
         Be concise and direct. When editing code, show the changes clearly.",
        working_dir.display()
    );

    // Load existing conversation history or start fresh
    let mut messages: Vec<Message> = match storage::load(session_id) {
        Ok(saved) => saved.messages,
        Err(_) => Vec::new(),
    };

    // Append the new user message
    messages.push(Message {
        role: "user".into(),
        content: MessageContent::Text(text.to_string()),
    });

    let cwd_str = working_dir.to_string_lossy().to_string();
    let max_turns = 30;

    for turn in 0..max_turns {
        if cancel_token.is_cancelled() {
            tracing::info!("Session {} cancelled at turn {}", session_id, turn);
            save_quietly(session_id, session_name, &cwd_str, &messages);
            return Ok(());
        }

        tracing::info!(session_id, turn, "Sending API request");

        send_event(event_tx, json!({"event": "message_start", "session_id": session_id}));

        // Stream the response
        let (stream_tx, mut stream_rx) =
            tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

        let sid = session_id.to_string();
        let bridge_tx = event_tx.clone();

        let forwarder = tokio::spawn(async move {
            while let Some(event) = stream_rx.recv().await {
                match &event {
                    StreamEvent::TextDelta(text) => {
                        send_event(&bridge_tx, json!({
                            "event": "message_delta", "session_id": sid, "text": text
                        }));
                    }
                    StreamEvent::ToolUseStart { name, .. } => {
                        send_event(&bridge_tx, json!({
                            "event": "tool_start", "session_id": sid,
                            "tool_name": name, "tool_input": ""
                        }));
                    }
                    _ => {}
                }
            }
        });

        let content_blocks = client
            .stream_message(&messages, Some(&system_prompt), &tool_defs, &stream_tx)
            .await?;

        drop(stream_tx);
        let _ = forwarder.await;

        send_event(event_tx, json!({"event": "message_end", "session_id": session_id}));

        // Add assistant message to history
        messages.push(Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(content_blocks.clone()),
        });

        // Save after each assistant response
        save_quietly(session_id, session_name, &cwd_str, &messages);

        // Check for tool calls
        let tool_uses: Vec<&ContentBlock> = content_blocks
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .collect();

        if tool_uses.is_empty() {
            tracing::info!(session_id, turn, "Agent completed (no tool calls)");
            return Ok(());
        }

        // Execute tools
        let mut tool_results: Vec<ContentBlock> = Vec::new();
        for block in &tool_uses {
            if let ContentBlock::ToolUse { id, name, input } = block {
                tracing::info!(session_id, tool = %name, "Executing tool");

                let (output, is_error) = tools::execute_tool(name, input, working_dir).await;

                send_event(event_tx, json!({
                    "event": "tool_end", "session_id": session_id,
                    "tool_name": name, "result": &output[..output.len().min(2000)],
                    "is_error": is_error
                }));

                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: output,
                    is_error: if is_error { Some(true) } else { None },
                });
            }
        }

        // Add tool results as user message
        messages.push(Message {
            role: "user".into(),
            content: MessageContent::Blocks(tool_results),
        });

        // Save after tool results too
        save_quietly(session_id, session_name, &cwd_str, &messages);
    }

    tracing::warn!(session_id, "Reached max turns ({})", max_turns);
    Ok(())
}

fn save_quietly(session_id: &str, name: Option<&str>, working_dir: &str, messages: &[Message]) {
    if let Err(e) = storage::save(session_id, name, working_dir, messages) {
        tracing::warn!(session_id, "Failed to save session: {:#}", e);
    }
}
