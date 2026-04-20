//! Claude Code session manager — spawns long-lived `claude` processes with
//! stream-json I/O. Adapted from Cogsworth's claude_session.rs.
//!
//! Key design: the Claude process stays alive across turns. Messages are sent
//! via stdin NDJSON. The CLI manages its own session state; we maintain a
//! separate display JSONL for our frontend.

use crate::storage;
use anyhow::{Context, Result};
use serde_json::json;
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

/// A running Claude Code subprocess.
pub struct ClaudeProcess {
    stdin: ChildStdin,
    pid: u32,
}

/// Manages Claude CLI subprocesses — one per session.
pub struct ClaudeProcessManager {
    processes: std::collections::HashMap<String, ClaudeProcess>,
}

impl ClaudeProcessManager {
    pub fn new() -> Self {
        Self {
            processes: std::collections::HashMap::new(),
        }
    }

    /// Spawn a new Claude process for a session, or resume an existing one.
    /// Returns immediately; stdout is read in a background thread.
    pub fn start(
        &mut self,
        session_id: &str,
        working_dir: &str,
        model: &str,
        effort: &str,
        event_tx: std::sync::mpsc::Sender<String>,
    ) -> Result<()> {
        if self.processes.contains_key(session_id) {
            return Ok(()); // already running
        }

        let claude_path = resolve_claude_bin()?;

        // The plain aliases "opus" / "sonnet" map to the 200k-context
        // variants; appending "[1m]" selects the 1M-context ones. Do this
        // here so every agent turn runs with the larger window (which is
        // also what `claude` defaults to for interactive use).
        let model_arg = if model.contains('[') { model.to_string() } else { format!("{model}[1m]") };

        let mut cmd = Command::new(&claude_path);
        cmd.arg("--output-format").arg("stream-json")
            .arg("--input-format").arg("stream-json")
            .arg("--verbose")
            .arg("--include-partial-messages")
            .arg("--replay-user-messages")
            .arg("--dangerously-skip-permissions")
            .arg("--model").arg(&model_arg)
            .arg("--effort").arg(effort);

        // Resume if a CLI session already exists for this ID.
        if crate::sync::cli_session_exists(session_id) {
            cmd.arg("--resume").arg(session_id);
        } else {
            cmd.arg("--session-id").arg(session_id);
        }

        cmd.current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().context("Failed to spawn claude CLI")?;
        let pid = child.id();

        let stdin = child.stdin.take().context("No stdin")?;
        let stdout = child.stdout.take().context("No stdout")?;

        tracing::info!(session_id, pid, "started claude process");

        self.processes.insert(session_id.to_string(), ClaudeProcess { stdin, pid });

        // Background stdout reader — relays events to the frontend.
        let sid = session_id.to_string();
        start_stdout_reader(stdout, child, sid, event_tx);

        Ok(())
    }

    /// Send a user message to an active session via stdin.
    pub fn send_message(&mut self, session_id: &str, text: &str) -> Result<()> {
        let proc = self.processes.get_mut(session_id)
            .context("No running process for this session")?;

        let msg = json!({
            "type": "user",
            "session_id": "",
            "message": {
                "role": "user",
                "content": [{"type": "text", "text": text}],
            },
            "parent_tool_use_id": null
        });

        let line = serde_json::to_string(&msg)?;
        proc.stdin.write_all(line.as_bytes()).context("stdin write")?;
        proc.stdin.write_all(b"\n").context("stdin newline")?;
        proc.stdin.flush().context("stdin flush")?;

        tracing::debug!(session_id, "sent message via stdin");
        Ok(())
    }

    /// Check if a session has a live process.
    pub fn is_running(&self, session_id: &str) -> bool {
        self.processes.contains_key(session_id)
    }

    /// Remove a dead process entry (called when we detect exit).
    pub fn remove(&mut self, session_id: &str) {
        self.processes.remove(session_id);
    }

    /// Interrupt the current turn via SIGINT.
    pub fn interrupt(&self, session_id: &str) -> Result<()> {
        let proc = self.processes.get(session_id)
            .context("No running process")?;
        unsafe { libc::kill(proc.pid as i32, libc::SIGINT); }
        tracing::debug!(session_id, "sent SIGINT");
        Ok(())
    }
}

impl Drop for ClaudeProcessManager {
    fn drop(&mut self) {
        for (handle, proc) in &self.processes {
            unsafe { libc::kill(proc.pid as i32, libc::SIGTERM); }
            tracing::debug!(handle, "killed claude process on drop");
        }
    }
}

