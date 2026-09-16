//! Thin ACP client over `grok agent stdio`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use tracing::{info, warn};

use crate::paths;

pub struct AcpSession {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    tx: mpsc::Sender<Outgoing>,
    inbox: Mutex<mpsc::Receiver<Incoming>>,
    next_id: Arc<Mutex<u64>>,
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Value, String>>>>>,
}

enum Outgoing {
    Rpc(Value),
}

enum Incoming {
    Response { id: u64, result: Result<Value, String> },
    Notify { method: String, params: Value },
    Request { id: Value, method: String, params: Value },
}

#[derive(Debug, Clone)]
pub struct TurnEvent {
    pub text_delta: Option<String>,
    pub done: bool,
    pub error: Option<String>,
}

impl AcpSession {
    pub fn spawn(cwd: &Path, model: &str) -> anyhow::Result<Self> {
        let grok = paths::grok_bin();
        let mut cmd = Command::new(&grok);
        // Default Grok 4.6 + low effort ("fast"). No `auto` model/effort
        // on this CLI; omit `-m` so the agent's default model is used.
        let _ = model;
        let debug_file = cwd.join("grok.acp.log");
        cmd.args([
            "--no-auto-update",
            "--cwd",
            &cwd.display().to_string(),
            "--always-approve",
            "--effort",
            "low",
            "--debug-file",
            &debug_file.display().to_string(),
            "--rules",
            "Never use solactl compositor screenshot or input, ydotool, xdotool, wtype, or any OS mouse/keyboard. Never tab.focus or tab.open --select. Always pass --tab <id> to solactl browser. Snapshot is the page description and works on background tabs. Do not poll solactl bots list.",
            "--deny",
            "Bash(solactl compositor screenshot*)",
            "--deny",
            "Bash(solactl compositor input*)",
            "--deny",
            "Bash(ydotool *)",
            "--deny",
            "Bash(xdotool *)",
            "--deny",
            "Bash(wtype *)",
            "agent",
            "--no-leader",
            "stdio",
        ])
        .env("SOLA_BOT", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!("spawn grok ({}) failed: {e}", grok.display())
        })?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
        let stderr = child.stderr.take();
        if let Some(stderr) = stderr {
            thread::Builder::new()
                .name("bots-grok-err".into())
                .spawn(move || {
                    let r = BufReader::new(stderr);
                    for line in r.lines().map_while(Result::ok) {
                        if !line.is_empty() {
                            warn!("grok stderr: {line}");
                        }
                    }
                })?;
        }

        let stdin = Arc::new(Mutex::new(stdin));
        let (out_tx, out_rx) = mpsc::channel::<Outgoing>();
        let (in_tx, in_rx) = mpsc::channel::<Incoming>();
        let pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Value, String>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(Mutex::new(1u64));

        {
            let stdin = Arc::clone(&stdin);
            thread::Builder::new()
                .name("bots-acp-w".into())
                .spawn(move || {
                    while let Ok(Outgoing::Rpc(v)) = out_rx.recv() {
                        if write_rpc(&mut *stdin.lock().unwrap(), &v).is_err() {
                            break;
                        }
                    }
                })?;
        }

        {
            let in_tx = in_tx.clone();
            thread::Builder::new()
                .name("bots-acp-r".into())
                .spawn(move || {
                    if let Err(e) = read_loop(stdout, in_tx) {
                        warn!("acp read: {e}");
                    }
                })?;
        }

