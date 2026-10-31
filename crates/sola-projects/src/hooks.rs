//! Grok, Codex, and Claude hooks: installed on each computer Projects
//! has sessions on, read off that computer's hook log ([`log`]).
//!
//! OpenCode stays presence-only. Identity is `SOLA_PANE_ID` (and the
//! payload's cwd), not the hook file name.

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

pub mod event;
pub mod install;
pub mod log;
pub mod map;

pub use event::Incoming;
pub use install::HookPaths;
pub use log::Event;

static HOOK_TX: OnceLock<mpsc::Sender<Event>> = OnceLock::new();
static HOOK_RX: Mutex<Option<mpsc::Receiver<Event>>> = Mutex::new(None);

fn ensure_channel() {
    HOOK_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *HOOK_RX.lock().unwrap() = Some(rx);
        tx
    });
}

/// Where events read off a hook log go (this computer's, or one a link
/// follows).
pub fn sender() -> mpsc::Sender<Event> {
    ensure_channel();
    HOOK_TX.get().unwrap().clone()
}

/// Install the hooks here and follow this computer's hook log. Fail-open
/// on errors.
pub fn start() {
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
            log = %self::log::local_path().display(),
            "installed grok, codex, and claude sola-status hooks"
        );
    }
    self::log::follow_local(sender());
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(hook_stream)
}

fn hook_stream() -> impl Stream<Item = Event> {
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
