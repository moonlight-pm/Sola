//! JSON-RPC ACP client over a `ChildTransport`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::transport::ChildTransport;
use crate::bridge;
use crate::protocol::{
    AgentEvent, EffortOption, PermissionChoice, PlanEntry, ToolTurn, Turn,
};
use crate::sessions;

pub struct AcpClient {
    transport: ChildTransport,
    next_id: u64,
    /// Pending permission request id → (json-rpc id to respond with).
    pending_permissions: HashMap<u64, u64>,
    /// Map our synthetic request_id → option list for UI.
    permission_options: HashMap<u64, Vec<PermissionChoice>>,
    session_id: Option<String>,
    /// Whether a prompt is in flight (waiting for result).
    prompt_inflight: bool,
    /// JSON-RPC id of the in-flight `session/prompt` request.
    prompt_rpc_id: Option<u64>,
    /// Accumulator while streaming a turn for tool pairing.
    open_tools: HashMap<String, usize>, // call_id → turn index
    /// Drop message/tool deltas while `session/load` is replaying history.
    suppress_history_replay: bool,
}

impl AcpClient {
    pub fn new(transport: ChildTransport) -> Self {
        Self {
            transport,
            next_id: 1,
            pending_permissions: HashMap::new(),
            permission_options: HashMap::new(),
            session_id: None,
            prompt_inflight: false,
            prompt_rpc_id: None,
            open_tools: HashMap::new(),
            suppress_history_replay: false,
        }
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn prompt_inflight(&self) -> bool {
        self.prompt_inflight
    }

    pub fn initialize(&mut self) -> Result<(), String> {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                },
                "clientInfo": {
                    "name": "sola-agent",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        )?;
        tracing::info!(?result, "ACP initialize ok");
        emit_agent_info_from_initialize(&result);
        // Some agents require an authenticated notification — ignore failures.
        let _ = self.notify("authenticated", json!({}));
        Ok(())
    }

