//! Single-instance claim + file handoff for sola-calendar.
//!
//! The first process binds `$XDG_RUNTIME_DIR/sola/calendar.sock` for its
//! lifetime. A later `sola-calendar [file.ics…]` (Mail's Open, `xdg-open`,
//! the launcher) cannot bind, writes its paths there, and exits — no
//! second window. The live one raises itself and drafts each file. A
//! connect that writes nothing is a liveness probe.

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

const LOCK_NAME: &str = "calendar.sock";

/// Outcome of trying to become the sole calendar process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// This process owns the socket; start the window.
    Primary,
    /// Another live calendar owns it; hand off argv and exit.
    Handoff,
}

/// Try to become the singleton. The socket is held until process exit.
pub fn claim() -> Claim {
    let path = lock_path();
    match bind_lock(&path) {
        Ok(listener) => {
            *LISTENER.lock().unwrap_or_else(|p| p.into_inner()) = Some(listener);
            tracing::info!(path = %path.display(), "calendar singleton claimed");
            Claim::Primary
        }
        Err(e) => {
            tracing::info!(
                path = %path.display(),
                error = %e,
                "calendar already running — will hand off"
            );
            Claim::Handoff
        }
    }
}

/// Files named on the command line, absolute (`file://` stripped).
pub fn launch_paths() -> Vec<PathBuf> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.trim().is_empty())
        .map(|a| {
            let p = PathBuf::from(a.strip_prefix("file://").unwrap_or(&a));
            if p.is_absolute() {
                p
            } else {
                std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p)
            }
        })
        .collect()
}

/// Send `paths` to the live calendar. A bare launch sends one empty line,
/// which only raises the window.
pub fn handoff(paths: &[PathBuf]) -> Result<(), String> {
    let path = lock_path();
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| format!("connect {}: {e}", path.display()))?;
    let mut body = paths
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    body.push('\n');
    stream
        .write_all(body.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(files = paths.len(), "handed off to running calendar");
    Ok(())
}

/// One item per handoff: the paths it carried (empty means raise only).
pub fn subscription() -> Subscription<Vec<PathBuf>> {
    Subscription::run(handoff_stream)
}

fn handoff_stream() -> impl Stream<Item = Vec<PathBuf>> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded();
    let listener = LISTENER.lock().unwrap_or_else(|p| p.into_inner()).take();
    match listener {
        Some(listener) => {
            std::thread::Builder::new()
                .name("calendar-handoff".into())
                .spawn(move || {
                    for conn in listener.incoming() {
                        let Ok(mut conn) = conn else { continue };
                        let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut body = String::new();
                        if conn.read_to_string(&mut body).is_err() || body.is_empty() {
                            continue;
                        }
                        let paths = body
                            .lines()
                            .map(str::trim)
                            .filter(|l| !l.is_empty())
                            .map(PathBuf::from)
                            .collect();
                        if tx.unbounded_send(paths).is_err() {
                            break;
                        }
                    }
                })
                .ok();
        }
        None => tracing::warn!("calendar handoff listener already taken"),
    }
    rx
}

fn lock_path() -> PathBuf {
    sola_core::env::runtime_dir().join("sola").join(LOCK_NAME)
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
            // Stale socket from a calendar that crashed or re-exec'd.
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
        let dir = std::env::temp_dir().join(format!("sola-cal-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join(LOCK_NAME);
        let first = bind_lock(&path).expect("first bind");
        assert!(bind_lock(&path).is_err());
        drop(first);
        bind_lock(&path).expect("stale socket replaced");
        let _ = fs::remove_dir_all(&dir);
    }
}
