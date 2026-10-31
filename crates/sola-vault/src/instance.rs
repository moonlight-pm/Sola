//! One Vault window. The first process binds
//! `$XDG_RUNTIME_DIR/sola/vault.sock` for its life; a later
//! `sola-vault [--item <id>]` (the daemon's `vault.open`, the launcher)
//! cannot bind, sends its item there and exits. The live one comes forward
//! and opens the item. A line of its own is the item id; an empty line
//! only raises the window.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use iced::Subscription;
use iced::futures::Stream;

static LISTENER: Mutex<Option<UnixListener>> = Mutex::new(None);

const LOCK_NAME: &str = "vault.sock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Primary,
    Handoff,
}

pub fn claim() -> Claim {
    let path = lock_path();
    match bind_lock(&path) {
        Ok(listener) => {
            *LISTENER.lock().unwrap_or_else(|p| p.into_inner()) = Some(listener);
            Claim::Primary
        }
        Err(e) => {
            tracing::info!(path = %path.display(), error = %e, "vault already open; handing off");
            Claim::Handoff
        }
    }
}

/// `--item <id>` or `--item=<id>` on the command line.
pub fn item_arg() -> Option<String> {
    item_from(std::env::args().skip(1))
}

fn item_from(args: impl IntoIterator<Item = String>) -> Option<String> {
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        if a == "--item" {
            return args.next().filter(|s| !s.trim().is_empty());
        }
        if let Some(id) = a.strip_prefix("--item=") {
            return (!id.trim().is_empty()).then(|| id.to_string());
        }
    }
    None
}

/// Send `item` (or nothing: raise only) to the live Vault.
pub fn handoff(item: Option<&str>) -> Result<(), String> {
    let path = lock_path();
    let mut stream =
        UnixStream::connect(&path).map_err(|e| format!("connect {}: {e}", path.display()))?;
    let line = format!("{}\n", item.unwrap_or_default());
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// One per later launch: the item it named, if any.
pub fn subscription() -> Subscription<Option<String>> {
    Subscription::run(stream)
}

fn stream() -> impl Stream<Item = Option<String>> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded();
    let listener = LISTENER.lock().unwrap_or_else(|p| p.into_inner()).take();
    match listener {
        Some(listener) => {
            let spawned = std::thread::Builder::new()
                .name("vault-handoff".into())
                .spawn(move || {
                    for conn in listener.incoming() {
                        let Ok(mut conn) = conn else { continue };
                        let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut body = String::new();
                        if conn.read_to_string(&mut body).is_err() || body.is_empty() {
                            continue;
                        }
                        let item = body.lines().map(str::trim).find(|l| !l.is_empty());
                        if tx.unbounded_send(item.map(str::to_string)).is_err() {
                            break;
                        }
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "could not start the handoff listener");
            }
        }
        None => tracing::warn!("vault handoff listener already taken"),
    }
    rx
}

fn lock_path() -> PathBuf {
    sola_core::env::runtime_dir().join("sola").join(LOCK_NAME)
}

fn bind_lock(path: &Path) -> io::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                return Err(e);
            }
            // A Vault that crashed or re-exec'd left it behind.
            let _ = fs::remove_file(path);
            UnixListener::bind(path)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_item_argument_in_both_spellings() {
        assert_eq!(item_from(args(&["--item", "abc"])).as_deref(), Some("abc"));
        assert_eq!(item_from(args(&["--item=abc"])).as_deref(), Some("abc"));
        assert_eq!(item_from(args(&["--item"])), None);
        assert_eq!(item_from(args(&["--item="])), None);
        assert_eq!(item_from(args(&[])), None);
    }

    #[test]
    fn second_bind_fails_while_first_lives() {
        let dir = std::env::temp_dir().join(format!("sola-vault-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join(LOCK_NAME);
        let first = bind_lock(&path).expect("first bind");
        assert!(bind_lock(&path).is_err());
        drop(first);
        bind_lock(&path).expect("stale socket replaced");
        let _ = fs::remove_dir_all(&dir);
    }
}
