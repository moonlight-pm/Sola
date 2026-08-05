//! Worker ↔ UI channels (agent-style process-wide bridge).

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

use crate::worker::{MailCmd, MailEvent};

static EVENT_TX: OnceLock<mpsc::Sender<MailEvent>> = OnceLock::new();
static EVENT_RX: Mutex<Option<mpsc::Receiver<MailEvent>>> = Mutex::new(None);

static CMD_TX: OnceLock<mpsc::Sender<MailCmd>> = OnceLock::new();
static CMD_RX: Mutex<Option<mpsc::Receiver<MailCmd>>> = Mutex::new(None);

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

pub fn mail_send(cmd: MailCmd) {
    match CMD_TX.get() {
        Some(tx) => {
            if let Err(e) = tx.send(cmd) {
                tracing::warn!("mail command channel closed: {e}");
            }
        }
        None => tracing::warn!("mail_send before init_channels"),
    }
}

pub fn emit(ev: MailEvent) {
    match EVENT_TX.get() {
        Some(tx) => {
            let _ = tx.send(ev);
        }
        None => tracing::warn!("emit before init_channels"),
    }
}

pub fn take_cmd_rx() -> mpsc::Receiver<MailCmd> {
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

pub fn mail_subscription() -> Subscription<MailEvent> {
    Subscription::run(event_stream)
}

fn event_stream() -> impl Stream<Item = MailEvent> {
    init_channels();
    let rx_opt = EVENT_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<MailEvent>();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || loop {
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
            });
        }
        None => {
            tracing::warn!("mail event receiver already taken");
        }
    }
    iced_rx
}
