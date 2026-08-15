//! Single iced chrome process.
//!
//! A second `sola-browser` (MIME / `solactl open` / launcher) must not open
//! another window: that process used to `reap_stale_browser_procs` and kill
//! the live CEF helpers, leaving the first window painted from a parked
//! last-frame with a dead engine (reload and clicks do nothing).
//!
//! Primary chrome binds `chrome.sock`. Everyone else writes one line and exits.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::profiles;

const ACTIVATE: &str = "@activate";

static HANDOFF_TX: OnceLock<Sender<Handoff>> = OnceLock::new();
static HANDOFF_RX: OnceLock<Mutex<Receiver<Handoff>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum Handoff {
    OpenUrl(String),
    Activate,
}

/// RAII lock: listener stays bound for the chrome lifetime.
pub struct ChromeLock {
    sock: PathBuf,
    pid_path: PathBuf,
}

impl Drop for ChromeLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.sock);
        let _ = fs::remove_file(&self.pid_path);
        tracing::info!(path = %self.sock.display(), "chrome singleton released");
    }
}

pub fn chrome_sock_path() -> PathBuf {
    profiles::browser_data_root().join("chrome.sock")
}

pub fn chrome_pid_path() -> PathBuf {
    profiles::browser_data_root().join("chrome.pid")
}

/// Become the primary chrome, or signal that one is already up.
pub fn claim() -> Result<ChromeLock, ()> {
    let sock = chrome_sock_path();
    if let Some(parent) = sock.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match UnixListener::bind(&sock) {
        Ok(listener) => Ok(become_primary(listener, sock)),
        Err(e) => {
            tracing::info!(
                error = %e,
                path = %sock.display(),
                "chrome.sock busy — probing existing chrome"
            );
            if sock_is_live(&sock) {
                return Err(());
            }
            tracing::warn!(
                path = %sock.display(),
                "stale chrome.sock (nothing listening) — taking over"
            );
            let _ = fs::remove_file(&sock);
            match UnixListener::bind(&sock) {
                Ok(listener) => Ok(become_primary(listener, sock)),
                Err(e) => {
                    tracing::warn!(error = %e, "chrome.sock re-bind failed");
                    Err(())
                }
            }
        }
    }
}

fn become_primary(listener: UnixListener, sock: PathBuf) -> ChromeLock {
    let pid_path = chrome_pid_path();
    let _ = fs::write(&pid_path, std::process::id().to_string());
    install_channel();
    let _ = listener.set_nonblocking(false);
    thread::Builder::new()
        .name("chrome-handoff".into())
        .spawn(move || accept_loop(listener))
        .expect("spawn chrome-handoff");
    tracing::info!(
        pid = std::process::id(),
        path = %sock.display(),
        "chrome singleton acquired"
    );
    ChromeLock { sock, pid_path }
}

fn install_channel() {
    let (tx, rx) = mpsc::channel();
    let _ = HANDOFF_TX.set(tx);
    let _ = HANDOFF_RX.set(Mutex::new(rx));
}

fn accept_loop(listener: UnixListener) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(h) = read_handoff(stream) {
                    tracing::info!(?h, "chrome handoff received");
                    if let Some(tx) = HANDOFF_TX.get() {
                        let _ = tx.send(h);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "chrome handoff accept failed");
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn read_handoff(stream: UnixStream) -> Option<Handoff> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let line = line.trim();
    if line.is_empty() || line == ACTIVATE {
        Some(Handoff::Activate)
    } else {
        Some(Handoff::OpenUrl(line.to_string()))
    }
}

fn sock_is_live(sock: &std::path::Path) -> bool {
    UnixStream::connect(sock).is_ok()
}

/// Tell the running chrome to open `url` (or just focus if `None`).
pub fn handoff(url: Option<&str>) -> Result<(), String> {
    let sock = chrome_sock_path();
    let mut stream =
        UnixStream::connect(&sock).map_err(|e| format!("connect {}: {e}", sock.display()))?;
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let line = match url.map(str::trim).filter(|s| !s.is_empty()) {
        Some(u) => format!("{u}\n"),
        None => format!("{ACTIVATE}\n"),
    };
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("write handoff: {e}"))?;
    tracing::info!(
        url = url.unwrap_or(ACTIVATE),
        "handed off to existing chrome"
    );
    Ok(())
}

/// Drain URLs / activate requests from other sola-browser processes (UI Tick).
pub fn try_recv_handoff() -> Option<Handoff> {
    let lock = HANDOFF_RX.get()?;
    let rx = lock.lock().ok()?;
    match rx.try_recv() {
        Ok(h) => Some(h),
        Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_line_round_trip() {
        assert!(matches!(parse_line(""), Handoff::Activate));
        assert!(matches!(parse_line(ACTIVATE), Handoff::Activate));
        match parse_line("https://wiki.example") {
            Handoff::OpenUrl(u) => assert_eq!(u, "https://wiki.example"),
            other => panic!("{other:?}"),
        }
    }

    fn parse_line(line: &str) -> Handoff {
        let line = line.trim();
        if line.is_empty() || line == ACTIVATE {
            Handoff::Activate
        } else {
            Handoff::OpenUrl(line.to_string())
        }
    }
}
