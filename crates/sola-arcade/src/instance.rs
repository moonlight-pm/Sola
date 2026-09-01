//! Single-instance claim + raise for sola-arcade.
//!
//! The UI process binds `$XDG_RUNTIME_DIR/sola/arcade.lock.sock`. A second
//! `sola-arcade` (launcher / session restore) writes `@activate` and exits
//! so it cannot start a second Fit driver. `--run` / `--nested-steam` never
//! claim this lock.

use std::fs;
use std::io::{self, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use iced::Subscription;
use iced::futures::Stream;

const ACTIVATE: &str = "@activate";
const LOCK_NAME: &str = "arcade.lock.sock";

static HANDOFF_TX: Mutex<Option<Sender<()>>> = Mutex::new(None);
static HANDOFF_RX: Mutex<Option<Receiver<()>>> = Mutex::new(None);
static ACCEPTING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Primary,
    Handoff,
}

pub fn lock_path() -> PathBuf {
    lock_path_in(&sola_core::env::runtime_dir())
}

fn lock_path_in(runtime: &Path) -> PathBuf {
    runtime.join("sola").join(LOCK_NAME)
}

pub fn claim() -> Claim {
    let path = lock_path();
    match bind_lock(&path) {
        Ok(listener) => {
            become_primary(listener);
            tracing::info!(path = %path.display(), "arcade singleton claimed");
            Claim::Primary
        }
        Err(e) => {
            tracing::info!(
                path = %path.display(),
                error = %e,
                "arcade already running — will raise"
            );
            Claim::Handoff
        }
    }
}

pub fn handoff() -> Result<(), String> {
    let sock = lock_path();
    let mut stream =
        UnixStream::connect(&sock).map_err(|e| format!("connect {}: {e}", sock.display()))?;
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream
        .write_all(format!("{ACTIVATE}\n").as_bytes())
        .map_err(|e| format!("write handoff: {e}"))?;
    tracing::info!("handed off to existing arcade");
    Ok(())
}

pub fn subscription() -> Subscription<()> {
    Subscription::run(activate_stream)
}

fn activate_stream() -> impl Stream<Item = ()> {
    let rx_opt = match HANDOFF_RX.lock() {
        Ok(mut g) => g.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded();
    match rx_opt {
        Some(std_rx) => {
            thread::Builder::new()
                .name("arcade-activate-fwd".into())
                .spawn(move || {
                    while !iced_tx.is_closed() {
                        match std_rx.recv() {
                            Ok(()) => {
                                if iced_tx.unbounded_send(()).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
                .ok();
        }
        None => drop(iced_tx),
    }
    iced_rx
}

fn become_primary(listener: UnixListener) {
    let (tx, rx) = mpsc::channel();
    if let Ok(mut g) = HANDOFF_TX.lock() {
        *g = Some(tx.clone());
    }
    if let Ok(mut g) = HANDOFF_RX.lock() {
        *g = Some(rx);
    }
    ACCEPTING.store(true, Ordering::SeqCst);
    thread::Builder::new()
        .name("arcade-singleton".into())
        .spawn(move || accept_loop(listener, tx))
        .expect("spawn arcade-singleton");
}

fn accept_loop(listener: UnixListener, tx: Sender<()>) {
    while ACCEPTING.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if read_activate(stream) {
                    let _ = tx.send(());
                }
            }
            Err(_) => break,
        }
    }
}

fn read_activate(stream: UnixStream) -> bool {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return false;
    }
    line.trim() == ACTIVATE
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
            let _ = fs::remove_file(path);
            UnixListener::bind(path)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_bind_fails_while_first_lives() {
        let dir = std::env::temp_dir().join(format!("sola-arcade-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = lock_path_in(&dir);
        let first = bind_lock(&path).expect("first bind");
        assert!(bind_lock(&path).is_err());
        drop(first);
        let _ = fs::remove_file(&path);
        bind_lock(&path).expect("bind after drop");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_socket_is_replaced() {
        let dir = std::env::temp_dir().join(format!("sola-arcade-stale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sola")).unwrap();
        let path = lock_path_in(&dir);
        let _ = fs::remove_file(&path);
        fs::write(&path, b"").unwrap();
        bind_lock(&path).expect("replaced stale lock");
        let _ = fs::remove_dir_all(&dir);
    }
}
