//! The browser as a client of `sola-vaultd`: requests off the iced
//! thread, and the daemon's events.
//!
//! Each request runs on its own short connection, so a login's key
//! derivation or a long sync never holds up a fill. Answers come back
//! tagged with why chrome asked (`A`), on one channel the chrome drains
//! when [`crate::chrome_wake`] wakes it. A second thread keeps a
//! subscription open, and reconnects while the daemon is away.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use sola_vault_proto::{self as proto, Event, Reply, Request, VaultError};

/// The name the daemon knows this client by (its log, not its peer check).
const CLIENT: &str = "sola-browser";

/// Waits between attempts to subscribe while the daemon is away.
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(10);

/// What the vault said, for chrome.
#[derive(Debug)]
pub enum VaultEvent<A> {
    /// The answer to one [`VaultHandle::ask`], with its tag.
    Answer(A, Result<Reply, VaultError>),
    /// Pushed by the daemon (never a secret).
    Daemon(Event),
    /// The subscription is up (`true`), or the daemon went away.
    Connected(bool),
}

/// Chrome's end: ask, and drain what came back.
pub struct VaultHandle<A> {
    tx: Sender<VaultEvent<A>>,
    rx: Mutex<Receiver<VaultEvent<A>>>,
}

impl<A: Send + 'static> VaultHandle<A> {
    /// Start the events thread. Nothing connects until it runs.
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let events = tx.clone();
        if let Err(e) = thread::Builder::new()
            .name("sola-vault-events".into())
            .spawn(move || events_loop(events))
        {
            tracing::error!(error = %e, "vault: could not start the events thread");
        }
        Self {
            tx,
            rx: Mutex::new(rx),
        }
    }

    /// Send `req` on a connection of its own; the answer comes back as
    /// [`VaultEvent::Answer`] with `tag`.
    pub fn ask(&self, tag: A, req: Request) {
        let tx = self.tx.clone();
        let spawned = thread::Builder::new()
            .name("sola-vault-ask".into())
            .spawn(move || {
                let answer = proto::call(CLIENT, req);
                emit(&tx, VaultEvent::Answer(tag, answer));
            });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "vault: could not start a request thread");
        }
    }

    pub fn try_recv(&self) -> Option<VaultEvent<A>> {
        match self.rx.lock().ok()?.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }
}

fn emit<A>(tx: &Sender<VaultEvent<A>>, ev: VaultEvent<A>) -> bool {
    let sent = tx.send(ev).is_ok();
    crate::chrome_wake::wake();
    sent
}

/// Subscribe, forward events until the daemon goes away, wait, again.
/// Ends when chrome drops its handle.
fn events_loop<A>(tx: Sender<VaultEvent<A>>) {
    let mut up: Option<bool> = None;
    let mut wait = RETRY_FIRST;
    loop {
        match proto::subscribe(CLIENT) {
            Ok(events) => {
                wait = RETRY_FIRST;
                up = Some(true);
                tracing::info!("vault: subscribed to the daemon");
                if !emit(&tx, VaultEvent::Connected(true)) {
                    return;
                }
                for ev in events {
                    if !emit(&tx, VaultEvent::Daemon(ev)) {
                        return;
                    }
                }
                tracing::info!("vault: the daemon closed its events");
            }
            Err(e) => {
                if up != Some(false) {
                    tracing::info!(error = %e, "vault: the daemon is not reachable");
                }
            }
        }
        if up != Some(false) {
            up = Some(false);
            if !emit(&tx, VaultEvent::Connected(false)) {
                return;
            }
        }
        thread::sleep(wait);
        wait = (wait * 2).min(RETRY_MAX);
    }
}
