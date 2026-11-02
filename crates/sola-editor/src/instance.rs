//! One editor per desk: the first process holds a socket. A later one finds
//! it taken and hands its files over.

use std::fs;
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;

/// Held for the life of the process.
static LOCK: Mutex<Option<UnixListener>> = Mutex::new(None);

fn lock_path() -> PathBuf {
    sola_core::env::runtime_dir().join("sola").join("editor.lock.sock")
}

/// Another editor holds the lock.
pub fn live() -> bool {
    UnixStream::connect(lock_path()).is_ok()
}

/// Try to be the editor. False when one is already running.
pub fn claim() -> bool {
    let path = lock_path();
    let bound = (|| {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match UnixListener::bind(&path) {
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                if UnixStream::connect(&path).is_ok() {
                    return Err(e);
                }
                // Left by a crash.
                let _ = fs::remove_file(&path);
                UnixListener::bind(&path)
            }
            other => other,
        }
    })();
    match bound {
        Ok(listener) => {
            // Drain the probes, so the backlog can't fill and look stale.
            if let Ok(probes) = listener.try_clone() {
                thread::spawn(move || {
                    for probe in probes.incoming() {
                        drop(probe);
                    }
                });
            }
            *LOCK.lock().unwrap_or_else(|p| p.into_inner()) = Some(listener);
            true
        }
        Err(e) => {
            tracing::info!(error = %e, "an editor is already running");
            false
        }
    }
}
