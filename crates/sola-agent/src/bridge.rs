//! Worker ↔ UI channels (terminal-style process-wide bridge).
//!
//! EVENT: worker → UI. CMD: UI → worker. Receivers are taken exactly once so
//! iced resubscribing does not race on the single consumer.

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

use crate::protocol::{AgentCmd, AgentEvent};

static EVENT_TX: OnceLock<mpsc::Sender<AgentEvent>> = OnceLock::new();
static EVENT_RX: Mutex<Option<mpsc::Receiver<AgentEvent>>> = Mutex::new(None);

static CMD_TX: OnceLock<mpsc::Sender<AgentCmd>> = OnceLock::new();
static CMD_RX: Mutex<Option<mpsc::Receiver<AgentCmd>>> = Mutex::new(None);

pub fn init_channels() {
    EVENT_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *EVENT_RX.lock().unwrap() = Some(rx);
        tx
    });
    CMD_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *CMD_RX.lock().unwrap() = Some(rx);
        tx
    });
}

pub fn agent_send(cmd: AgentCmd) {
    match CMD_TX.get() {
        Some(tx) => {
            if let Err(e) = tx.send(cmd) {
                tracing::warn!("agent command channel closed: {e}");
            }
        }
        None => tracing::warn!("agent_send before init_channels"),
    }
}

pub fn emit(ev: AgentEvent) {
    match EVENT_TX.get() {
        Some(tx) => {
            let _ = tx.send(ev);
        }
        None => tracing::warn!("emit before init_channels"),
    }
}

/// Engine thread takes the single command receiver exactly once.
pub fn take_cmd_rx() -> mpsc::Receiver<AgentCmd> {
    init_channels();
    match CMD_RX.lock().unwrap().take() {
        Some(rx) => rx,
        None => {
            tracing::warn!("take_cmd_rx: receiver already taken; returning disconnected");
            let (tx, rx) = mpsc::channel();
            drop(tx);
            rx
        }
    }
}

pub fn agent_subscription() -> Subscription<AgentEvent> {
    Subscription::run(event_stream)
}

fn event_stream() -> impl Stream<Item = AgentEvent> {
    init_channels();
    let rx_opt = EVENT_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<AgentEvent>();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || {
                loop {
                    if iced_tx.is_closed() {
                        break;
                    }
                    match std_rx.recv() {
                        Ok(ev) => {
                            if iced_tx.unbounded_send(ev).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        None => {
            tracing::warn!("agent_subscription: receiver already taken; empty stream");
            drop(iced_tx);
        }
    }
    iced_rx
}
