//! The vault on the bus: `Topic::VaultState` (sticky, never a secret) on
//! every change; in, the desk's idle fade and its settings for the idle
//! lock; out, `LaunchApp` for the call plane's `open`. Reconnects when
//! the bus restarts (the client re-sends the sticky state itself).

use std::time::Duration;

use sola_bus::BusClient;
use sola_bus::topics::{Topic, TopicKind, VaultState};
use sola_vault_proto::Status;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::actor::{DeskSignal, Input};

const APP_ID: &str = "sola-vaultd";
const RETRY: Duration = Duration::from_secs(2);

/// The bus's view of a status.
pub fn vault_state(s: &Status) -> VaultState {
    VaultState {
        signed_in: s.signed_in,
        unlocked: s.unlocked,
        email: s.email.clone(),
        server: s.server.web_vault(),
        synced_at: s.synced_at,
        syncing: s.syncing,
        sync_error: s.sync_error.clone(),
        items: s.items,
    }
}

pub async fn run(
    mut status: watch::Receiver<Status>,
    to_vault: mpsc::UnboundedSender<Input>,
    mut outgoing: mpsc::UnboundedReceiver<Topic>,
) {
    let mut bus = BusClient::new();
    bus.set_app_id(APP_ID);
    let mut told: Option<VaultState> = None;
    loop {
        let mut logged = false;
        while let Err(e) = bus.connect() {
            if !logged {
                warn!(error = %e, "vault: bus not up; retrying");
                logged = true;
            }
            tokio::time::sleep(RETRY).await;
        }
        if let Err(e) = bus.subscribe(&[TopicKind::IdleFade, TopicKind::Appearance]) {
            warn!(error = %e, "vault: bus subscribe failed");
        }
        let Some(Ok(notify)) = bus.try_clone_notify() else {
            warn!("vault: bus notify unavailable; retrying");
            tokio::time::sleep(RETRY).await;
            continue;
        };
        let Ok(notify) = notify.set_nonblocking(true).and_then(|()| tokio::net::UnixStream::from_std(notify)) else {
            warn!("vault: bus notify not pollable; retrying");
            tokio::time::sleep(RETRY).await;
            continue;
        };
        info!("vault: on the bus");
        loop {
            let state = vault_state(&status.borrow_and_update());
            if told.as_ref() != Some(&state) {
                if let Err(e) = bus.emit(Topic::VaultState(state.clone())) {
                    warn!(error = %e, "vault: VaultState emit failed");
                }
                told = Some(state);
            }
            tokio::select! {
                changed = status.changed() => if changed.is_err() { return },
                topic = outgoing.recv() => match topic {
                    Some(topic) => {
                        if let Err(e) = bus.emit(topic) {
                            warn!(error = %e, "vault: bus emit failed");
                        }
                    }
                    None => return,
                },
                ready = notify.readable() => {
                    if ready.is_err() || !drain(&notify) {
                        warn!("vault: bus connection lost; reconnecting");
                        break;
                    }
                    while let Some(msg) = bus.try_recv() {
                        if let Some(signal) = Topic::parse(&msg).and_then(desk_signal) {
                            let _ = to_vault.send(Input::Desk(signal));
                        }
                    }
                }
            }
        }
    }
}

/// Clear the notify pipe; false when its writer is gone (the bus reader
/// stopped).
fn drain(notify: &tokio::net::UnixStream) -> bool {
    let mut buf = [0u8; 64];
    loop {
        match notify.try_read(&mut buf) {
            Ok(0) => return false,
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return true,
            Err(_) => return false,
        }
    }
}

fn desk_signal(topic: Topic) -> Option<DeskSignal> {
    match topic {
        Topic::IdleFade(fade) => Some(DeskSignal::Fade(fade.fading)),
        Topic::Appearance(look) => Some(DeskSignal::Look {
            fade_idle: look.fade_idle,
            fade_after_min: look.fade_after_min,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bus_state_carries_no_more_than_the_status() {
        let s = Status {
            signed_in: true,
            unlocked: false,
            email: "ana@example.com".into(),
            items: 12,
            ..Default::default()
        };
        let v = vault_state(&s);
        assert!(v.signed_in && !v.unlocked);
        assert_eq!(v.server, "https://vault.bitwarden.com");
        assert_eq!(v.items, 12);
    }
}
