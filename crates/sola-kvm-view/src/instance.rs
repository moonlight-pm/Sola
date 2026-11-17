//! One Remote Desktop: the first process binds
//! `$XDG_RUNTIME_DIR/sola/remote-desktop.sock` for its lifetime. A later
//! launch (the launcher, `sola-kvm view <host>`) cannot bind, writes the
//! machines it names there, one a line, and exits; the running one opens
//! them, or with none, its Connections window.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use iced::Subscription;
use iced::futures::Stream;

/// Bound by [`claim`]; moved into the accept thread by [`subscription`].
static LISTENER: Mutex<Option<UnixListener>> = Mutex::new(None);

/// Try to be the one Remote Desktop: `true` when this process is.
pub fn claim() -> bool {
    let path = lock_path();
    match bind_lock(&path) {
        Ok(listener) => {
            *LISTENER.lock().unwrap_or_else(|p| p.into_inner()) = Some(listener);
            true
        }
        Err(e) => {
            tracing::info!(path = %path.display(), error = %e, "Remote Desktop is running; handing over");
            false
        }
    }
}

/// Send `targets` to the running Remote Desktop. None sends one empty line,
/// which brings up its Connections window.
pub fn handoff(targets: &[String]) -> Result<(), String> {
    let path = lock_path();
    let mut stream = UnixStream::connect(&path).map_err(|e| format!("connect {}: {e}", path.display()))?;
    let mut body = targets.join("\n");
    body.push('\n');
    stream
        .write_all(body.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// One item per handoff: the machines it named (empty: the list).
pub fn subscription() -> Subscription<Vec<String>> {
    Subscription::run(handoff_stream)
}

fn handoff_stream() -> impl Stream<Item = Vec<String>> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded();
    let listener = LISTENER.lock().unwrap_or_else(|p| p.into_inner()).take();
    match listener {
        Some(listener) => {
            let spawned = std::thread::Builder::new()
                .name("remote-desktop-handoff".into())
                .spawn(move || {
                    for conn in listener.incoming() {
                        let Ok(mut conn) = conn else { continue };
                        let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut body = String::new();
                        // Writing nothing, a connect is a probe.
                        if conn.read_to_string(&mut body).is_err() || body.is_empty() {
                            continue;
                        }
                        let targets = body
                            .lines()
                            .map(str::trim)
                            .filter(|l| !l.is_empty())
                            .map(String::from)
                            .collect();
                        if tx.unbounded_send(targets).is_err() {
                            break;
                        }
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!(%e, "Remote Desktop: no handoff listener");
            }
        }
        None => tracing::warn!("Remote Desktop: the handoff listener is already taken"),
    }
    rx
}

fn lock_path() -> PathBuf {
    sola_core::env::runtime_dir().join("sola").join("remote-desktop.sock")
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
            // Left by one that crashed or re-executed.
            let _ = fs::remove_file(path);
            UnixListener::bind(path)
        }
        Err(e) => Err(e),
    }
}
