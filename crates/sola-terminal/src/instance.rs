//! One Terminal process. Its tabs are windows in the shell's tab bar, and a
//! second process would open every saved tab again. The first process
//! holds a socket; a later launch finds it, asks for a new tab over the bus
//! (`MenuAction` `new_tab`), and exits.

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

fn lock_path() -> PathBuf {
    sola_core::env::runtime_dir().join("sola").join("terminal.sock")
}

/// True for the first Terminal process, which now holds the socket.
pub fn claim() -> bool {
    let path = lock_path();
    match bind(&path) {
        Ok(listener) => {
            // Accept and drop, so waiting launches never fill the backlog.
            let spawned = std::thread::Builder::new()
                .name("terminal-instance".into())
                .spawn(move || {
                    for stream in listener.incoming() {
                        drop(stream);
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!("terminal instance thread: {e}");
            }
            true
        }
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            tracing::info!(path = %path.display(), "terminal already running");
            false
        }
        Err(e) => {
            // Cannot tell; run rather than lose the launch.
            tracing::warn!(path = %path.display(), "terminal instance socket: {e}");
            true
        }
    }
}

/// Bind, replacing a stale socket nobody answers on.
fn bind(path: &Path) -> io::Result<UnixListener> {
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
