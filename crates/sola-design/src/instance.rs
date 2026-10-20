//! One Design window. A second launch raises the live one and exits —
//! two chrome processes would start two CEF helpers on one profile.
//!
//! Same shape as sola-wrapper's singleton: a Unix socket the primary
//! listens on; `@activate` asks it to raise. The accept thread wakes iced
//! through `chrome_wake`, so the primary has no polling timer.

use std::io::{self, BufRead, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const ACTIVATE: &str = "@activate";

static ACTIVATE_PENDING: AtomicBool = AtomicBool::new(false);

pub enum Claim {
    Primary,
    Handoff,
}

fn lock_path() -> PathBuf {
    sola_core::env::runtime_dir().join("sola").join("design.sock")
}

pub fn claim() -> Claim {
    let path = lock_path();
    match bind_lock(&path) {
        Ok(listener) => {
            std::thread::Builder::new()
                .name("design-handoff".into())
                .spawn(move || accept_loop(listener))
                .expect("spawn design-handoff");
            Claim::Primary
        }
        Err(e) => {
            tracing::info!(path = %path.display(), error = %e, "design already running — will raise");
            Claim::Handoff
        }
    }
}

pub fn handoff() -> Result<(), String> {
    let path = lock_path();
    let mut stream =
        UnixStream::connect(&path).map_err(|e| format!("connect {}: {e}", path.display()))?;
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream
        .write_all(format!("{ACTIVATE}\n").as_bytes())
        .map_err(|e| format!("write handoff: {e}"))
}

/// A second launch asked the primary to raise (consumes the request).
pub fn take_activate() -> bool {
    ACTIVATE_PENDING.swap(false, Ordering::AcqRel)
}

fn accept_loop(listener: UnixListener) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let mut line = String::new();
        if io::BufReader::new(stream).read_line(&mut line).is_ok() && line.trim() == ACTIVATE {
            ACTIVATE_PENDING.store(true, Ordering::Release);
            sola_browser::chrome_wake::wake();
        }
    }
}

/// Bind, replacing a stale socket nobody answers on.
fn bind_lock(path: &Path) -> io::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                return Err(e);
            }
            let _ = std::fs::remove_file(path);
            UnixListener::bind(path)
        }
        Err(e) => Err(e),
    }
}
