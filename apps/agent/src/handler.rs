use std::sync::Arc;

use serde_json::{json, Value};

use crate::agent;
use crate::session::SessionManager;
use crate::storage;

pub struct AgentHandler {
    pub session_mgr: Arc<SessionManager>,
    pub event_tx: std::sync::mpsc::Sender<String>,
    pub process_mgr: Arc<tokio::sync::Mutex<agent::ClaudeProcessManager>>,
}

#[async_trait::async_trait]
impl sola_app::AppHandler for AgentHandler {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value {
        match cmd {
            "new_session" => self.cmd_new_session(args).await,
            "send_message" => self.cmd_send_message(args).await,
            "cancel" => self.cmd_cancel(args).await,
            "close_session" => self.cmd_close_session(args).await,
            "delete_session" => self.cmd_delete_session(args).await,
            "list_mcps" => self.cmd_list_mcps(args).await,
            "rename_conversation" => self.cmd_rename(args).await,
            "list_conversations" => self.cmd_list_conversations().await,
            "resume_session" => self.cmd_resume_session(args).await,
            "update_session_config" => self.cmd_update_session_config(args).await,
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        }
    }
}

impl AgentHandler {
    async fn cmd_new_session(&self, args: &Value) -> Value {
        let Some(working_dir) = args.get("working_dir").and_then(|v| v.as_str()) else {
            return json!({ "error": "working_dir is required" });
        };

        let expanded = if working_dir.starts_with("~/") {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{}/{}", home, &working_dir[2..])
        } else if working_dir == "~" {
            std::env::var("HOME").unwrap_or_default()
        } else {
            working_dir.to_string()
        };
        let dir = std::path::PathBuf::from(&expanded);
        if !dir.is_dir() {
            return json!({ "error": format!("Not a directory: {}", working_dir) });
        }

        let folder_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| expanded.clone());
        let session_id = self.session_mgr.create_session(dir).await;
        self.session_mgr
            .rename_session(&session_id, folder_name.clone())
            .await;

        // Persist immediately so the session survives app restart
        if let Err(e) = storage::save_meta(&session_id, Some(&folder_name), &expanded, None) {
            tracing::warn!("Failed to save new session: {:#}", e);
        }

        self.send_event(json!({
            "event": "session_state",
            "session_id": session_id,
            "status": "idle",
            "name": folder_name,
            "working_dir": expanded
        }));

