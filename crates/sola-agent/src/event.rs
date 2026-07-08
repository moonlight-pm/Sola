//! Agent event / command types + the iced <-> worker bridge.
//!
//! Foundation defines the message enums and the `NodeId` alias. This file
//! also carries the bridge itself: process-wide channel statics,
//! `init_channels`, `agent_subscription`, `agent_send`, `emit`, and
//! `take_cmd_rx`.
//!
//! Mirrors `sola-terminal`'s `emulator.rs` process-wide channel pattern: an
//! `OnceLock<Sender<_>>` paired with a `Mutex<Option<Receiver<_>>>` so the
//! single receiver is taken exactly once and a second taker gets a guarded,
//! inert channel instead of racing on it. Two directions:
//!
//! - EVENT: worker → UI. `emit` sends; `agent_subscription` drains into iced.
//! - CMD:   UI → worker. `agent_send` sends; `take_cmd_rx` hands the engine
//!   thread the single receiver.

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

use crate::session::Usage;
use crate::tools::ToolResult;

pub type NodeId = String; // uuid v4 string

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Delta { node_id: NodeId, text: String },
    Reasoning { text: String },
    ToolStart { call_id: String, tool: String, args: serde_json::Value },
    ToolOutput { call_id: String, chunk: String },
    ToolEnd { call_id: String, result: ToolResult },
    ApprovalRequest { call_id: String, tool: String, preview: String },
    TurnEnd { usage: Usage },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum AgentCmd {
    Send { text: String, branch_from: Option<NodeId> },
    Approve { call_id: String, remember: bool },
    Deny { call_id: String },
    Abort,
}

// ── Process-wide statics ──────────────────────────────────────────────────────

/// worker → UI.
static EVENT_TX: OnceLock<mpsc::Sender<AgentEvent>> = OnceLock::new();
static EVENT_RX: Mutex<Option<mpsc::Receiver<AgentEvent>>> = Mutex::new(None);

/// UI → worker.
static CMD_TX: OnceLock<mpsc::Sender<AgentCmd>> = OnceLock::new();
static CMD_RX: Mutex<Option<mpsc::Receiver<AgentCmd>>> = Mutex::new(None);

/// Create both channel pairs into the statics. Idempotent (`get_or_init`), so
/// it is safe to call from `main` at startup and again from any lazy path
/// (e.g. `agent_subscription` guards against being built before startup ran).
pub fn init_channels() {
    EVENT_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<AgentEvent>();
        *EVENT_RX.lock().unwrap() = Some(rx);
        tx
    });
    CMD_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<AgentCmd>();
        *CMD_RX.lock().unwrap() = Some(rx);
        tx
    });
}

/// UI → worker: enqueue a command. Dropped with a warning if the channels
/// aren't initialised yet — never a panic.
pub fn agent_send(cmd: AgentCmd) {
    match CMD_TX.get() {
        Some(tx) => {
            let _ = tx.send(cmd);
        }
        None => {
            tracing::warn!(?cmd, "agent_send before init_channels; dropping command");
        }
    }
}

/// worker → UI: emit an event toward the iced subscription. Dropped with a
/// warning if the channels aren't initialised yet — never a panic.
pub(crate) fn emit(ev: AgentEvent) {
    match EVENT_TX.get() {
        Some(tx) => {
            let _ = tx.send(ev);
        }
        None => {
            tracing::warn!(?ev, "emit before init_channels; dropping event");
        }
    }
}

/// The engine thread takes the single command receiver exactly once. A second
/// call is guarded: it logs and returns a fresh, already-disconnected receiver
/// (its sender immediately dropped) so callers get an inert receiver rather
/// than a panic — the same "one receiver per process" discipline
/// `agent_subscription` uses for the event side.
pub(crate) fn take_cmd_rx() -> mpsc::Receiver<AgentCmd> {
    init_channels();
    match CMD_RX.lock().unwrap().take() {
        Some(rx) => rx,
        None => {
            tracing::warn!(
                "take_cmd_rx called while receiver is already taken; \
                 returning a disconnected receiver (one receiver per process)"
            );
            let (tx, rx) = mpsc::channel::<AgentCmd>();
            drop(tx);
            rx
        }
    }
}

/// iced `Subscription` delivering `AgentEvent`s from the worker. The receiver
/// is taken once; a rebuilt subscription (iced rebuilds the set on every
/// update) gets an empty stream — mirror of `emulator.rs::output_subscription`.
pub fn agent_subscription() -> Subscription<AgentEvent> {
    Subscription::run(event_stream)
}

fn event_stream() -> impl Stream<Item = AgentEvent> {
    init_channels();
    let rx_opt = EVENT_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<AgentEvent>();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || loop {
                // Exit if the iced side dropped the subscription.
                if iced_tx.is_closed() {
                    break;
                }
                match std_rx.recv() {
                    Ok(ev) => {
                        if iced_tx.unbounded_send(ev).is_err() {
                            break;
                        }
                    }
                    // All senders dropped — worker gone. Stop.
                    Err(_) => break,
                }
            });
        }
        None => {
            tracing::warn!(
                "agent_subscription called while receiver is already taken; \
                 returning empty stream (one receiver per process)"
            );
            drop(iced_tx);
        }
    }
    iced_rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::TryRecvError;

    /// Full bridge round-trip in a single test to keep the process-global
    /// statics deterministic: CMD (UI→worker), EVENT (worker→UI) drained
    /// straight off the static receiver, and the second-take guard.
    #[test]
    fn bridge_round_trips_and_guards_second_take() {
        init_channels();

        // ── CMD: UI → worker ──────────────────────────────────────────────
        agent_send(AgentCmd::Send { text: "hello".into(), branch_from: None });
        agent_send(AgentCmd::Abort);

        let cmd_rx = take_cmd_rx(); // first take → the real receiver
        match cmd_rx.try_recv() {
            Ok(AgentCmd::Send { text, branch_from }) => {
                assert_eq!(text, "hello");
                assert!(branch_from.is_none());
            }
            other => panic!("expected Send, got {other:?}"),
        }
        match cmd_rx.try_recv() {
            Ok(AgentCmd::Abort) => {}
            other => panic!("expected Abort, got {other:?}"),
        }

        // ── EVENT: worker → UI, drained straight off the static receiver ──
        emit(AgentEvent::Delta { node_id: "n1".into(), text: "chunk".into() });
        let event_rx = EVENT_RX
            .lock()
            .unwrap()
            .take()
            .expect("EVENT_RX present after init_channels");
        match event_rx.try_recv() {
            Ok(AgentEvent::Delta { node_id, text }) => {
                assert_eq!(node_id, "n1");
                assert_eq!(text, "chunk");
            }
            other => panic!("expected Delta, got {other:?}"),
        }

        // ── Second take is guarded: an inert, disconnected receiver ───────
        let dead_rx = take_cmd_rx();
        assert!(matches!(dead_rx.try_recv(), Err(TryRecvError::Disconnected)));

        // Dropping the only live receiver and sending again must not panic —
        // the send just fails silently (swallowed by `agent_send`).
        drop(cmd_rx);
        agent_send(AgentCmd::Abort);
    }
}
