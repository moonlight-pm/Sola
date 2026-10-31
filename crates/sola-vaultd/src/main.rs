//! sola-vaultd — the desk's Bitwarden service: the one place the vault is
//! unlocked (`docs/specs/2026-10-31-sola-vault-design.md`).
//!
//! It owns the SDK, the keys, sync, URI matching, codes, passkey signing,
//! the generator and deliveries. Vault (the app), the browser and the
//! shell ask it on a private socket (`sola_vault_proto`); any other Sola
//! app may redeem a delivery made for it. The bus gets its state, the
//! call plane `status`, `lock`, `sync` and `open` — never a value.
//!
//! On disk (`~/.local/share/sola/vault/`): prefs, settings, and the
//! signed-in account in its locked form (V1). The decrypted user key
//! lives in this process's memory only, so after a restart the vault is
//! signed in and locked.
//!
//! Env: `SOLA_VAULT_DEV_ROOTS` — extra directories (colon-separated)
//! whose Sola binaries may connect, for development from `target/`.

mod actor;
mod bus;
mod calls;
mod harden;
mod idle;
mod migrate;
mod paths;
mod peer;
mod prefs;
mod server;
mod settings;
mod tickets;
mod vault;

use std::sync::Arc;

use sola_vault_proto::Status;
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{error, info, warn};

fn main() {
    sola_core::log::init("sola-vaultd");
    let socket = sola_vault_proto::socket_path();
    if server::live(&socket) {
        error!(path = %socket.display(), "vault: another sola-vaultd is answering; not starting");
        std::process::exit(1);
    }
    // Before anything secret is read.
    harden::apply();

    let dir = paths::state_dir();
    if let Err(e) = paths::ensure_private_dir(&dir) {
        error!(dir = %dir.display(), error = %e, "vault: state dir not made");
        std::process::exit(1);
    }
    let roots = peer::roots();
    let dev = peer::dev_roots();
    if !dev.is_empty() {
        warn!(roots = ?dev, "vault: development roots allowed to connect ({})", peer::DEV_ROOTS_ENV);
    }

    let mut prefs = prefs::Prefs::load(&dir);
    migrate::run(&dir, &mut prefs);
    let settings = settings::load(&dir);

    let (status_tx, status_rx) = watch::channel(Status::default());
    let (events_tx, _) = broadcast::channel(64);
    let (to_vault, from_clients) = mpsc::unbounded_channel();
    let (bus_out, bus_out_rx) = mpsc::unbounded_channel();

    let vault = actor::Vault::new(
        dir,
        prefs,
        settings,
        actor::Outlets {
            status: status_tx,
            events: events_tx.clone(),
        },
    );
    // The SDK's futures run on a thread of their own; a KDF there holds
    // up nothing but the vault.
    let owner = std::thread::Builder::new().name("vault".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("vault runtime");
        rt.block_on(actor::run(vault, from_clients));
    });
    if let Err(e) = owner {
        error!(error = %e, "vault: owner thread not started");
        std::process::exit(1);
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("vault-io")
        .enable_all()
        .build()
        .expect("io runtime");
    rt.block_on(async move {
        let listener = match server::bind(&socket) {
            Ok(l) => l,
            Err(e) => {
                error!(path = %socket.display(), error = %e, "vault: socket not bound");
                std::process::exit(1);
            }
        };
        tokio::spawn(bus::run(status_rx.clone(), to_vault.clone(), bus_out_rx));
        calls::start(status_rx.clone(), to_vault.clone(), bus_out);
        info!("vault: ready");
        let ctx = Arc::new(server::Ctx {
            to_vault,
            status: status_rx,
            events: events_tx,
            roots,
        });
        server::serve(listener, ctx).await;
    });
}
