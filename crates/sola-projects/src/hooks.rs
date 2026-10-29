//! Grok, Codex, and Claude hook installer + Unix-socket receiver.
//!
//! OpenCode stays presence-only. Identity is `SOLA_PANE_ID`, not the hook file name.

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

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

/// Install Grok + Codex hooks and start the UDS server. Fail-open on errors.
pub fn start() -> HookPaths {
    let paths = HookPaths::live();
    if let Err(e) = install::install(&paths) {
        tracing::warn!("agent hook install failed: {e}");
    } else {
        tracing::info!(
            grok_script = %paths.script_path.display(),
            grok_hooks = %paths.grok_hooks_dir.display(),
            codex_script = %paths.codex_script_path.display(),
            codex_hooks = %paths.codex_hooks_json.display(),
            claude_script = %paths.claude_script_path.display(),
            claude_settings = %paths.claude_settings_json.display(),
            "installed grok, codex, and claude sola-status hooks"
        );
    }
    match server::bind(&paths.socket_path) {
        Ok(listener) => {
            ensure_channel();
            let tx = HOOK_TX.get().unwrap().clone();
            std::thread::Builder::new()
                .name("sola-ws-hooks".into())
                .spawn(move || server::serve(listener, tx, None))
                .ok();
            tracing::info!(sock = %paths.socket_path.display(), "hook socket listening");
        }
        Err(e) => tracing::warn!("hook socket bind failed: {e}"),
    }
    paths
}

/// A hook socket here for `host`'s agents alone (its link forwards
/// theirs to it, [`crate::link`]): every hook it takes says `host`.
pub fn listen_host(host: &str) -> std::io::Result<std::path::PathBuf> {
    let path = sola_core::env::runtime_dir()
        .join(format!("sola-ws-hooks-{}.sock", crate::spawn::slug(host)));
    let listener = server::bind(&path)?;
    ensure_channel();
    let tx = HOOK_TX.get().unwrap().clone();
    let host = host.to_string();
    std::thread::Builder::new()
        .name(format!("hooks-{host}"))
        .spawn(move || server::serve(listener, tx, Some(host)))?;
    tracing::info!(sock = %path.display(), "remote hook socket listening");
    Ok(path)
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
