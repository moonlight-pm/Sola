//! ACP worker thread: owns the leader-bridge connection and applies UI commands.

use std::collections::VecDeque;
use std::time::Duration;

use crate::acp::{AcpClient, ChildTransport};
use crate::backend::{self, BackendSpec, ConnectionMode};
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
    // Connect eagerly so the status bar shows leader state without waiting
    // for the first user action.
    let mut client: Option<AcpClient> = connect(&mode);
    // Local buffer so we can coalesce session switches without dropping
    // other commands that arrived while a slow `session/load` was in flight.
    let mut inbox: VecDeque<AgentCmd> = VecDeque::new();

    loop {
        // Fill inbox: block briefly when empty so we can poll the child.
        if inbox.is_empty() {
            match cmd_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(c) => inbox.push_back(c),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        while let Ok(c) = cmd_rx.try_recv() {
            inbox.push_back(c);
        }
        // Drop superseded LoadSession/NewSession so rapid sidebar clicks
        // never queue multi-second attach storms.
        coalesce_session_cmds(&mut inbox);

        let Some(cmd) = inbox.pop_front() else {
            if let Some(c) = client.as_mut() {
                if let Err(e) = c.poll(Duration::from_millis(5)) {
                    bridge::emit(AgentEvent::Disconnected { reason: e });
                    client = None;
                }
            }
            continue;
        };

        match cmd {
            AgentCmd::Shutdown => break,
            AgentCmd::Restart | AgentCmd::EnsureConnected => {
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
                        Err(e) => bridge::emit(AgentEvent::Error {
                            session_id: None,
                            message: e,
                        }),
                    }
                }
            }
            AgentCmd::LoadSession { id, cwd } => {
                // Re-coalesce: more LoadSessions may have arrived while we
                // were blocked on a previous attach.
                while let Ok(c) = cmd_rx.try_recv() {
                    inbox.push_back(c);
                }
                coalesce_session_cmds(&mut inbox);
                // If a newer switch is already queued (maybe behind a Cancel),
                // drop this attach — only the latest target matters.
                if inbox.iter().any(|c| {
                    matches!(
                        c,
                        AgentCmd::LoadSession { .. } | AgentCmd::NewSession { .. }
                    )
                }) {
                    continue;
                }
                if client.is_none() {
                    client = connect(&mode);
                }
                if let Some(c) = client.as_mut() {
                    match c.load_session(&id, &cwd) {
                        Ok(()) => {
                            crate::overlay::note_opened(&id, &cwd);
                            refresh_sessions(&cwd);
                        }
                        Err(e) => bridge::emit(AgentEvent::Error {
                            session_id: Some(id),
                            message: e,
                        }),
                    }
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
                            bridge::emit(AgentEvent::Error {
                                session_id: None,
                                message: e,
                            });
                            continue;
                        }
                    }
                    if let Err(e) = c.send_prompt(&text) {
                        bridge::emit(AgentEvent::Error {
                            session_id: c.session_id().map(|s| s.to_string()),
                            message: e,
                        });
                    }
                }
            }
            AgentCmd::SetPermissionMode { mode_id } => {
                if let Some(c) = client.as_mut() {
                    if let Err(e) = c.set_mode(&mode_id) {
                        bridge::emit(AgentEvent::Error {
                            session_id: c.session_id().map(|s| s.to_string()),
                            message: format!("set permission mode: {e}"),
                        });
                    }
                }
            }
            AgentCmd::SetEffort { effort_id } => {
                if let Some(c) = client.as_mut() {
                    // Grok maps effort ids through session/set_mode.
                    if let Err(e) = c.set_mode(&effort_id) {
                        bridge::emit(AgentEvent::Error {
                            session_id: c.session_id().map(|s| s.to_string()),
                            message: format!("set effort: {e}"),
                        });
                    }
                }
            }
            AgentCmd::Cancel => {
                if let Some(c) = client.as_mut() {
                    if let Err(e) = c.cancel() {
                        bridge::emit(AgentEvent::Error {
                            session_id: c.session_id().map(|s| s.to_string()),
                            message: e,
                        });
                    }
                }
            }
            AgentCmd::Permission {
                request_id,
                option_id,
            } => {
                if let Some(c) = client.as_mut() {
                    if let Err(e) = c.respond_permission(request_id, &option_id) {
                        bridge::emit(AgentEvent::Error {
                            session_id: c.session_id().map(|s| s.to_string()),
                            message: e,
                        });
                    }
                }
            }
            AgentCmd::PermissionCancel { request_id } => {
                if let Some(c) = client.as_mut() {
                    if let Err(e) = c.cancel_permission(request_id) {
                        bridge::emit(AgentEvent::Error {
                            session_id: c.session_id().map(|s| s.to_string()),
                            message: e,
                        });
                    }
                }
            }
            AgentCmd::RefreshSessions { cwd } => refresh_sessions(&cwd),
            AgentCmd::BulkDelete { ids } => {
                let total = ids.len() as u32;
                let mut deleted = 0u32;
                let mut failed = 0u32;
                let mut errors = Vec::new();
                for (i, id) in ids.into_iter().enumerate() {
                    match sessions::delete_session(&id) {
                        Ok(()) => deleted += 1,
                        Err(e) => {
                            failed += 1;
                            if errors.len() < 8 {
                                errors.push(e);
                            }
                        }
                    }
                    bridge::emit(AgentEvent::BulkDeleteProgress {
                        done: (i as u32) + 1,
                        total,
                        last_id: id,
                    });
                }
                bridge::emit(AgentEvent::BulkDeleteFinished {
                    deleted,
                    failed,
                    errors,
                });
                // Refresh sidebar after disk changes.
                let entries = sessions::list_all();
                bridge::emit(AgentEvent::SessionsListed { entries });
            }
        }

        // Poll bridge for unsolicited notifications between commands.
        if let Some(c) = client.as_mut() {
            if let Err(e) = c.poll(Duration::from_millis(5)) {
                bridge::emit(AgentEvent::Disconnected { reason: e });
                client = None;
            }
        }
    }
}

