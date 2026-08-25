//! One iced chrome per wrapper id. A second spawn raises the live window.

use std::fs;
use std::io::{self, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

const ACTIVATE: &str = "@activate";

static HANDOFF_TX: Mutex<Option<Sender<()>>> = Mutex::new(None);
static HANDOFF_RX: Mutex<Option<Receiver<()>>> = Mutex::new(None);
static ACCEPTING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Primary,
    Handoff,
}

pub fn lock_path(id: &str) -> PathBuf {
    sola_core::env::runtime_dir()
        .join("sola")
        .join("wrapper")
        .join(format!("{id}.sock"))
}

pub fn claim(id: &str) -> Claim {
    let path = lock_path(id);
    match bind_lock(&path) {
        Ok(listener) => {
            become_primary(listener);
            tracing::info!(path = %path.display(), %id, "wrapper singleton claimed");
            Claim::Primary
        }
        Err(e) => {
            tracing::info!(
                path = %path.display(),
                error = %e,
                %id,
                "wrapper already running — will raise"
            );
            Claim::Handoff
        }
    }
}

pub fn handoff(id: &str) -> Result<(), String> {
    let sock = lock_path(id);
    let mut stream =
        UnixStream::connect(&sock).map_err(|e| format!("connect {}: {e}", sock.display()))?;
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream
        .write_all(format!("{ACTIVATE}\n").as_bytes())
        .map_err(|e| format!("write handoff: {e}"))?;
    tracing::info!(%id, "handed off to existing wrapper");
    Ok(())
}

pub fn try_recv_activate() -> bool {
    let Ok(guard) = HANDOFF_RX.lock() else {
        return false;
    };
    let Some(rx) = guard.as_ref() else {
        return false;
    };
    match rx.try_recv() {
        Ok(()) => true,
        Err(TryRecvError::Empty | TryRecvError::Disconnected) => false,
    }
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
        .name("wrapper-handoff".into())
        .spawn(move || accept_loop(listener, tx))
        .expect("spawn wrapper-handoff");
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
    fn lock_path_is_per_id() {
        let a = lock_path("slack");
        let b = lock_path("discord");
        assert_ne!(a, b);
        assert!(a.to_string_lossy().ends_with("wrapper/slack.sock"));
    }
}