fn resolve_claude_bin() -> Result<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        format!("{home}/.local/bin/claude"),
        "/usr/local/bin/claude".to_string(),
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    anyhow::bail!("claude binary not found")
}


/// Background thread reads stdout NDJSON and forwards as events.
/// Adapted from Cogsworth's start_stdout_reader.
fn start_stdout_reader(
    stdout: std::process::ChildStdout,
    mut child: Child,
    session_id: String,
    event_tx: std::sync::mpsc::Sender<String>,
) {
    // Blocking reader thread → std mpsc → tokio task → event_tx
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();

    let sid_for_thread = session_id.clone();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    if line_tx.send(line).is_err() { break; }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(session_id = %sid_for_thread, "stdout error: {e}");
                    break;
                }
            }
        }
        let exit_code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        tracing::info!(session_id = %sid_for_thread, exit_code, "claude process exited");
        let _ = line_tx.send(json!({"__exit": true, "code": exit_code}).to_string());
    });

    let sid_for_task = session_id.clone();
    tokio::spawn(async move {
        let mut text_acc = String::new();
        let mut blocks: Vec<serde_json::Value> = Vec::new();
        let mut in_turn = false;

        loop {
            let batch: Vec<String> = tokio::task::block_in_place(|| {
                let mut batch = Vec::new();
                match line_rx.recv() {
                    Ok(line) => batch.push(line),
                    Err(_) => return batch,
                }
                while batch.len() < 50 {
                    match line_rx.try_recv() {
                        Ok(line) => batch.push(line),
                        Err(_) => break,
                    }
                }
                batch
            });

            if batch.is_empty() { break; }

            for line in &batch {
                let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };

                // Exit marker
                if parsed.get("__exit").is_some() {
                    send_event(&event_tx, json!({
                        "event": "session_exit", "session_id": sid_for_task,
                        "code": parsed.get("code").cloned().unwrap_or(json!(-1))
                    }));
                    return;
                }

                let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match msg_type {
                    "system" => {
                        // Init event — contains session_id from CLI
                        if parsed.get("subtype").and_then(|v| v.as_str()) == Some("init") {
                            send_event(&event_tx, json!({
                                "event": "session_init", "session_id": sid_for_task,
                                "cli_session_id": parsed.get("session_id").cloned().unwrap_or(json!(null))
                            }));
                        }
                    }

                    "stream_event" => {
                        if !in_turn {
                            in_turn = true;
                            text_acc.clear();
                            blocks.clear();
                            send_event(&event_tx, json!({
                                "event": "message_start", "session_id": sid_for_task
                            }));
                        }
                        if let Some(event) = parsed.get("event") {
                            let etype = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            if etype == "content_block_delta" {
                                if let Some(delta) = event.get("delta") {
                                    if delta.get("type").and_then(|v| v.as_str()) == Some("text_delta") {
                                        if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                            text_acc.push_str(text);
                                            send_event(&event_tx, json!({
                                                "event": "message_delta",
                                                "session_id": sid_for_task,
                                                "text": text
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    "assistant" => {
                        if !in_turn {
                            in_turn = true;
                            text_acc.clear();
                            blocks.clear();
                            send_event(&event_tx, json!({
                                "event": "message_start", "session_id": sid_for_task
                            }));
                        }
                        if let Some(content) = parsed.get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                        {
                            for block in content {
                                let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                if btype == "tool_use" {
                                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                                    let input = block.get("input").map(|v| v.to_string()).unwrap_or_default();
                                    send_event(&event_tx, json!({
                                        "event": "tool_start",
                                        "session_id": sid_for_task,
                                        "tool_name": name,
                                        "tool_input": input
                                    }));
                                }
                                blocks.push(block.clone());
                            }
                        }
                    }

                    "user" => {
                        // Two shapes arrive on this channel:
                        //   1. tool_result blocks (plumbing — we convert to tool_end events)
                        //   2. replayed user messages (via --replay-user-messages) —
                        //      the CLI echoes each submitted user prompt back to stdout
                        //      when it accepts it. For mid-stream injections that's the
                        //      point the frontend should insert the user bubble.
                        if let Some(content) = parsed.get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                        {
                            // Collect any text content — if present, this is a replay echo.
                            let text: String = content.iter()
                                .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                                .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                                .collect::<Vec<_>>()
                                .join("");
                            if !text.is_empty() {
                                send_event(&event_tx, json!({
                                    "event": "user_appended",
                                    "session_id": sid_for_task,
                                    "text": text,
                                }));
                            }

                            for block in content {
                                if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                                    let tool_use_id = block.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                                    let name = blocks.iter()
                                        .find(|b| b.get("id").and_then(|v| v.as_str()) == Some(tool_use_id))
                                        .and_then(|b| b.get("name").and_then(|v| v.as_str()))
                                        .unwrap_or("unknown");
                                    let output = block.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                    let is_error = block.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                                    send_event(&event_tx, json!({
                                        "event": "tool_end",
                                        "session_id": sid_for_task,
                                        "tool_name": name,
                                        "result": &output[..output.len().min(2000)],
                                        "is_error": is_error
                                    }));
                                }
                            }
                        }
                    }

                    "result" => {
                        // Turn complete. Save to display JSONL and emit metrics.
                        if !text_acc.is_empty() || !blocks.is_empty() {
                            let content = if blocks.is_empty() {
                                json!([{"type": "text", "text": text_acc}])
                            } else {
                                // Filter thinking blocks from display
                                let display_blocks: Vec<_> = blocks.iter()
                                    .filter(|b| b.get("type").and_then(|v| v.as_str()) != Some("thinking"))
                                    .cloned()
                                    .collect();
                                json!(display_blocks)
                            };
                            let assistant_msg = json!({"role": "assistant", "content": content});
                            let _ = storage::append_message(&sid_for_task, &assistant_msg);
                        }

                        // Emit metrics
                        let mut metrics = json!({"event": "metrics", "session_id": sid_for_task});
                        if let Some(usage) = parsed.get("usage") {
                            metrics["input_tokens"] = usage.get("input_tokens").cloned().unwrap_or(json!(0));
                            metrics["output_tokens"] = usage.get("output_tokens").cloned().unwrap_or(json!(0));
                            metrics["cache_read_tokens"] = usage.get("cache_read_input_tokens").cloned().unwrap_or(json!(0));
                            metrics["cache_creation_tokens"] = usage.get("cache_creation_input_tokens").cloned().unwrap_or(json!(0));
                        }
                        if let Some(model_usage) = parsed.get("modelUsage") {
                            if let Some(model_data) = model_usage.as_object().and_then(|m| m.values().next()) {
                                metrics["context_window"] = model_data.get("contextWindow").cloned().unwrap_or(json!(0));
                            }
                        }
                        metrics["total_cost_usd"] = parsed.get("total_cost_usd").cloned().unwrap_or(json!(0.0));
                        metrics["duration_ms"] = parsed.get("duration_ms").cloned().unwrap_or(json!(0));
                        metrics["num_turns"] = parsed.get("num_turns").cloned().unwrap_or(json!(0));
                        metrics["model"] = parsed.get("modelUsage")
                            .and_then(|m| m.as_object())
                            .and_then(|m| m.keys().next())
                            .map(|k| json!(k))
                            .unwrap_or(json!("unknown"));

                        let total = metrics["input_tokens"].as_u64().unwrap_or(0)
                            + metrics["cache_read_tokens"].as_u64().unwrap_or(0)
                            + metrics["cache_creation_tokens"].as_u64().unwrap_or(0)
                            + metrics["output_tokens"].as_u64().unwrap_or(0);
                        let ctx = metrics["context_window"].as_u64().unwrap_or(1);
                        metrics["context_used_pct"] = json!((total as f64 / ctx as f64 * 100.0).round() as u64);

                        send_event(&event_tx, metrics.clone());

                        // Save metrics to our meta file, preserving everything
                        // else (name — which the user may have customized —
                        // working_dir, model, effort, cli_synced_at).
                        if let Ok(mut meta) = storage::load_meta(&sid_for_task) {
                            meta.metrics = Some(metrics);
                            meta.updated_at = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            let _ = storage::save_meta_full(&meta);
                        }

                        send_event(&event_tx, json!({
                            "event": "message_end", "session_id": sid_for_task,
                            "cancelled": false
                        }));

                        // Emit idle state
                        send_event(&event_tx, json!({
                            "event": "session_state", "session_id": sid_for_task,
                            "status": "idle"
                        }));

                        in_turn = false;
                        text_acc.clear();
                        blocks.clear();
                    }

                    _ => {}
                }
            }
        }
    });
}

fn send_event(tx: &std::sync::mpsc::Sender<String>, value: serde_json::Value) {
    let _ = tx.send(value.to_string());
}