/// Keep only the **latest** `LoadSession` / `NewSession` in the inbox.
/// Other commands keep relative order; the surviving session op sits where
/// the first session op was (so a Cancel enqueued before a switch still
/// runs first).
fn coalesce_session_cmds(inbox: &mut VecDeque<AgentCmd>) {
    let mut last_session: Option<AgentCmd> = None;
    let mut first_session_slot: Option<usize> = None;
    let mut out: VecDeque<AgentCmd> = VecDeque::new();
    for c in inbox.drain(..) {
        match c {
            AgentCmd::LoadSession { .. } | AgentCmd::NewSession { .. } => {
                if first_session_slot.is_none() {
                    first_session_slot = Some(out.len());
                }
                last_session = Some(c);
            }
            other => out.push_back(other),
        }
    }
    if let Some(sess) = last_session {
        let at = first_session_slot.unwrap_or(out.len()).min(out.len());
        out.insert(at, sess);
    }
    *inbox = out;
}

fn connect(mode: &ConnectionMode) -> Option<AcpClient> {
    match mode {
        ConnectionMode::Leader { socket, bridge } => {
            connect_leader(socket, bridge, mode.label())
        }
    }
}

fn connect_leader(
    socket: &std::path::Path,
    bridge_spec: &BackendSpec,
    label: ConnectionModeLabel,
) -> Option<AcpClient> {
    // Preflight: refuse to spawn the bridge when the leader is down.
    // `grok agent --leader stdio` auto-spawns a leader otherwise, which
    // fights the systemd-managed sticky unit.
    if !backend::leader_reachable(socket) {
        bridge::emit(AgentEvent::NeedSetup {
            message: format!(
                "Grok leader is not running (nothing accepting on {}). \
                 Start it with: systemctl --user start grok-leader.service \
                 (or enable at login: systemctl --user enable --now grok-leader.service).",
                socket.display()
            ),
        });
        bridge::emit(AgentEvent::Disconnected {
            reason: "leader not reachable".into(),
        });
        return None;
    }

    match ChildTransport::spawn(bridge_spec) {
        Ok(transport) => {
            let mut client = AcpClient::new(transport);
            match client.initialize() {
                Ok(()) => {
                    bridge::emit(AgentEvent::Connected {
                        backend: bridge_spec.label.to_string(),
                        mode: label,
                    });
                    // Apply default permission mode on the next session; no
                    // session yet. Version watcher is independent.
                    Some(client)
                }
                Err(e) => {
                    bridge::emit(AgentEvent::NeedSetup {
                        message: format!(
                            "Leader is up but ACP initialize failed: {e}. \
                             Check auth (`grok login`) and: systemctl --user status grok-leader"
                        ),
                    });
                    None
                }
            }
        }
        Err(e) => {
            bridge::emit(AgentEvent::NeedSetup {
                message: format!(
                    "Failed to start leader bridge (`grok agent --leader stdio`): {e}"
                ),
            });
            None
        }
    }
}

fn refresh_sessions(_cwd: &str) {
    // Always list every project group so the sidebar is not limited to
    // the process cwd / last project.
    let entries = sessions::list_all();
    bridge::emit(AgentEvent::SessionsListed { entries });
}
