//! Backend identity and connection mode.
//!
//! sola-agent always attaches to a **shared Grok leader** (user systemd unit
//! `grok-leader.service`, socket `~/.grok/leader.sock`). It never spawns a
//! private agent process for the turn loop.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BackendSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub command: PathBuf,
    pub args: Vec<String>,
}

impl BackendSpec {
    /// ACP stdio **client** that attaches to the shared leader.
    ///
    /// Requires a reachable leader (`grok agent leader` / `grok-leader.service`).
    /// Does not start a private agent when the leader is already up.
    pub fn grok_leader_bridge() -> Self {
        let mut args = vec!["agent".into(), "--leader".into(), "stdio".into()];
        if let Some(sock) = leader_socket_override() {
            args.push("--leader-socket".into());
            args.push(sock.display().to_string());
        }
        Self {
            id: "grok",
            label: "Grok",
            command: resolve_grok_binary(),
            args,
        }
    }
}

/// How we attach to the agent process.
#[derive(Debug, Clone)]
pub enum ConnectionMode {
    /// Attach to `grok agent leader` via the stdio bridge (`grok agent --leader stdio`).
    Leader {
        socket: PathBuf,
        /// Thin `grok agent --leader stdio` child — ACP NDJSON on its stdio;
        /// the shared leader owns tools/sessions and outlives this process.
        bridge: BackendSpec,
    },
}

impl ConnectionMode {
    pub fn default_mode() -> Self {
        Self::Leader {
            socket: default_leader_socket(),
            bridge: BackendSpec::grok_leader_bridge(),
        }
    }

    pub fn label(&self) -> crate::protocol::ConnectionModeLabel {
        match self {
            Self::Leader { .. } => crate::protocol::ConnectionModeLabel::Leader,
        }
    }

    pub fn backend_label(&self) -> &str {
        match self {
            Self::Leader { bridge, .. } => bridge.label,
        }
    }

    pub fn socket(&self) -> &Path {
        match self {
            Self::Leader { socket, .. } => socket,
        }
    }
}

/// Default leader socket (`~/.grok/leader.sock`), honouring `GROK_LEADER_SOCKET`.
pub fn default_leader_socket() -> PathBuf {
    if let Some(p) = leader_socket_override() {
        return p;
    }
    grok_home().join("leader.sock")
}

fn leader_socket_override() -> Option<PathBuf> {
    std::env::var_os("GROK_LEADER_SOCKET")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

pub fn grok_home() -> PathBuf {
    if let Ok(h) = std::env::var("GROK_HOME") {
        return PathBuf::from(h);
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".grok"))
        .unwrap_or_else(|| PathBuf::from(".grok"))
}

/// True when something is accepting connections on the leader socket.
///
/// Used as a preflight so we do **not** spawn `grok agent --leader stdio`
/// when the leader is down (that path auto-spawns a non-systemd leader).
pub fn leader_reachable(socket: &Path) -> bool {
    use std::os::unix::net::UnixStream;
    UnixStream::connect(socket).is_ok()
}

/// Resolve the Grok CLI binary.
///
/// Prefer the **managed install symlink** `~/.grok/bin/grok` — Grok’s
/// auto-updater rewrites its target on each release. Avoid pinning a
/// versioned download path or a stale `~/.local/bin` copy.
pub fn resolve_grok_binary() -> PathBuf {
    if let Ok(p) = std::env::var("SOLA_GROK_BIN") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home {
        let managed = home.join(".grok/bin/grok");
        if managed.is_file() || managed.is_symlink() {
            return managed;
        }
        let local = home.join(".local/bin/grok");
        if local.is_file() || local.is_symlink() {
            return local;
        }
        let dl = home.join(".grok/downloads/grok-linux-x86_64");
        if dl.is_file() {
            return dl;
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let candidate = PathBuf::from(dir).join("grok");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("grok")
}
