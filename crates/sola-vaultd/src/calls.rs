//! Call-plane owner `vault` (P2): `status`, `lock`, `sync`, `open`. Agents
//! reach the call plane (`solactl mcp`), so no method here returns a
//! value from the vault — an agent can lock it or ask for a sync, never
//! read it.

use std::thread;

use serde_json::json;
use sola_bus::topics::{LaunchAppPayload, Topic};
use sola_call::{ArgSpec, ArgType, Incoming, MethodSpec};
use sola_vault_proto::Status;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{info, warn};

use crate::actor::{CallJob, Input};

pub const OWNER: &str = "vault";
const APP_ID: &str = "sola-vaultd";
/// Long enough for a full sync.
const SYNC_TIMEOUT_MS: u64 = 120_000;

pub fn methods() -> Vec<MethodSpec> {
    vec![
        method("status", "Signed in, locked or not, last sync, item count (no items, no values)", &[], None),
        method("lock", "Lock the vault now", &[], None),
        method("sync", "Sync the vault with Bitwarden", &[], Some(SYNC_TIMEOUT_MS)),
        method(
            "open",
            "Show an item in Vault",
            &[ArgSpec {
                name: "id".into(),
                long: Some("id".into()),
                short: Some('i'),
                ty: ArgType::String,
                required: true,
                help: "The item's id".into(),
            }],
            None,
        ),
    ]
}

/// Advertise `vault` and answer on a thread of its own.
pub fn start(status: watch::Receiver<Status>, to_vault: mpsc::UnboundedSender<Input>, bus_out: mpsc::UnboundedSender<Topic>) {
    let rx = sola_call::start_provider(OWNER, APP_ID, methods());
    let spawned = thread::Builder::new().name("vault-calls".into()).spawn(move || {
        while let Ok(incoming) = rx.recv() {
            dispatch(incoming, &status, &to_vault, &bus_out);
        }
        warn!("vault: call plane closed");
    });
    if let Err(e) = spawned {
        warn!(error = %e, "vault: call thread not started");
    }
}

fn dispatch(
    incoming: Incoming,
    status: &watch::Receiver<Status>,
    to_vault: &mpsc::UnboundedSender<Input>,
    bus_out: &mpsc::UnboundedSender<Topic>,
) {
    let Incoming { method, params, reply } = incoming;
    info!(%method, "vault: call");
    match method.as_str() {
        "status" => reply.ok(status_json(&status.borrow())),
        "lock" | "sync" => {
            let job = if method == "lock" { CallJob::Lock } else { CallJob::Sync };
            let (tx, rx) = oneshot::channel();
            if to_vault.send(Input::Call(job, tx)).is_err() {
                return reply.err("the vault stopped");
            }
            match rx.blocking_recv() {
                Ok(Ok(())) => reply.ok(status_json(&status.borrow())),
                Ok(Err(e)) => reply.err(e),
                Err(_) => reply.err("the vault stopped"),
            }
        }
        "open" => match params.get("id").and_then(|v| v.as_str()).and_then(item_id) {
            Some(id) => {
                let command = format!("{} --item {id}", sola_core::paths::bin_path("sola-vault").display());
                let launch = Topic::LaunchApp(LaunchAppPayload {
                    app_id: "sola-vault".into(),
                    command,
                });
                if bus_out.send(launch).is_err() {
                    return reply.err("the bus is away");
                }
                reply.ok(json!({ "opened": id }))
            }
            None => reply.err("id required: an item's id"),
        },
        other => reply.err(format!("unknown method {other}")),
    }
}

/// What the call plane may know: no item, no value.
fn status_json(s: &Status) -> serde_json::Value {
    json!({
        "signed_in": s.signed_in,
        "unlocked": s.unlocked,
        "email": s.email,
        "server": s.server.label(),
        "synced_at": s.synced_at,
        "syncing": s.syncing,
        "sync_error": s.sync_error,
        "items": s.items,
    })
}

/// An item id is a UUID; nothing else goes into a command line.
fn item_id(s: &str) -> Option<String> {
    let s = s.trim();
    let shape = s.len() == 36
        && s.char_indices()
            .all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() });
    shape.then(|| s.to_ascii_lowercase())
}

fn method(name: &str, summary: &str, args: &[ArgSpec], timeout_ms: Option<u64>) -> MethodSpec {
    MethodSpec {
        name: name.into(),
        summary: summary.into(),
        args: args.to_vec(),
        timeout_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_plane_has_no_reading_method() {
        let names: Vec<String> = methods().into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["status", "lock", "sync", "open"]);
    }

    #[test]
    fn open_takes_only_an_item_id() {
        assert_eq!(
            item_id("11111111-AAAA-1111-1111-111111111111").as_deref(),
            Some("11111111-aaaa-1111-1111-111111111111")
        );
        assert_eq!(item_id("x; rm -rf ~"), None);
        assert_eq!(item_id("11111111-1111-1111-1111-11111111111 --x"), None);
    }

    #[test]
    fn the_status_names_no_item() {
        let v = status_json(&Status::default());
        assert_eq!(v["server"], "bitwarden.com");
        assert!(v.get("unreadable").is_none());
    }
}