        json!({ "session_id": session_id })
    }

    async fn cmd_send_message(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
            return json!({ "error": "text is required" });
        };
        let model = args.get("model").and_then(|v| v.as_str()).unwrap_or("opus");
        let effort = args.get("effort").and_then(|v| v.as_str()).unwrap_or("high");

        let working_dir = {
            let sessions = self.session_mgr.sessions.read().await;
            match sessions.get(session_id) {
                Some(s) => s.working_dir.to_string_lossy().to_string(),
                None => return json!({ "error": "Session not found" }),
            }
        };

        // Save user message to our display JSONL.
        let user_msg = json!({
            "role": "user",
            "content": [{"type": "text", "text": text}]
        });
        let _ = storage::append_message(session_id, &user_msg);

        // Ensure a Claude process is running for this session.
        {
            let mut mgr = self.process_mgr.lock().await;
            if !mgr.is_running(session_id) {
                if let Err(e) = mgr.start(session_id, &working_dir, model, effort, self.event_tx.clone()) {
                    return json!({ "error": format!("Failed to start claude: {e:#}") });
                }
            }
        }

        // Send the message via stdin.
        {
            let mut mgr = self.process_mgr.lock().await;
            if let Err(e) = mgr.send_message(session_id, text) {
                // Process might have died; remove it so next attempt respawns.
                mgr.remove(session_id);
                return json!({ "error": format!("Failed to send message: {e:#}") });
            }
        }

        // Emit running state.
        self.send_event(json!({
            "event": "session_state", "session_id": session_id, "status": "running"
        }));

        json!({ "ok": true })
    }

    async fn cmd_cancel(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        let mgr = self.process_mgr.lock().await;
        let _ = mgr.interrupt(session_id);
        json!({ "ok": true })
    }

    async fn cmd_close_session(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        self.session_mgr.close_session(session_id).await;
        json!({ "ok": true })
    }

    async fn cmd_delete_session(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        self.session_mgr.close_session(session_id).await;
        if let Err(e) = storage::delete_session(session_id) {
            tracing::warn!(session_id, "failed to delete session files: {:#}", e);
        }
        json!({ "ok": true })
    }

    async fn cmd_list_mcps(&self, args: &Value) -> Value {
        let working_dir = args.get("working_dir").and_then(|v| v.as_str()).unwrap_or(".");
        let claude_bin = {
            let home = std::env::var("HOME").unwrap_or_default();
            let local = std::path::PathBuf::from(&home).join(".local/bin/claude");
            if local.exists() { local } else { std::path::PathBuf::from("claude") }
        };
        let output = match tokio::process::Command::new(&claude_bin)
            .args(["mcp", "list"])
            .current_dir(working_dir)
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => return json!({ "error": format!("failed to run claude mcp list: {e}") }),
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let mut servers = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("Checking") { continue; }
            // Format: "name: command - status"
            let (name, rest) = match line.split_once(':') {
                Some(pair) => pair,
                None => continue,
            };
            let name = name.trim();
            let (command, status) = match rest.rsplit_once(" - ") {
                Some(pair) => (pair.0.trim(), pair.1.trim()),
                None => (rest.trim(), ""),
            };
            if name == "claude.ai Google Drive" { continue; }
            let connected = status.contains("Connected");
            let needs_auth = status.contains("authentication");
            servers.push(json!({
                "name": name,
                "command": command,
                "status": if connected { "connected" } else if needs_auth { "auth" } else { "error" },
            }));
        }
        json!({ "servers": servers })
    }

    async fn cmd_rename(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
            return json!({ "error": "name is required" });
        };
        self.session_mgr
            .rename_session(session_id, name.to_string())
            .await;

        // Update saved metadata with new name
        if let Ok(meta) = storage::load_meta(session_id) {
            let _ = storage::save_meta(session_id, Some(name), &meta.working_dir, meta.metrics);
        }

        json!({ "ok": true })
    }

    async fn cmd_update_session_config(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        if let Ok(mut meta) = storage::load_meta(session_id) {
            if let Some(model) = args.get("model").and_then(|v| v.as_str()) {
                meta.model = model.to_string();
            }
            if let Some(effort) = args.get("effort").and_then(|v| v.as_str()) {
                meta.effort = effort.to_string();
            }
            let _ = storage::save_meta_full(&meta);
        }
        json!({ "ok": true })
    }

    async fn cmd_list_conversations(&self) -> Value {
        // Return whatever view models are already on disk. Sync runs in
        // the background on startup (see main.rs) and streams updates via
        // the `session_updated` event channel.
        let metas = storage::list_all();
        let active = crate::active::detect();
        tracing::info!(count = metas.len(), active = active.len(), "list_conversations");
        let conversations: Vec<Value> = metas
            .iter()
            .map(|m| {
                // Peek at first user message from JSONL for the preview
                let first_prompt = storage::load_history(&m.session_id).ok()
                    .and_then(|msgs| {
                        msgs.iter()
                            .find(|msg| msg.get("role").and_then(|v| v.as_str()) == Some("user"))
                            .and_then(|msg| extract_text_content(msg).into())
                    })
                    .unwrap_or_default();
                json!({
                    "session_id": &m.session_id,
                    "name": &m.name,
                    "first_prompt": first_prompt,
                    "working_dir": &m.working_dir,
                    "updated_at": m.updated_at,
                    "metrics": &m.metrics,
                    "model": &m.model,
                    "effort": &m.effort,
                    "active": active.contains(&m.session_id),
                })
            })
            .collect();
        // Return the conversations directly in the invoke reply; the frontend
        // reads them from the promise result rather than going through a
        // separate event channel.
        json!({ "conversations": conversations })
    }

    async fn cmd_resume_session(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };

        let meta = match storage::load_meta(session_id) {
            Ok(m) => m,
            Err(e) => return json!({ "error": format!("Failed to load session: {:#}", e) }),
        };

        let dir = std::path::PathBuf::from(&meta.working_dir);
        self.session_mgr.sessions.write().await.insert(
            session_id.to_string(),
            crate::session::Session::new(dir),
        );
        if let Some(ref name) = meta.name {
            self.session_mgr.rename_session(session_id, name.clone()).await;
        }

        self.send_event(json!({
            "event": "session_state",
            "session_id": session_id,
            "status": "idle",
            "name": meta.name,
            "working_dir": meta.working_dir,
        }));

        // Load and forward conversation history with full content blocks
        // (including tool_use, tool_result, thinking) so the frontend can
        // reconstruct the interleaved presentation.
        let history = storage::load_history(session_id).unwrap_or_default();
        let messages: Vec<Value> = history.iter().map(|m: &Value| {
            let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            json!({ "role": role, "content": m.get("content").cloned().unwrap_or(json!("")) })
        }).collect();

        self.send_event(json!({
            "event": "session_loaded",
            "session_id": session_id,
            "messages": messages,
        }));

        json!({ "ok": true })
    }

    fn send_event(&self, value: Value) {
        let _ = self.event_tx.send(value.to_string());
    }
}

fn extract_text_content(msg: &Value) -> String {
    if let Some(content) = msg.get("content") {
        if let Some(text) = content.as_str() {
            return text.to_string();
        }
        if let Some(blocks) = content.as_array() {
            return blocks.iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                        b.get("text").and_then(|v| v.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    String::new()
}
