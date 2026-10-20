//! Worker ↔ UI channels (process-wide, like the kit's bus bridge).

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

use crate::worker::{Input, MailCmd, MailEvent};

static EVENT_TX: OnceLock<mpsc::Sender<MailEvent>> = OnceLock::new();
static EVENT_RX: Mutex<Option<mpsc::Receiver<MailEvent>>> = Mutex::new(None);

static INPUT_TX: OnceLock<mpsc::Sender<Input>> = OnceLock::new();
static INPUT_RX: Mutex<Option<mpsc::Receiver<Input>>> = Mutex::new(None);

pub fn init_channels() {
    EVENT_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *EVENT_RX.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
        tx
    });
    INPUT_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *INPUT_RX.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
        tx
    });
}

/// UI → worker.
pub fn mail_send(cmd: MailCmd) {
    match INPUT_TX.get() {
        Some(tx) => {
            if tx.send(Input::Cmd(cmd)).is_err() {
                tracing::warn!("mail worker is gone; command dropped");
            }
        }
        None => tracing::warn!("mail_send before init_channels"),
    }
}

/// Worker / sessions → UI.
pub fn emit(ev: MailEvent) {
    match EVENT_TX.get() {
        Some(tx) => {
            let _ = tx.send(ev);
        }
        None => tracing::warn!("emit before init_channels"),
    }
}

/// The coordinator's end: a sender for session threads + the receiver.
pub fn take_input() -> (mpsc::Sender<Input>, mpsc::Receiver<Input>) {
    init_channels();
    let tx = INPUT_TX.get().expect("initialized").clone();
    let rx = INPUT_RX
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or_else(|| {
            tracing::warn!("mail worker input already taken; returning disconnected");
            mpsc::channel().1
        });
    (tx, rx)
}

pub fn mail_subscription() -> Subscription<MailEvent> {
    Subscription::run(event_stream)
}

fn event_stream() -> impl Stream<Item = MailEvent> {
    init_channels();
    let rx = EVENT_RX.lock().unwrap_or_else(|e| e.into_inner()).take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<MailEvent>();
    match rx {
        Some(std_rx) => {
            std::thread::Builder::new()
                .name("sola-mail-events".into())
                .spawn(move || {
                    while let Ok(ev) = std_rx.recv() {
                        if iced_tx.unbounded_send(ev).is_err() {
                            break;
                        }
                    }
                })
                .expect("spawn mail event forwarder");
        }
        None => tracing::warn!("mail event receiver already taken"),
    }
    iced_rx
}
