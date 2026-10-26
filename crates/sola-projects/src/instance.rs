//! One Projects process. Its project windows are tabs in the shell's tab
//! bar, and a second process would open every project again on the same
//! tmux sessions. The first process holds a socket; a later launch finds
//! it, asks the running one over the bus to show its project
//! (`MenuAction` [`RAISE`]), and exits.

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// The action a second launch sends: show the selected project's window.
pub const RAISE: &str = "raise";

fn lock_path() -> PathBuf {
    sola_core::env::runtime_dir().join("sola").join("projects.sock")
}

/// True for the first Projects process, which now holds the socket.
pub fn claim() -> bool {
    let path = lock_path();
    match bind(&path) {
        Ok(listener) => {
            // Accept and drop, so waiting launches never fill the backlog.
            let spawned = std::thread::Builder::new()
                .name("projects-instance".into())
                .spawn(move || {
                    for stream in listener.incoming() {
                        drop(stream);
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!("projects instance thread: {e}");
            }
            true
        }
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            tracing::info!(path = %path.display(), "projects already running");
            false
        }
        Err(e) => {
            // Cannot tell; run rather than lose the launch.
            tracing::warn!(path = %path.display(), "projects instance socket: {e}");
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
