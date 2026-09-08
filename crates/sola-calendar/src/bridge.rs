//! Worker ↔ UI channels.

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

use crate::worker::{CalCmd, CalNotice};

static EVENT_TX: OnceLock<mpsc::Sender<CalNotice>> = OnceLock::new();
static EVENT_RX: Mutex<Option<mpsc::Receiver<CalNotice>>> = Mutex::new(None);

static CMD_TX: OnceLock<mpsc::Sender<CalCmd>> = OnceLock::new();
static CMD_RX: Mutex<Option<mpsc::Receiver<CalCmd>>> = Mutex::new(None);

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

pub fn send(cmd: CalCmd) {
    match CMD_TX.get() {
        Some(tx) => {
            if let Err(e) = tx.send(cmd) {
                tracing::warn!("calendar command channel closed: {e}");
            }
        }
        None => tracing::warn!("calendar send before init_channels"),
    }
}

pub fn emit(ev: CalNotice) {
    match EVENT_TX.get() {
        Some(tx) => {
            let _ = tx.send(ev);
        }
        None => tracing::warn!("calendar emit before init_channels"),
    }
}

pub fn take_cmd_rx() -> mpsc::Receiver<CalCmd> {
    init_channels();
    match CMD_RX.lock().unwrap().take() {
        Some(rx) => rx,
        None => {
            let (tx, rx) = mpsc::channel();
            drop(tx);
            rx
        }
    }
}

pub fn subscription() -> Subscription<CalNotice> {
    Subscription::run(event_stream)
}

fn event_stream() -> impl Stream<Item = CalNotice> {
    init_channels();
    let rx_opt = EVENT_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<CalNotice>();
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
        None => tracing::warn!("calendar event receiver already taken"),
    }
    iced_rx
}
