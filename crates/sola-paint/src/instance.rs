//! Single-instance claim + bus handoff for sola-paint.
//!
//! The first process binds `$XDG_RUNTIME_DIR/sola/paint.lock.sock` and
//! keeps the listener for its lifetime. A second `sola-paint` that cannot
//! bind (and can still connect) emits `Topic::OpenImage` at the live
//! instance and exits — no second window.

use std::fs;
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use sola_bus::BusClient;
use sola_bus::topics::{OpenImageRequest, Topic};

use crate::app::APP_ID;

/// Held so the accept thread is not the only owner we can name in tests.
static LOCK: Mutex<Option<UnixListener>> = Mutex::new(None);
static ACCEPTING: AtomicBool = AtomicBool::new(false);

const LOCK_NAME: &str = "paint.lock.sock";

/// Outcome of trying to become the sole paint process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// This process owns the lock; start the window.
    Primary,
    /// Another live paint owns the lock; hand off argv and exit.
    Handoff,
}

/// Try to become the singleton. Hold the listener until process exit.
pub fn claim() -> Claim {
    let path = lock_path();
    match bind_lock(&path) {
        Ok(listener) => {
            store_lock(listener);
            tracing::info!(path = %path.display(), "paint singleton claimed");
            Claim::Primary
        }
        Err(e) => {
            tracing::info!(
                path = %path.display(),
                error = %e,
                "paint already running — will hand off"
            );
            Claim::Handoff
        }
    }
}

/// Emit `OpenImage` for each path so the live instance opens them.
pub fn handoff(paths: &[PathBuf]) -> Result<(), String> {
    let mut client = BusClient::new();
    client.set_app_id(APP_ID);
    client
        .connect()
        .map_err(|e| format!("bus connect for paint handoff: {e}"))?;
    if paths.is_empty() {
        // Still poke the live app so a bare second launch can focus it.
        client
            .emit(Topic::OpenImage(OpenImageRequest {
                path: PathBuf::new(),
                activate: true,
                app_id: Some(APP_ID.into()),
            }))
            .map_err(|e| format!("emit OpenImage: {e}"))?;
        return Ok(());
    }
    for path in paths {
        client
            .emit(Topic::OpenImage(OpenImageRequest {
                path: path.clone(),
                activate: true,
                app_id: Some(APP_ID.into()),
            }))
            .map_err(|e| format!("emit OpenImage: {e}"))?;
    }
    // Give the writer a beat to flush before we drop the socket.
    std::thread::sleep(Duration::from_millis(20));
    Ok(())
}

pub fn lock_path() -> PathBuf {
    lock_path_in(&sola_core::env::runtime_dir())
}

fn lock_path_in(runtime: &Path) -> PathBuf {
    runtime.join("sola").join(LOCK_NAME)
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
            // Stale socket from a crashed paint.
            let _ = fs::remove_file(path);
            UnixListener::bind(path)
        }
        Err(e) => Err(e),
    }
}

fn store_lock(listener: UnixListener) {
    // Drain probe connects from later `sola-paint` processes so the
    // listen backlog cannot fill and look like a stale socket.
    ACCEPTING.store(true, Ordering::SeqCst);
    let probe = match listener.try_clone() {
        Ok(l) => l,
        Err(_) => {
            store_listener(listener);
            return;
        }
    };
    store_listener(listener);
    thread::Builder::new()
        .name("paint-singleton".into())
        .spawn(move || {
            while ACCEPTING.load(Ordering::Relaxed) {
                match probe.accept() {
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        })
        .ok();
}

fn store_listener(listener: UnixListener) {
    match LOCK.lock() {
        Ok(mut slot) => *slot = Some(listener),
        Err(poisoned) => *poisoned.into_inner() = Some(listener),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_bind_fails_while_first_lives() {
        let dir = std::env::temp_dir().join(format!("sola-paint-lock-{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("sola-paint-stale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sola")).unwrap();
        let path = lock_path_in(&dir);
        // Leave a dead socket file with no listener.
        let _ = UnixListener::bind(&path);
        // Drop by unlinking the inode's only live listener via replace:
        // bind_lock sees AddrInUse, connect fails, unlinks, rebinds.
        drop(UnixListener::bind(&path));
        // The path still exists if the previous bind left it; force a
        // leftover file with no process holding it.
        let _ = fs::remove_file(&path);
        fs::write(&path, b"").unwrap();
        bind_lock(&path).expect("replaced stale lock");
        let _ = fs::remove_dir_all(&dir);
    }
}
