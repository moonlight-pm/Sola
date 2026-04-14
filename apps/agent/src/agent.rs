//! Agent: spawns `claude -p` subprocess with stream-json I/O.
//!
//! Stdin stays open so follow-up messages can be injected mid-response.
//! Full conversation history is managed by us.

use crate::session::{SessionManager, SessionStatus};
use crate::storage;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// Start a new agent subprocess and send the first message.
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

    let session_name = {
        let sessions = session_mgr.sessions.read().await;
        sessions.get(&session_id).and_then(|s| s.name.clone())
    };

    match run_claude(
        &session_id, &text, &working_dir, session_name.as_deref(),
        &session_mgr, &event_tx, cancel_token,
    ).await {
        Ok(()) => {
            session_mgr.set_stdin(&session_id, None).await;
            session_mgr.set_status(&session_id, SessionStatus::Idle).await;
            send_event(&event_tx, json!({
                "event": "session_state", "session_id": session_id, "status": "idle"
            }));
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            tracing::error!("Agent error for session {}: {}", session_id, msg);
            session_mgr.set_stdin(&session_id, None).await;
            session_mgr
                .set_status(&session_id, SessionStatus::Error(msg.clone()))
                .await;
            send_event(&event_tx, json!({
                "event": "error", "session_id": session_id, "message": msg
            }));
        }
    }
}

/// Send a follow-up message to an already-running subprocess.
pub async fn send_followup(
    session_id: &str,
    text: &str,
    session_mgr: &SessionManager,
) -> Result<()> {
    let stdin = session_mgr.get_stdin(session_id).await
        .context("No running subprocess for this session")?;

    let msg = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": text}]
        }
    });

    let mut stdin_guard = stdin.lock().await;
    stdin_guard.write_all(msg.to_string().as_bytes()).await?;
    stdin_guard.write_all(b"\n").await?;
    stdin_guard.flush().await?;

    tracing::info!(session_id, "Injected follow-up message");
    Ok(())
}

fn send_event(tx: &std::sync::mpsc::Sender<String>, value: serde_json::Value) {
    let _ = tx.send(value.to_string());
}