    pub fn new_session(&mut self, cwd: &str) -> Result<String, String> {
        let result = self.request(
            "session/new",
            json!({
                "cwd": cwd,
                "mcpServers": [],
            }),
        )?;
        let id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("session/new missing sessionId: {result}"))?
            .to_string();
        self.session_id = Some(id.clone());
        self.open_tools.clear();
        self.suppress_history_replay = false;
        emit_session_config_from_result(&result);
        bridge::emit(AgentEvent::SessionReady {
            id: id.clone(),
            title: None,
        });
        bridge::emit(AgentEvent::Transcript {
            session_id: id.clone(),
            turns: Vec::new(),
            history_start_byte: 0,
            has_older: false,
            from_watch: false,
        });
        Ok(id)
    }

    pub fn load_session(&mut self, id: &str, cwd: &str) -> Result<(), String> {
        // UI already painted from disk on click. This call only attaches the
        // shared leader for prompt/permission ownership — do not gate the
        // transcript on it (session/load can take hundreds of ms+).
        self.suppress_history_replay = true;
        let _result = self.request(
            "session/load",
            json!({
                "sessionId": id,
                "cwd": cwd,
                "mcpServers": [],
            }),
        );
        self.suppress_history_replay = false;
        let result = _result?;
        self.session_id = Some(id.to_string());
        self.open_tools.clear();
        emit_session_config_from_result(&result);

        let title = sessions::title_for(cwd, id);
        bridge::emit(AgentEvent::SessionReady {
            id: id.to_string(),
            title,
        });
        // Soft re-sync from disk (live tool statuses, late writes) without
        // resetting scroll / auto-fill the way a cold replace does.
        let slice = sessions::history_tail_live(cwd, id);
        bridge::emit(AgentEvent::Transcript {
            session_id: id.to_string(),
            turns: slice.turns,
            history_start_byte: slice.start_byte,
            has_older: slice.has_older,
            from_watch: true,
        });
        Ok(())
    }

    /// ACP `session/set_mode` — permission modes and (on Grok) effort ids.
    pub fn set_mode(&mut self, mode_id: &str) -> Result<(), String> {
        let sid = self
            .session_id
            .clone()
            .ok_or_else(|| "no active session".to_string())?;
        let _ = self.request(
            "session/set_mode",
            json!({
                "sessionId": sid,
                "modeId": mode_id,
            }),
        )?;
        Ok(())
    }

    pub fn load_older_history(&mut self, id: &str, cwd: &str, before_byte: u64) {
        let slice = sessions::history_before(cwd, id, before_byte);
        bridge::emit(AgentEvent::HistoryOlder {
            session_id: id.to_string(),
            turns: slice.turns,
            history_start_byte: slice.start_byte,
            has_older: slice.has_older,
        });
    }

    /// Start a prompt without blocking; completion arrives via `poll`.
    pub fn send_prompt(&mut self, text: &str) -> Result<(), String> {
        let sid = self
            .session_id
            .clone()
            .ok_or_else(|| "no active session".to_string())?;
        if self.prompt_inflight {
            return Err("turn already in progress".into());
        }
        bridge::emit(AgentEvent::UserEcho {
            text: text.to_string(),
        });
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/prompt",
            "params": {
                "sessionId": sid,
                "prompt": [{ "type": "text", "text": text }],
            }
        });
        self.transport.write_line(&msg.to_string()).map_err(|e| e)?;
        self.prompt_inflight = true;
        self.prompt_rpc_id = Some(id);
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        let Some(sid) = self.session_id.clone() else {
            return Ok(());
        };
        self.notify(
            "session/cancel",
            json!({ "sessionId": sid }),
        )?;
        Ok(())
    }

    pub fn respond_permission(&mut self, request_id: u64, option_id: &str) -> Result<(), String> {
        let Some(rpc_id) = self.pending_permissions.remove(&request_id) else {
            return Err(format!("unknown permission request {request_id}"));
        };
        self.permission_options.remove(&request_id);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "result": {
                "outcome": {
                    "outcome": "selected",
                    "optionId": option_id,
                }
            }
        });
        self.transport.write_line(&msg.to_string())
    }

    pub fn cancel_permission(&mut self, request_id: u64) -> Result<(), String> {
        let Some(rpc_id) = self.pending_permissions.remove(&request_id) else {
            return Err(format!("unknown permission request {request_id}"));
        };
        self.permission_options.remove(&request_id);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "result": {
                "outcome": { "outcome": "cancelled" }
            }
        });
        self.transport.write_line(&msg.to_string())
    }

    /// Process available stdout without blocking long. Call from the worker
    /// idle loop so notifications arrive between commands.
    pub fn poll(&mut self, budget: Duration) -> Result<(), String> {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            match self.transport.try_read_line() {
                Some(Ok(line)) => self.handle_line(&line)?,
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.transport.write_line(&msg.to_string())?;
        self.pump_until_response(id)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.transport.write_line(&msg.to_string())
    }

    fn pump_until_response(&mut self, expect_id: u64) -> Result<Value, String> {
        let deadline = Instant::now() + Duration::from_secs(600);
        loop {
            if Instant::now() > deadline {
                return Err(format!("timeout waiting for rpc id {expect_id}"));
            }
            let line = self.transport.read_line()?;
            if let Some(result) = self.handle_line_for_response(&line, expect_id)? {
                return Ok(result);
            }
        }
    }

    fn handle_line(&mut self, line: &str) -> Result<(), String> {
        let _ = self.handle_line_for_response(line, u64::MAX)?;
        Ok(())
    }

    /// Returns Some(result) if this line was the response for `expect_id`.
    fn handle_line_for_response(
        &mut self,
        line: &str,
        expect_id: u64,
    ) -> Result<Option<Value>, String> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(None);
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("skip bad ACP line: {e}; line={}", truncate(line, 200));
                return Ok(None);
            }
        };

        // Response to our request (no method field)
        if v.get("method").is_none() {
            if let Some(id) = v.get("id").and_then(|i| i.as_u64()).or_else(|| {
                v.get("id")
                    .and_then(|i| i.as_i64())
                    .map(|i| i as u64)
            }) {
                // In-flight prompt completed (async path via poll).
                if self.prompt_rpc_id == Some(id) {
                    self.prompt_rpc_id = None;
                    self.prompt_inflight = false;
                    if let Some(err) = v.get("error") {
                        let msg = err
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("prompt error")
                            .to_string();
                        bridge::emit(AgentEvent::Error { message: msg });
                        bridge::emit(AgentEvent::TurnEnded {
                            stop_reason: "error".into(),
                        });
                    } else {
                        let stop = v
                            .pointer("/result/stopReason")
                            .and_then(|s| s.as_str())
                            .unwrap_or("end_turn")
                            .to_string();
                        bridge::emit(AgentEvent::TurnEnded {
                            stop_reason: stop,
                        });
                    }
                    if id == expect_id {
                        return Ok(Some(v.get("result").cloned().unwrap_or(Value::Null)));
                    }
                    return Ok(None);
                }

                if let Some(err) = v.get("error") {
                    if id == expect_id {
                        let msg = err
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("rpc error")
                            .to_string();
                        return Err(msg);
                    }
                    return Ok(None);
                }
                if id == expect_id {
                    return Ok(Some(v.get("result").cloned().unwrap_or(Value::Null)));
                }
                return Ok(None);
            }
        }

        // Server request (permission)
        if let Some(method) = v.get("method").and_then(|m| m.as_str()) {
            if method == "session/request_permission" {
                if let Some(rpc_id) = v.get("id").and_then(|i| i.as_u64()).or_else(|| {
                    v.get("id")
                        .and_then(|i| i.as_i64())
                        .map(|i| i as u64)
                }) {
                    self.handle_permission_request(rpc_id, v.get("params").cloned().unwrap_or(Value::Null));
                }
                return Ok(None);
            }
            if method == "session/update" {
                self.handle_session_update(v.get("params").cloned().unwrap_or(Value::Null));
                return Ok(None);
            }
            // Other server methods: fs/*, etc. — reject minimally so agent doesn't hang
            if let Some(rpc_id) = v.get("id").and_then(|i| i.as_u64()).or_else(|| {
                v.get("id")
                    .and_then(|i| i.as_i64())
                    .map(|i| i as u64)
            }) {
                tracing::debug!(%method, "rejecting unsupported agent→client request");
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": rpc_id,
                    "error": { "code": -32601, "message": format!("method not supported: {method}") }
                });
                let _ = self.transport.write_line(&resp.to_string());
            }
        }

        Ok(None)
    }

    fn handle_permission_request(&mut self, rpc_id: u64, params: Value) {
        let request_id = rpc_id; // reuse
        let tool = params
            .pointer("/toolCall/title")
            .and_then(|t| t.as_str())
            .or_else(|| params.pointer("/toolCall/kind").and_then(|t| t.as_str()))
            .unwrap_or("tool")
            .to_string();
        let preview = params
            .pointer("/toolCall/rawInput")
            .map(|v| v.to_string())
            .or_else(|| {
                params
                    .pointer("/toolCall")
                    .map(|v| truncate(&v.to_string(), 500))
            })
            .unwrap_or_default();
        let options: Vec<PermissionChoice> = params
            .get("options")
            .and_then(|o| o.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|o| {
                        Some(PermissionChoice {
                            option_id: o.get("optionId")?.as_str()?.to_string(),
                            name: o
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("Allow")
                                .to_string(),
                            kind: o
                                .get("kind")
                                .and_then(|k| k.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        self.pending_permissions.insert(request_id, rpc_id);
        self.permission_options
            .insert(request_id, options.clone());
        bridge::emit(AgentEvent::PermissionRequired {
            request_id,
            tool,
            preview,
            options,
        });
    }

    fn handle_session_update(&mut self, params: Value) {
        let update = params.get("update").cloned().unwrap_or(Value::Null);
        let kind = update
            .get("sessionUpdate")
            .and_then(|s| s.as_str())
            .unwrap_or("");

        // Grok totalTokens fallback — assume 500K context window when size absent.
        if let Some(tokens) = params
            .pointer("/_meta/totalTokens")
            .and_then(|t| t.as_u64())
            .or_else(|| update.pointer("/_meta/totalTokens").and_then(|t| t.as_u64()))
        {
            bridge::emit(AgentEvent::Usage {
                used: tokens,
                size: Some(500_000),
            });
        }

        match kind {
            "agent_message_chunk" | "agent_message" => {
                if self.suppress_history_replay {
                    return;
                }
                if let Some(text) = content_text(&update) {
                    bridge::emit(AgentEvent::AgentDelta { text });
                }
            }
            "agent_thought_chunk" | "agent_thought" => {
                if self.suppress_history_replay {
                    return;
                }
                if let Some(text) = content_text(&update) {
                    bridge::emit(AgentEvent::ThoughtDelta { text });
                }
            }
            "user_message_chunk" => {
                if self.suppress_history_replay {
                    return;
                }
                // Already echoed from UI; ignore or show if history replay
                if let Some(text) = content_text(&update) {
                    // Only emit if not our local echo during live turn — history loads use Transcript
                    if !self.prompt_inflight {
                        bridge::emit(AgentEvent::UserEcho { text });
                    }
                }
            }
            "tool_call" => {
                if self.suppress_history_replay {
                    return;
                }
                let call_id = update
                    .get("toolCallId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool = update
                    .get("title")
                    .and_then(|s| s.as_str())
                    .or_else(|| update.get("kind").and_then(|s| s.as_str()))
                    .unwrap_or("tool")
                    .to_string();
                // Skip rawInput — UI only shows collapsed "N tool uses".
                bridge::emit(AgentEvent::ToolStart { call_id, tool });
            }
            "tool_call_update" => {
                if self.suppress_history_replay {
                    return;
                }
                let call_id = update
                    .get("toolCallId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let status = update
                    .get("status")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                let title = update
                    .get("title")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                // Skip tool content/output — not rendered.
                // Grok emits both `completed` and `Completed` (and similar).
                let done = status.as_deref().is_some_and(sessions::is_terminal_tool_status);
                if done {
                    bridge::emit(AgentEvent::ToolEnd {
                        call_id,
                        status: status.unwrap_or_else(|| "completed".into()),
                    });
                } else {
                    bridge::emit(AgentEvent::ToolUpdate {
                        call_id,
                        status,
                        title,
                    });
                }
            }
            "plan" => {
                if self.suppress_history_replay {
                    return;
                }
                let entries = update
                    .get("entries")
                    .and_then(|e| e.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| {
                                Some(PlanEntry {
                                    content: e.get("content")?.as_str()?.to_string(),
                                    status: e
                                        .get("status")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("pending")
                                        .to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                bridge::emit(AgentEvent::Plan { entries });
            }
            "usage_update" => {
                let used = update
                    .get("used")
                    .and_then(|u| u.as_u64())
                    .or_else(|| update.get("totalTokens").and_then(|t| t.as_u64()))
                    .unwrap_or(0);
                let size = update
                    .get("size")
                    .and_then(|s| s.as_u64())
                    .or_else(|| update.get("contextWindow").and_then(|s| s.as_u64()))
                    .or(Some(500_000));
                bridge::emit(AgentEvent::Usage { used, size });
            }
            "session_info_update" => {
                if let Some(title) = update
                    .get("title")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                {
                    if let Some(id) = self.session_id.clone() {
                        bridge::emit(AgentEvent::SessionReady {
                            id,
                            title: Some(title),
                        });
                    }
                }
            }
            _ => {
                tracing::trace!(%kind, "unhandled session update");
            }
        }
    }
}

fn content_text(update: &Value) -> Option<String> {
    let content = update.get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(t) = content.get("text").and_then(|t| t.as_str()) {
        return Some(t.to_string());
    }
    if let Some(arr) = content.as_array() {
        let mut out = String::new();
        for c in arr {
            if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
            } else if let Some(t) = c.as_str() {
                out.push_str(t);
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn emit_agent_info_from_initialize(result: &Value) {
    let meta = result.get("_meta");
    let agent_version = meta
        .and_then(|m| m.get("agentVersion"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let model_state = meta.and_then(|m| m.get("modelState"));
    let model_id = model_state
        .and_then(|m| m.get("currentModelId"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let (efforts, current_effort) = efforts_from_models(
        model_state.and_then(|m| m.get("availableModels")),
        model_id.as_deref(),
    );
    bridge::emit(AgentEvent::AgentInfo {
        agent_version,
        model_id,
        efforts,
        current_effort,
    });
}

fn emit_session_config_from_result(result: &Value) {
    // Prefer models block; fall back to x.ai/sessionConfig options.
    let models = result.get("models");
    let model_id = models
        .and_then(|m| m.get("currentModelId"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let (mut efforts, mut current_effort) = efforts_from_models(
        models.and_then(|m| m.get("availableModels")),
        model_id.as_deref(),
    );
    if efforts.is_empty() {
        if let Some(opts) = result
            .pointer("/_meta/x.ai/sessionConfig/options")
            .and_then(|v| v.as_array())
        {
            for o in opts {
                let cat = o.get("category").and_then(|c| c.as_str()).unwrap_or("");
                if cat != "mode" {
                    continue;
                }
                let id = o
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() {
                    continue;
                }
                let label = o
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&id)
                    .to_string();
                if o.get("selected").and_then(|s| s.as_bool()) == Some(true) {
                    current_effort = Some(id.clone());
                }
                efforts.push(EffortOption { id, label });
            }
        }
    }
    if !efforts.is_empty() || model_id.is_some() {
        bridge::emit(AgentEvent::SessionConfig {
            efforts,
            current_effort,
            model_id,
        });
    }
}

fn efforts_from_models(
    available: Option<&Value>,
    prefer_model: Option<&str>,
) -> (Vec<EffortOption>, Option<String>) {
    let Some(arr) = available.and_then(|v| v.as_array()) else {
        return (Vec::new(), None);
    };
    let model = arr
        .iter()
        .find(|m| {
            prefer_model.is_some_and(|id| m.get("modelId").and_then(|v| v.as_str()) == Some(id))
        })
        .or_else(|| arr.first());
    let Some(model) = model else {
        return (Vec::new(), None);
    };
    let meta = model.get("_meta");
    let current = meta
        .and_then(|m| m.get("reasoningEffort"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut efforts = Vec::new();
    if let Some(list) = meta.and_then(|m| m.get("reasoningEfforts")).and_then(|v| v.as_array()) {
        for e in list {
            let id = e
                .get("id")
                .or_else(|| e.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            let label = e
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or(&id)
                .to_string();
            efforts.push(EffortOption { id, label });
        }
    }
    (efforts, current)
}

/// Helpers used when mapping history — re-export turn builders.
#[allow(dead_code)]
pub fn empty_tool(call_id: String, tool: String) -> ToolTurn {
    ToolTurn {
        call_id,
        tool,
        status: String::new(),
    }
}

#[allow(dead_code)]
pub fn turns_demo() -> Vec<Turn> {
    Vec::new()
}
