//! Grok hook installer + Unix-socket receiver.
//!
//! Other agents wait. Identity is `SOLA_PANE_ID`, not the hook file name.

use std::sync::{mpsc, Mutex, OnceLock};

use iced::futures::Stream;
use iced::Subscription;

pub mod install;
pub mod map;
pub mod server;

pub use install::HookPaths;
pub use server::Incoming;

static HOOK_TX: OnceLock<mpsc::Sender<Incoming>> = OnceLock::new();
static HOOK_RX: Mutex<Option<mpsc::Receiver<Incoming>>> = Mutex::new(None);

fn ensure_channel() {
    HOOK_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *HOOK_RX.lock().unwrap() = Some(rx);
        tx
    });
}

/// Install Grok hooks and start the UDS server. Fail-open on errors.
pub fn start() -> HookPaths {
    let paths = HookPaths::live();
    if let Err(e) = install::install(&paths) {
        tracing::warn!("grok hook install failed: {e}");
    } else {
        tracing::info!(
            script = %paths.script_path.display(),
            hooks = %paths.grok_hooks_dir.display(),
            "installed grok sola-status hooks"
        );
    }
    match server::bind(&paths.socket_path) {
        Ok(listener) => {
            ensure_channel();
            let tx = HOOK_TX.get().unwrap().clone();
            std::thread::Builder::new()
                .name("sola-ws-hooks".into())
                .spawn(move || server::serve(listener, tx))
                .ok();
            tracing::info!(sock = %paths.socket_path.display(), "hook socket listening");
        }
        Err(e) => tracing::warn!("hook socket bind failed: {e}"),
    }
    paths
}

pub fn subscription() -> Subscription<Incoming> {
    Subscription::run(hook_stream)
}

fn hook_stream() -> impl Stream<Item = Incoming> {
    ensure_channel();
    let rx_opt = HOOK_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || {
                while !iced_tx.is_closed() {
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
        None => drop(iced_tx),
    }
    iced_rx
}
