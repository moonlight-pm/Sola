//! The daemon's events, and whether it is there at all. A thread holds a
//! subscribed connection; when the daemon goes away it says so once and
//! tries again every few seconds, so Vault shows a calm state instead of
//! an error and comes back by itself.

use std::time::Duration;

use iced::Subscription;
use iced::futures::Stream;
use iced::futures::channel::mpsc;
use sola_vault_proto::{ErrorCode, Event, subscribe};

use super::CLIENT;

const RETRY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub enum Link {
    /// Connected: ask for the status now.
    Up,
    /// The daemon is not running (or just went away). `refused`: it is,
    /// but would not let Vault in (its peer check), and says why.
    Down { refused: Option<String> },
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
    let mut refused: Option<String> = None;
    loop {
        match subscribe(CLIENT) {
            Ok(events) => {
                tracing::info!("following the vault service's events");
                up = Some(true);
                refused = None;
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
                let why = (e.code != ErrorCode::Unavailable).then(|| e.message.clone());
                if up != Some(false) || why != refused {
                    tracing::info!(reason = %e, code = ?e.code, "the vault service is not reachable; retrying");
                    up = None;
                    refused = why;
                }
            }
        }
        if up != Some(false) {
            up = Some(false);
            let down = Link::Down {
                refused: refused.clone(),
            };
            if tx.unbounded_send(down).is_err() {
                return;
            }
        }
        std::thread::sleep(RETRY);
    }
}
