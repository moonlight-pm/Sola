//! The daemon's events, and whether it is there at all. A thread holds a
//! subscribed connection; when the daemon goes away it says so once and
//! tries again every few seconds, so Vault shows a calm state instead of
//! an error and comes back by itself.

use std::time::Duration;

use iced::Subscription;
use iced::futures::Stream;
use iced::futures::channel::mpsc;
use sola_vault_proto::{Event, subscribe};

use super::CLIENT;

const RETRY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub enum Link {
    /// Connected: ask for the status now.
    Up,
    /// The daemon is not running (or just went away).
    Down,
    Event(Event),
}

pub fn subscription() -> Subscription<Link> {
    Subscription::run(stream)
}

fn stream() -> impl Stream<Item = Link> {
    let (tx, rx) = mpsc::unbounded();
    let spawned = std::thread::Builder::new()
        .name("vault-events".into())
        .spawn(move || follow(tx));
    if let Err(e) = spawned {
        tracing::error!(error = %e, "could not start the vault events thread");
    }
    rx
}

fn follow(tx: mpsc::UnboundedSender<Link>) {
    // `None` until the first try: the first failure is said too.
    let mut up: Option<bool> = None;
    loop {
        match subscribe(CLIENT) {
            Ok(events) => {
                tracing::info!("following the vault service's events");
                up = Some(true);
                if tx.unbounded_send(Link::Up).is_err() {
                    return;
                }
                for ev in events {
                    if tx.unbounded_send(Link::Event(ev)).is_err() {
                        return;
                    }
                }
                tracing::warn!("the vault service's events ended");
            }
            Err(e) => {
                if up != Some(false) {
                    tracing::info!(reason = %e, "the vault service is not reachable; retrying");
                }
            }
        }
        if up != Some(false) {
            up = Some(false);
            if tx.unbounded_send(Link::Down).is_err() {
                return;
            }
        }
        std::thread::sleep(RETRY);
    }
}