        let sess = Self {
            child: Mutex::new(child),
            stdin,
            tx: out_tx,
            inbox: Mutex::new(in_rx),
            next_id,
            pending,
        };
        sess.handshake(cwd)?;
        Ok(sess)
    }

    fn handshake(&self, cwd: &Path) -> anyhow::Result<()> {
        let init = self.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientInfo": { "name": "sola-bots", "version": "0.1.0" },
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "_meta": { "headless": true }
                }
            }),
            Duration::from_secs(30),
        )?;
        let method_id = init
            .pointer("/_meta/defaultAuthMethodId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                init.get("authMethods").and_then(|v| v.as_array()).and_then(|methods| {
                    methods
                        .iter()
                        .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
                        .find(|id| *id == "cached_token")
                        .or_else(|| {
                            methods
                                .iter()
                                .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
                                .next()
                        })
                        .map(|s| s.to_string())
                })
            });
        if let Some(id) = method_id {
            info!(auth = %id, "acp authenticate");
            self.request(
                "authenticate",
                json!({ "methodId": id, "_meta": { "headless": true } }),
                Duration::from_secs(30),
            )?;
        }
        let _ = cwd;
        Ok(())
    }

    pub fn new_session(&self, cwd: &Path) -> anyhow::Result<String> {
        let v = self.request(
            "session/new",
            json!({
                "cwd": cwd.display().to_string(),
                "mcpServers": [],
                "_meta": { "reasoningEffort": "low" }
            }),
            Duration::from_secs(30),
        )?;
        v.get("sessionId")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("session/new missing sessionId: {v}"))
    }

    pub fn load_session(&self, id: &str) -> anyhow::Result<()> {
        self.request(
            "session/load",
            json!({ "sessionId": id }),
            Duration::from_secs(30),
        )?;
        Ok(())
    }

    pub fn child_exited(&self) -> bool {
        matches!(
            self.child.lock().ok().and_then(|mut c| c.try_wait().ok()),
            Some(Some(_))
        )
    }

    pub fn cancel(&self, session_id: &str) {
        let _ = self.tx.send(Outgoing::Rpc(json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": session_id }
        })));
    }

    pub fn prompt(
        &self,
        session_id: &str,
        text: &str,
        cancel: impl Fn() -> bool,
        mut on_event: impl FnMut(TurnEvent),
    ) -> anyhow::Result<String> {
        let id = self.begin_request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }),
        )?;
        let mut acc = String::new();
        loop {
            if cancel() {
                self.cancel(session_id);
                return Err(anyhow::anyhow!("cancelled"));
            }
            let msg = {
                let inbox = self.inbox.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
                inbox.recv_timeout(Duration::from_millis(200))
            };
            match msg {
                Ok(Incoming::Notify { method, params }) => {
                    if method == "session/update" || method.ends_with("session/update") {
                        if let Some(delta) = extract_text(&params) {
                            acc.push_str(&delta);
                            on_event(TurnEvent {
                                text_delta: Some(delta),
                                done: false,
                                error: None,
                            });
                        }
                    }
                }
                Ok(Incoming::Request { id, method, params }) => {
                    self.answer_rpc(id, &method, &params);
                }
                Ok(Incoming::Response {
                    id: rid,
                    result,
                }) if rid == id => {
                    if let Err(e) = result {
                        on_event(TurnEvent {
                            text_delta: None,
                            done: true,
                            error: Some(e.clone()),
                        });
                        return Err(anyhow::anyhow!(e));
                    }
                    on_event(TurnEvent {
                        text_delta: None,
                        done: true,
                        error: None,
                    });
                    return Ok(acc);
                }
                Ok(Incoming::Response { id: rid, result }) => {
                    if let Some(tx) = self.pending.lock().unwrap().remove(&rid) {
                        let _ = tx.send(result);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.child_exited() {
                        return Err(anyhow::anyhow!("acp disconnected"));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(anyhow::anyhow!("acp disconnected"));
                }
            }
        }
    }

    fn answer_rpc(&self, id: Value, method: &str, params: &Value) {
        if !method.contains("permission") {
            info!(%method, "acp server request");
            let _ = self.tx.send(Outgoing::Rpc(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            })));
            return;
        }
        let option_id = params
            .get("options")
            .and_then(|o| o.as_array())
            .and_then(|a| {
                a.iter().find_map(|o| {
                    let kind = o.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                    if kind.contains("allow") {
                        o.get("optionId").and_then(|i| i.as_str())
                    } else {
                        None
                    }
                })
            })
            .or_else(|| {
                params
                    .pointer("/options/0/optionId")
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("allow-always");
        let _ = self.tx.send(Outgoing::Rpc(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "outcome": { "outcome": "selected", "optionId": option_id }
            }
        })));
    }

    fn request(&self, method: &str, params: Value, timeout: Duration) -> anyhow::Result<Value> {
        let id = self.begin_request(method, params)?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err(anyhow::anyhow!("{method} timed out"));
            }
            let msg = {
                let inbox = self.inbox.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
                inbox.recv_timeout(left.min(Duration::from_millis(200)))
            };
            match msg {
                Ok(Incoming::Response {
                    id: rid,
                    result,
                }) if rid == id => {
                    return result.map_err(anyhow::Error::msg);
                }
                Ok(Incoming::Response { id: rid, result }) => {
                    if let Some(tx) = self.pending.lock().unwrap().remove(&rid) {
                        let _ = tx.send(result);
                    }
                }
                Ok(Incoming::Request { id, method, params }) => {
                    self.answer_rpc(id, &method, &params);
                }
                Ok(Incoming::Notify { .. }) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(anyhow::anyhow!("acp disconnected"));
                }
            }
        }
    }

    fn begin_request(&self, method: &str, params: Value) -> anyhow::Result<u64> {
        let id = {
            let mut n = self.next_id.lock().unwrap();
            let id = *n;
            *n += 1;
            id
        };
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.tx
            .send(Outgoing::Rpc(msg))
            .map_err(|_| anyhow::anyhow!("acp writer gone"))?;
        Ok(id)
    }
}

