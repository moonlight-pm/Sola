//! Backend identity and connection mode.
//!
//! v1 wires only Grok over `StdioChild`. `Leader` is reserved for the future
//! daemon attach path documented in the design.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BackendSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub command: PathBuf,
    pub args: Vec<String>,
}

impl BackendSpec {
    /// Default Grok Build ACP stdio backend.
    pub fn grok() -> Self {
        Self {
            id: "grok",
            label: "Grok",
            command: resolve_grok_binary(),
            args: vec!["agent".into(), "stdio".into()],
        }
    }
}

/// How we attach to the agent process.
#[derive(Debug, Clone)]
pub enum ConnectionMode {
    /// v1: private child for the app lifetime. Quit stops the agent.
    StdioChild { spec: BackendSpec },
    /// Future: attach to `grok agent leader` at this socket.
    #[allow(dead_code)]
    Leader { socket: PathBuf },
}

impl ConnectionMode {
    pub fn v1_default() -> Self {
        Self::StdioChild {
            spec: BackendSpec::grok(),
        }
    }

    pub fn label(&self) -> crate::protocol::ConnectionModeLabel {
        match self {
            Self::StdioChild { .. } => crate::protocol::ConnectionModeLabel::Local,
            Self::Leader { .. } => crate::protocol::ConnectionModeLabel::Leader,
        }
    }

    pub fn backend_label(&self) -> &str {
        match self {
            Self::StdioChild { spec } => spec.label,
            Self::Leader { .. } => "Grok (leader)",
        }
    }
}

fn resolve_grok_binary() -> PathBuf {
    if let Ok(p) = std::env::var("SOLA_GROK_BIN") {
        return PathBuf::from(p);
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let candidate = PathBuf::from(dir).join("grok");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home {
        let local = home.join(".local/bin/grok");
        if local.is_file() {
            return local;
        }
        let grok_home = home.join(".grok/bin/grok");
        if grok_home.is_file() {
            return grok_home;
        }
    }
    PathBuf::from("grok")
}
