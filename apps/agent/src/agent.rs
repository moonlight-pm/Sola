//! Agent: spawns `claude -p` subprocess per turn with stream-json I/O.
//!
//! We manage conversation history ourselves in JSONL files.
//! Each turn, we replay the full history as stream-json input so the
//! subprocess has full context without relying on Claude's session persistence.

use crate::session::{SessionManager, SessionStatus};
use crate::storage;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// Start a new agent subprocess and send a message.
pub async fn run_session_message(
    session_id: String,
    text: String,
    working_dir: PathBuf,
    session_mgr: Arc<SessionManager>,
    event_tx: std::sync::mpsc::Sender<String>,
    cancel_token: tokio_util::sync::CancellationToken,
    model: String,
    effort: String,
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
        &session_mgr, &event_tx, cancel_token, &model, &effort,
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

fn resolve_claude_bin() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let local = PathBuf::from(&home).join(".local/bin/claude");
    if local.exists() { local } else { PathBuf::from("claude") }
}

async fn run_claude(
    session_id: &str,
    text: &str,
    working_dir: &PathBuf,
    session_name: Option<&str>,
    session_mgr: &SessionManager,
    event_tx: &std::sync::mpsc::Sender<String>,
    cancel_token: tokio_util::sync::CancellationToken,
    model: &str,
    effort: &str,
) -> Result<()> {
    // Load existing conversation history.
    let mut history = storage::load_history(session_id).unwrap_or_default();

    // Append the new user message and persist it.
    let user_msg = json!({
        "role": "user",
        "content": [{"type": "text", "text": text}]
    });
    history.push(user_msg.clone());
    let _ = storage::append_message(session_id, &user_msg);

    // Build stream-json input: replay full conversation so Claude has context.
    // Merge consecutive same-role messages (e.g. tool_result user + next user
    // prompt) into a single message — the API requires strict alternation.
    let mut merged: Vec<serde_json::Value> = Vec::new();
    for msg in &history {
        let role = msg["role"].as_str().unwrap_or("user");
        let content = msg.get("content").cloned().unwrap_or(json!([]));
        let blocks: Vec<serde_json::Value> = if let Some(arr) = content.as_array() {
            arr.clone()
        } else if let Some(s) = content.as_str() {
            vec![json!({"type": "text", "text": s})]
        } else {
            vec![]
        };

        // If the last merged message has the same role, append blocks to it.
        let should_merge = merged.last()
            .map(|m| m["role"].as_str() == Some(role))
            .unwrap_or(false);

        if should_merge {
            let last = merged.last_mut().unwrap();
            if let Some(arr) = last["content"].as_array_mut() {
                arr.extend(blocks);
            }
        } else {
            merged.push(json!({"role": role, "content": blocks}));
        }
    }

    let mut input_lines = String::new();
    for msg in &merged {
        let msg_type = if msg["role"] == "user" { "user" } else { "assistant" };
        let stream_msg = json!({
            "type": msg_type,
            "message": msg
        });
        input_lines.push_str(&stream_msg.to_string());
        input_lines.push('\n');
    }

    let claude_bin = resolve_claude_bin();
    let mut child = tokio::process::Command::new(&claude_bin)
        .arg("-p")
        .arg("--verbose")
        .arg("--no-session-persistence")
        .arg("--output-format").arg("stream-json")
        .arg("--input-format").arg("stream-json")
        .arg("--include-partial-messages")
        .arg("--dangerously-skip-permissions")
        .arg("--model").arg(model)
        .arg("--effort").arg(effort)
        .current_dir(working_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn claude CLI")?;

    // Write full history to stdin, then keep it open for follow-ups.
    let stdin = child.stdin.take().context("No stdin")?;
    let stdin = Arc::new(Mutex::new(stdin));

    {
        let mut stdin_guard = stdin.lock().await;
        stdin_guard.write_all(input_lines.as_bytes()).await?;
        stdin_guard.flush().await?;
    }

    session_mgr.set_stdin(session_id, Some(stdin.clone())).await;

    // Capture stderr in background.
    let stderr = child.stderr.take().context("No stderr")?;
    let stderr_sid = session_id.to_string();
    let stderr_tx = event_tx.clone();
    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr);
        use tokio::io::AsyncReadExt;
        let _ = reader.read_to_string(&mut buf).await;
        if !buf.trim().is_empty() {
            tracing::warn!(session_id = %stderr_sid, "claude stderr: {}", buf.trim());
            send_event(&stderr_tx, json!({
                "event": "error", "session_id": stderr_sid,
                "message": buf.trim()
            }));
        }
        buf
    });

    // Read and process stdout.
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
        let mut tool_results: Vec<serde_json::Value> = Vec::new();
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

                // Tool result from Claude CLI — a synthetic "user" message with
                // tool_result content. Save it so the API sees the required
                // tool_result after each tool_use on replay.
                "user" => {
                    if let Some(content) = parsed.get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        let has_tool_result = content.iter().any(|b|
                            b.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                        );
                        if has_tool_result {
                            tool_results.extend(content.iter().cloned());

                            // Also emit tool_end events to the frontend.
                            for block in content {
                                if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                                    let tool_use_id = block.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                                    // Find the matching tool_use name from our blocks.
                                    let name = blocks.iter()
                                        .find(|b| b.get("id").and_then(|v| v.as_str()) == Some(tool_use_id))
                                        .and_then(|b| b.get("name").and_then(|v| v.as_str()))
                                        .unwrap_or("unknown");
                                    let output = block.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                    let is_error = block.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                                    send_event(&tx, json!({
                                        "event": "tool_end", "session_id": sid,
                                        "tool_name": name,
                                        "result": &output[..output.len().min(2000)],
                                        "is_error": is_error
                                    }));
                                }
                            }
                        }
                    }
                }

                "tool_use_summary" => {
                    // We now get tool results from the "user" tool_result messages
                    // above, but tool_use_summary still fires. Use it as a fallback
                    // for the frontend event if we didn't already emit one.
                }

                "result" => {
                    result_metrics = Some(parsed.clone());
                    done_signal_reader.notify_one();
                    break;
                }

                _ => {}
            }
        }

        (text_acc, blocks, tool_results, result_metrics)
    });

    // Wait for completion, done signal, or cancellation.
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

    // Close stdin so subprocess can exit.
    {
        let mut stdin_guard = stdin.lock().await;
        let _ = stdin_guard.shutdown().await;
    }

    let (acc_text, acc_blocks, acc_tool_results, result_metrics) = read_task.await
        .unwrap_or_else(|_| (String::new(), Vec::new(), Vec::new(), None));

    // Don't block on stderr — let it finish in the background.
    // It'll log warnings and emit error events if anything shows up.
    tokio::spawn(stderr_task);

    send_event(event_tx, json!({
        "event": "message_end", "session_id": session_id,
        "cancelled": was_cancelled
    }));

    // Send metrics and persist.
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

    // Save assistant response to our JSONL for display on reload.
    if !was_cancelled && (!acc_text.is_empty() || !acc_blocks.is_empty()) {
        let content = if acc_blocks.is_empty() {
            json!([{"type": "text", "text": acc_text}])
        } else {
            json!(acc_blocks)
        };
        let assistant_msg = json!({"role": "assistant", "content": content});
        let _ = storage::append_message(session_id, &assistant_msg);

        // Save tool results as a user message so the API sees the required
        // tool_result blocks when we replay history on the next turn.
        if !acc_tool_results.is_empty() {
            let tool_result_msg = json!({"role": "user", "content": acc_tool_results});
            let _ = storage::append_message(session_id, &tool_result_msg);
        }
    }

    // Update metadata with latest metrics.
    if let Err(e) = storage::save_meta(session_id, session_name, &cwd_str, saved_metrics) {
        tracing::warn!("Failed to save session metadata: {:#}", e);
    }

    Ok(())
}