impl Drop for AcpSession {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = self.stdin;
        info!("acp child stopped");
    }
}

fn extract_text(params: &Value) -> Option<String> {
    let u = params.get("update").or_else(|| params.get("sessionUpdate"))?;
    let kind = u
        .get("sessionUpdate")
        .or_else(|| u.get("kind"))
        .and_then(|k| k.as_str())
        .unwrap_or("");
    let kind_l = kind.to_ascii_lowercase();
    if kind_l.contains("thought") || kind_l.contains("reason") {
        return None;
    }
    if kind_l.contains("agent_message") {
        if let Some(c) = u.get("content") {
            return content_text(c);
        }
        if let Some(t) = u.get("text").and_then(|t| t.as_str()) {
            return Some(t.to_string());
        }
    }
    None
}

fn content_text(c: &Value) -> Option<String> {
    if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
        return Some(t.to_string());
    }
    if let Some(arr) = c.as_array() {
        let mut s = String::new();
        for part in arr {
            if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                    s.push_str(t);
                }
            }
        }
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

fn write_rpc(w: &mut ChildStdin, v: &Value) -> std::io::Result<()> {
    // Grok 1.0 ACP stdio is newline-delimited JSON, not LSP Content-Length.
    let mut body = serde_json::to_vec(v)?;
    body.push(b'\n');
    w.write_all(&body)?;
    w.flush()
}

fn read_loop(stdout: impl Read, tx: mpsc::Sender<Incoming>) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stdout);
    loop {
        let msg = match read_one(&mut reader)? {
            Some(v) => v,
            None => break,
        };
        let parsed = classify(msg);
        if tx.send(parsed).is_err() {
            break;
        }
    }
    Ok(())
}

fn rpc_id(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn classify(msg: Value) -> Incoming {
    if let Some(id) = msg.get("id").and_then(rpc_id) {
        if msg.get("method").is_some() {
            return Incoming::Request {
                id: msg.get("id").cloned().unwrap_or(Value::Null),
                method: msg
                    .get("method")
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .into(),
                params: msg.get("params").cloned().unwrap_or(Value::Null),
            };
        }
        if let Some(err) = msg.get("error") {
            let text = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("error")
                .to_string();
            return Incoming::Response {
                id,
                result: Err(text),
            };
        }
        return Incoming::Response {
            id,
            result: Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
        };
    }
    if msg.get("method").is_some() && msg.get("id").is_some() {
        return Incoming::Request {
            id: msg.get("id").cloned().unwrap_or(Value::Null),
            method: msg
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .into(),
            params: msg.get("params").cloned().unwrap_or(Value::Null),
        };
    }
    Incoming::Notify {
        method: msg
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .into(),
        params: msg.get("params").cloned().unwrap_or(Value::Null),
    }
}

fn read_one(reader: &mut BufReader<impl Read>) -> anyhow::Result<Option<Value>> {
    let mut header = String::new();
    let mut content_len: Option<usize> = None;
    loop {
        header.clear();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Ok(None);
        }
        let line = header.trim_end();
        if line.is_empty() {
            if let Some(len) = content_len {
                let mut buf = vec![0u8; len];
                reader.read_exact(&mut buf)?;
                return Ok(Some(serde_json::from_slice(&buf)?));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_len = rest.trim().parse().ok();
            continue;
        }
        if line.starts_with('{') {
            match serde_json::from_str(line) {
                Ok(v) => return Ok(Some(v)),
                Err(e) => {
                    warn!("skip bad acp json: {e}");
                    continue;
                }
            }
        }
        warn!("skip acp stdout: {line}");
    }
}
