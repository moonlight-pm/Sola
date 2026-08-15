//! `sat` protocol. Unix socket to the running app. Fail if it is down.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const SOCK_NAME: &str = "sola-at-cli.sock";

pub fn socket_path() -> PathBuf {
    sola_core::env::runtime_dir().join(SOCK_NAME)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Request {
    Ps,
    ProjectList,
    WorkspaceList {
        #[serde(default)]
        project: Option<String>,
    },
    WorkspaceSpawn {
        project: String,
        name: String,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        prompt: Option<String>,
        #[serde(default)]
        parent: Option<String>,
    },
    WorkspaceRm {
        workspace: String,
    },
    PaneList {
        #[serde(default)]
        workspace: Option<String>,
    },
    PaneSend {
        #[serde(default)]
        pane: Option<String>,
        text: String,
        #[serde(default)]
        enter: bool,
    },
    PaneRead {
        #[serde(default)]
        pane: Option<String>,
        #[serde(default)]
        lines: Option<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl Response {
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            error: None,
            data: Some(data),
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
            data: None,
        }
    }
}

/// Connect, write one JSON request, read one JSON response.
/// Errors if the app is not running — never launches a window.
pub fn call(req: &Request) -> Result<Response, String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|_| {
        "Workspaces is not running (sat does not launch it)".to_string()
    })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let body = serde_json::to_vec(req).map_err(|e| e.to_string())?;
    stream.write_all(&body).map_err(|e| e.to_string())?;
    stream.write_all(b"\n").map_err(|e| e.to_string())?;
    let _ = stream.flush();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.is_empty() {
        return Err("app returned no reply".into());
    }
    serde_json::from_slice(&buf).map_err(|e| format!("bad reply: {e}"))
}

/// Text form of `sat ps` — the sidebar as a scan table.
pub fn format_ps(data: &serde_json::Value) -> String {
    let Some(projects) = data.get("projects").and_then(|v| v.as_array()) else {
        return String::new();
    };
    let mut out = String::new();
    for proj in projects {
        let name = proj.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        out.push_str(name);
        out.push('\n');
        let Some(wss) = proj.get("workspaces").and_then(|v| v.as_array()) else {
            continue;
        };
        for ws in wss {
            let title = ws.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let status = ws.get("status").and_then(|v| v.as_str()).unwrap_or("idle");
            let agent = ws.get("agent").and_then(|v| v.as_str()).unwrap_or("");
            let mark = if ws.get("selected").and_then(|v| v.as_bool()) == Some(true) {
                "*"
            } else {
                " "
            };
            if agent.is_empty() {
                out.push_str(&format!("  {mark} {title:<16} {status}\n"));
            } else {
                out.push_str(&format!("  {mark} {title:<16} {status:<8} {agent}\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let req = Request::WorkspaceSpawn {
            project: "sola".into(),
            name: "kvm-perf".into(),
            agent: Some("grok".into()),
            prompt: Some("fix it".into()),
            parent: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["op"], "workspace-spawn");
        let back: Request = serde_json::from_value(v).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn format_ps_is_scannable() {
        let data = serde_json::json!({
            "projects": [{
                "name": "Sola",
                "workspaces": [
                    {"name": "root", "status": "idle", "selected": true},
                    {"name": "kvm-perf", "status": "working", "agent": "grok", "selected": false}
                ]
            }]
        });
        let text = format_ps(&data);
        assert!(text.contains("Sola"));
        assert!(text.contains("root"));
        assert!(text.contains("working"));
        assert!(text.contains("grok"));
        assert!(text.contains('*'));
    }
}
