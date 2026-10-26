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
use std::time::{Duration, Instant};

use crate::profiles;

const ACTIVATE: &str = "@activate";
/// `@app <id>`: open (or bring up) web app `<id>`.
const OPEN_APP: &str = "@app";

static HANDOFF_TX: OnceLock<Sender<Handoff>> = OnceLock::new();
static HANDOFF_RX: OnceLock<Mutex<Receiver<Handoff>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum Handoff {
    OpenUrl(String),
    /// A bare launch: open a new page window.
    Launch,
    /// Open web app `<id>`'s window, or bring it up.
    OpenApp(String),
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
                        crate::chrome_wake::wake();
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
    parse_line(&line)
}

/// One handoff line. Connect-only (no line) is a liveness probe —
/// `chrome_is_running` and `sock_is_live` just connect and drop. Treating
/// that as `@activate` made every shell OpenUrl check queue a raise, which
/// re-probed the sock and flooded the chrome.
fn parse_line(line: &str) -> Option<Handoff> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    if line == ACTIVATE {
        Some(Handoff::Launch)
    } else if let Some(rest) = line
        .strip_prefix(OPEN_APP)
        .filter(|r| r.is_empty() || r.starts_with(' '))
    {
        let id = rest.trim();
        (!id.is_empty()).then(|| Handoff::OpenApp(id.to_string()))
    } else {
        Some(Handoff::OpenUrl(line.to_string()))
    }
}

/// Tell the running chrome to open web app `app_id`.
pub fn handoff_app(app_id: &str) -> Result<(), String> {
    let sock = chrome_sock_path();
    let mut stream =
        UnixStream::connect(&sock).map_err(|e| format!("connect {}: {e}", sock.display()))?;
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream
        .write_all(format!("{OPEN_APP} {app_id}\n").as_bytes())
        .map_err(|e| format!("write handoff: {e}"))?;
    tracing::info!(%app_id, "handed a web app off to the existing chrome");
    Ok(())
}

/// A primary chrome is up and listening.
pub fn chrome_is_live() -> bool {
    sock_is_live(&chrome_sock_path())
}

/// How long `--app` waits for the browser it asked the session for.
const BROWSER_START_WAIT: Duration = Duration::from_secs(20);

/// `--app <id>` with no browser up: have the session start the browser as
/// itself, then hand it the app once it listens. The session then tracks,
/// restores and closes the browser as the browser — were this process the
/// browser, closing the web app would stop every page and app with it, and
/// the next login would start `--app <id>` instead. False when there is no
/// session to ask, or the browser did not come up (this process goes on
/// to claim the chrome itself).
pub fn start_browser_for_app(app_id: &str) -> bool {
    use sola_bus::topics::{LaunchAppPayload, Topic};
    let mut bus = sola_bus::BusClient::new();
    if bus.connect().is_err() {
        return false;
    }
    let command = sola_core::paths::bin_path("sola-browser").display().to_string();
    let launch = Topic::LaunchApp(LaunchAppPayload {
        app_id: "sola-browser".into(),
        command,
    });
    if bus.emit(launch).is_err() {
        return false;
    }
    let deadline = Instant::now() + BROWSER_START_WAIT;
    while Instant::now() < deadline {
        if chrome_is_live() {
            return handoff_app(app_id).is_ok();
        }
        thread::sleep(Duration::from_millis(100));
    }
    tracing::warn!(%app_id, "the session's browser did not come up — starting here");
    false
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
    let normalized = url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(crate::util::normalize_url);
    let line = match normalized.as_deref() {
        Some(u) => format!("{u}\n"),
        None => format!("{ACTIVATE}\n"),
    };
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("write handoff: {e}"))?;
    tracing::info!(
        url = normalized.as_deref().unwrap_or(ACTIVATE),
        "handed off to existing chrome"
    );
    Ok(())
}

/// Drain URLs / activate requests from other sola-browser processes.
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
        assert!(parse_line("").is_none(), "empty connect is a probe");
        assert!(matches!(parse_line(ACTIVATE), Some(Handoff::Launch)));
        match parse_line("https://wiki.example") {
            Some(Handoff::OpenUrl(u)) => assert_eq!(u, "https://wiki.example"),
            other => panic!("{other:?}"),
        }
        match parse_line("@app web-1\n") {
            Some(Handoff::OpenApp(id)) => assert_eq!(id, "web-1"),
            other => panic!("{other:?}"),
        }
        assert!(parse_line("@app ").is_none());
    }
}