async fn run_claude(
    session_id: &str,
    text: &str,
    working_dir: &PathBuf,
    session_name: Option<&str>,
    session_mgr: &SessionManager,
    event_tx: &std::sync::mpsc::Sender<String>,
    cancel_token: tokio_util::sync::CancellationToken,
) -> Result<()> {
    // Load existing conversation or start fresh
    let mut history = storage::load_history(session_id).unwrap_or_default();

    // Append user message and persist it
    let user_msg = json!({
        "role": "user",
        "content": [{"type": "text", "text": text}]
    });
    history.push(user_msg.clone());
    let _ = storage::append_message(session_id, &user_msg);

    // Build stream-json input from history
    let mut input_lines = String::new();
    for msg in &history {
        let stream_msg = json!({
            "type": if msg["role"] == "user" { "user" } else { "assistant" },
            "message": {
                "role": msg["role"],
                "content": msg["content"]
            }
        });
        input_lines.push_str(&stream_msg.to_string());
        input_lines.push('\n');
    }

    // Spawn claude subprocess
    let mut child = tokio::process::Command::new("claude")
        .arg("-p")
        .arg("--verbose")
        .arg("--no-session-persistence")
        .arg("--output-format").arg("stream-json")
        .arg("--input-format").arg("stream-json")
        .arg("--include-partial-messages")
        .arg("--dangerously-skip-permissions")
        .current_dir(working_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to spawn claude CLI")?;

    // Write history to stdin but keep it open for follow-ups
    let stdin = child.stdin.take().context("No stdin")?;
    let stdin = Arc::new(Mutex::new(stdin));

    {
        let mut stdin_guard = stdin.lock().await;
        stdin_guard.write_all(input_lines.as_bytes()).await?;
        stdin_guard.flush().await?;
    }

    // Store stdin handle in session so handler can inject follow-ups
    session_mgr.set_stdin(session_id, Some(stdin.clone())).await;

    // Read and process stdout
    let stdout = child.stdout.take().context("No stdout")?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let sid = session_id.to_string();
    let tx = event_tx.clone();

    send_event(event_tx, json!({"event": "message_start", "session_id": session_id}));

    let done_signal = Arc::new(tokio::sync::Notify::new());
    let done_signal_reader = done_signal.clone();
    let read_task = tokio::spawn(async move {
        let mut text_acc = String::new();
        let mut blocks: Vec<serde_json::Value> = Vec::new();
        let mut result_metrics: Option<serde_json::Value> = None;

        while let Ok(Some(line)) = lines.next_line().await {
            if line.is_empty() { continue; }
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&line) else { continue };

            let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
            tracing::debug!("stream-json: type={}", msg_type);

            match msg_type {
                "stream_event" => {
                    if let Some(event) = parsed.get("event") {
                        let etype = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if etype == "content_block_delta" {
                            if let Some(delta) = event.get("delta") {
                                if delta.get("type").and_then(|v| v.as_str()) == Some("text_delta") {
                                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                        text_acc.push_str(text);
                                        send_event(&tx, json!({
                                            "event": "message_delta", "session_id": sid, "text": text
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }

                "assistant" => {
                    if let Some(content) = parsed.get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for block in content {
                            let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            if btype == "tool_use" {
                                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                                let input = block.get("input").map(|v| v.to_string()).unwrap_or_default();
                                send_event(&tx, json!({
                                    "event": "tool_start", "session_id": sid,
                                    "tool_name": name, "tool_input": input
                                }));
                            }
                            blocks.push(block.clone());
                        }
                    }
                }

                "tool_use_summary" => {
                    let name = parsed.get("tool_name").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let output = parsed.get("output").and_then(|v| v.as_str())
                        .or_else(|| parsed.get("result").and_then(|v| v.as_str()))
                        .unwrap_or("").to_string();
                    let is_error = parsed.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                    send_event(&tx, json!({
                        "event": "tool_end", "session_id": sid,
                        "tool_name": name, "result": &output[..output.len().min(2000)],
                        "is_error": is_error
                    }));
                }

                "result" => {
                    result_metrics = Some(parsed.clone());
                    done_signal_reader.notify_one();
                    break;
                }

                _ => {}
            }
        }

        (text_acc, blocks, result_metrics)
    });

    // Wait for completion, done signal, or cancellation
    let was_cancelled = tokio::select! {
        _ = cancel_token.cancelled() => {
            tracing::info!("Session {} cancelled", session_id);
            true
        }
        _ = done_signal.notified() => {
            tracing::debug!("Session {} got result signal", session_id);
            false
        }
        _ = child.wait() => {
            tracing::debug!("Session {} subprocess exited", session_id);
            false
        }
    };

    // Close stdin and wait for subprocess to exit
    tracing::debug!("Closing stdin...");
    {
        let mut stdin_guard = stdin.lock().await;
        tracing::debug!("Got stdin lock, shutting down...");
        let _ = stdin_guard.shutdown().await;
    }
    tracing::debug!("Stdin closed, waiting for child...");
    let _ = child.wait().await;
    tracing::debug!("Child exited, waiting for read_task...");

    let (acc_text, acc_blocks, result_metrics) = read_task.await
        .unwrap_or_else(|_| (String::new(), Vec::new(), None));
    tracing::debug!("Read task complete, acc_text len={}, blocks={}", acc_text.len(), acc_blocks.len());

    send_event(event_tx, json!({
        "event": "message_end", "session_id": session_id,
        "cancelled": was_cancelled
    }));

    // Send metrics and save for persistence
    let mut saved_metrics: Option<serde_json::Value> = None;
    if let Some(ref result) = result_metrics {
        let mut metrics = json!({"event": "metrics", "session_id": session_id});

        if let Some(usage) = result.get("usage") {
            metrics["input_tokens"] = usage.get("input_tokens").cloned().unwrap_or(json!(0));
            metrics["output_tokens"] = usage.get("output_tokens").cloned().unwrap_or(json!(0));
            metrics["cache_read_tokens"] = usage.get("cache_read_input_tokens").cloned().unwrap_or(json!(0));
            metrics["cache_creation_tokens"] = usage.get("cache_creation_input_tokens").cloned().unwrap_or(json!(0));
        }

        if let Some(model_usage) = result.get("modelUsage") {
            if let Some(model_data) = model_usage.as_object().and_then(|m| m.values().next()) {
                metrics["context_window"] = model_data.get("contextWindow").cloned().unwrap_or(json!(0));
                metrics["max_output_tokens"] = model_data.get("maxOutputTokens").cloned().unwrap_or(json!(0));
            }
        }

        metrics["total_cost_usd"] = result.get("total_cost_usd").cloned().unwrap_or(json!(0.0));
        metrics["duration_ms"] = result.get("duration_ms").cloned().unwrap_or(json!(0));
        metrics["duration_api_ms"] = result.get("duration_api_ms").cloned().unwrap_or(json!(0));
        metrics["num_turns"] = result.get("num_turns").cloned().unwrap_or(json!(0));
        metrics["model"] = result.get("modelUsage")
            .and_then(|m| m.as_object())
            .and_then(|m| m.keys().next())
            .map(|k| json!(k))
            .unwrap_or(json!("unknown"));

        let total_input = metrics["input_tokens"].as_u64().unwrap_or(0)
            + metrics["cache_read_tokens"].as_u64().unwrap_or(0)
            + metrics["cache_creation_tokens"].as_u64().unwrap_or(0)
            + metrics["output_tokens"].as_u64().unwrap_or(0);
        let context_window = metrics["context_window"].as_u64().unwrap_or(1);
        metrics["context_used_pct"] = json!((total_input as f64 / context_window as f64 * 100.0).round() as u64);

        send_event(event_tx, metrics.clone());
        saved_metrics = Some(metrics);
    }

    let cwd_str = working_dir.to_string_lossy().to_string();

    if was_cancelled {
        // TODO: truncate the JSONL to remove the last user message
        // For now, the user message is already in the JSONL — we'd need to rewrite it.
        // This is acceptable for now; the model will see the unanswered message on resume.
    } else if !acc_text.is_empty() || !acc_blocks.is_empty() {
        let content = if acc_blocks.is_empty() {
            json!([{"type": "text", "text": acc_text}])
        } else {
            json!(acc_blocks)
        };
        let assistant_msg = json!({"role": "assistant", "content": content});
        let _ = storage::append_message(session_id, &assistant_msg);
    }

    // Update metadata with latest metrics
    if let Err(e) = storage::save_meta(session_id, session_name, &cwd_str, saved_metrics) {
        tracing::warn!("Failed to save session metadata: {:#}", e);
    }

    Ok(())
}
