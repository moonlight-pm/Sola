//! ACP worker thread: owns the child connection and applies UI commands.

use std::time::Duration;

use crate::acp::{AcpClient, ChildTransport};
use crate::backend::{BackendSpec, ConnectionMode};
use crate::bridge;
use crate::protocol::{AgentCmd, AgentEvent, ConnectionModeLabel};
use crate::sessions;

pub fn start(mode: ConnectionMode) {
    std::thread::Builder::new()
        .name("sola-agent-worker".into())
        .spawn(move || run(mode))
        .expect("spawn agent worker");
}

fn run(mode: ConnectionMode) {
    let cmd_rx = bridge::take_cmd_rx();
    let mut client: Option<AcpClient> = None;

    loop {
        // Drain commands with a short wait so we can poll the child.
        let cmd = match cmd_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(c) => Some(c),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if let Some(cmd) = cmd {
            match cmd {
                AgentCmd::Shutdown => {
                    client = None;
                    break;
                }
                AgentCmd::Restart | AgentCmd::EnsureConnected => {
                    client = None;
                    client = connect(&mode);
                }
                AgentCmd::NewSession { cwd } => {
                    if client.is_none() {
                        client = connect(&mode);
                    }
                    if let Some(c) = client.as_mut() {
                        match c.new_session(&cwd) {
                            Ok(id) => {
                                crate::overlay::note_opened(&id, &cwd);
                                refresh_sessions(&cwd);
                            }
                            Err(e) => bridge::emit(AgentEvent::Error { message: e }),
                        }
                    }
                }
                AgentCmd::LoadSession { id, cwd } => {
                    if client.is_none() {
                        client = connect(&mode);
                    }
                    if let Some(c) = client.as_mut() {
                        match c.load_session(&id, &cwd) {
                            Ok(()) => {
                                crate::overlay::note_opened(&id, &cwd);
                                refresh_sessions(&cwd);
                            }
                            Err(e) => bridge::emit(AgentEvent::Error { message: e }),
                        }
                    }
                }
                AgentCmd::LoadOlderHistory {
                    id,
                    cwd,
                    before_byte,
                } => {
                    if let Some(c) = client.as_mut() {
                        c.load_older_history(&id, &cwd, before_byte);
                    } else {
                        // File-only path — no live child required.
                        let slice = sessions::history_before(&cwd, &id, before_byte);
                        bridge::emit(AgentEvent::HistoryOlder {
                            turns: slice.turns,
                            history_start_byte: slice.start_byte,
                            has_older: slice.has_older,
                        });
                    }
                }
                AgentCmd::Send { text } => {
                    if client.is_none() {
                        client = connect(&mode);
                    }
                    if let Some(c) = client.as_mut() {
                        if c.session_id().is_none() {
                            // auto new session in cwd of process
                            let cwd = std::env::current_dir()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_else(|_| ".".into());
                            if let Err(e) = c.new_session(&cwd) {
                                bridge::emit(AgentEvent::Error { message: e });
                                continue;
                            }
                        }
                        if let Err(e) = c.send_prompt(&text) {
                            bridge::emit(AgentEvent::Error { message: e });
                        }
                    }
                }
                AgentCmd::Cancel => {
                    if let Some(c) = client.as_mut() {
                        if let Err(e) = c.cancel() {
                            bridge::emit(AgentEvent::Error { message: e });
                        }
                    }
                }
                AgentCmd::Permission {
                    request_id,
                    option_id,
                } => {
                    if let Some(c) = client.as_mut() {
                        if let Err(e) = c.respond_permission(request_id, &option_id) {
                            bridge::emit(AgentEvent::Error { message: e });
                        }
                    }
                }
                AgentCmd::PermissionCancel { request_id } => {
                    if let Some(c) = client.as_mut() {
                        if let Err(e) = c.cancel_permission(request_id) {
                            bridge::emit(AgentEvent::Error { message: e });
                        }
                    }
                }
                AgentCmd::RefreshSessions { cwd } => refresh_sessions(&cwd),
            }
        }

        // Poll child for unsolicited notifications when idle.
        if let Some(c) = client.as_mut() {
            if let Err(e) = c.poll(Duration::from_millis(5)) {
                bridge::emit(AgentEvent::Disconnected { reason: e });
                client = None;
            }
        }
    }
}

fn connect(mode: &ConnectionMode) -> Option<AcpClient> {
    match mode {
        ConnectionMode::StdioChild { spec } => connect_stdio(spec, mode.label()),
        ConnectionMode::Leader { socket } => {
            bridge::emit(AgentEvent::Error {
                message: format!(
                    "leader mode not implemented yet (socket {}); using local child is required",
                    socket.display()
                ),
            });
            None
        }
    }
}

fn connect_stdio(spec: &BackendSpec, label: ConnectionModeLabel) -> Option<AcpClient> {
    match ChildTransport::spawn(spec) {
        Ok(transport) => {
            let mut client = AcpClient::new(transport);
            match client.initialize() {
                Ok(()) => {
                    bridge::emit(AgentEvent::Connected {
                        backend: spec.label.to_string(),
                        mode: label,
                    });
                    Some(client)
                }
                Err(e) => {
                    bridge::emit(AgentEvent::NeedSetup {
                        message: format!("ACP initialize failed: {e}"),
                    });
                    None
                }
            }
        }
        Err(e) => {
            bridge::emit(AgentEvent::NeedSetup { message: e });
            None
        }
    }
}

fn refresh_sessions(cwd: &str) {
    let entries = sessions::list_for_cwd(cwd);
    bridge::emit(AgentEvent::SessionsListed { entries });
}

// silence unused import if any
#[allow(dead_code)]
fn _mode_default() -> ConnectionMode {
    ConnectionMode::v1_default()
}
