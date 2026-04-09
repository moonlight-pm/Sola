/// Binary watcher — triggers a graceful restart when the binary is replaced.
///
/// Uses `notify` (inotify on Linux) to watch the binary's parent directory.
/// When the binary changes, sets a flag that the main loop checks. The main
/// loop then performs a graceful shutdown (dropping XWayland, Wayland socket,
/// DRM devices) before execv'ing the new binary.
///
/// This ensures X11 lock files and Wayland sockets are properly cleaned up
/// so the new process can bind to the same display numbers.
use std::ffi::{CString, OsStr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE_MS: u64 = 500;

/// Start watching the running binary for replacement.
///
/// When a change is detected, sets `restart_flag` to `true`. The main loop
/// should check this flag and initiate a graceful shutdown + execv.
pub fn watch_binary(restart_flag: Arc<AtomicBool>) {
    let exe = match std::env::current_exe().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(?err, "cannot resolve binary path, skipping watcher");
            return;
        }
    };

    let parent = match exe.parent() {
        Some(p) => p.to_path_buf(),
        None => return,
    };

    let binary_name = match exe.file_name() {
        Some(n) => n.to_os_string(),
        None => return,
    };

    let (event_tx, event_rx) = std::sync::mpsc::channel::<notify::Event>();

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = event_tx.send(event);
            }
        },
        Config::default(),
    ) {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!(?err, "failed to create file watcher");
            return;
        }
    };

    if let Err(err) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
        tracing::warn!(?err, "failed to watch directory");
        return;
    }

    tracing::info!(path = %exe.display(), "watching binary for changes");

    std::thread::spawn(move || {
        let _watcher = watcher; // keep alive

        loop {
            let event = match event_rx.recv() {
                Ok(ev) => ev,
                Err(_) => return,
            };

            if !event_matches_binary(&event, &binary_name) {
                continue;
            }

            // Debounce: wait for rsync temp + rename to settle.
            let deadline = Instant::now() + Duration::from_millis(DEBOUNCE_MS);
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match event_rx.recv_timeout(remaining) {
                    Ok(_) => {}
                    Err(_) => break,
                }
            }

            tracing::info!("binary changed on disk, requesting restart");
            restart_flag.store(true, Ordering::Relaxed);
            return;
        }
    });
}

/// Replace the current process with a fresh copy of the binary.
///
/// Should be called AFTER graceful shutdown (XWayland, Wayland socket, DRM
/// devices all dropped) so lock files and sockets are released.
pub fn exec_new_binary() -> ! {
    let mut exe =
        std::env::current_exe().expect("cannot resolve binary for restart");

    // On Linux, /proc/self/exe gets " (deleted)" when the binary is replaced.
    let path_str = exe.to_string_lossy();
    if path_str.ends_with(" (deleted)") {
        exe = std::path::PathBuf::from(path_str.trim_end_matches(" (deleted)"));
    }

    let exe_cstr = CString::new(exe.as_os_str().as_encoded_bytes().to_vec())
        .expect("binary path contains null byte");

    let args: Vec<CString> = std::env::args()
        .map(|a| CString::new(a).expect("arg contains null byte"))
        .collect();

    tracing::info!(path = %exe.display(), "execv");

    match nix::unistd::execv(&exe_cstr, &args) {
        Ok(infallible) => match infallible {},
        Err(err) => panic!("execv failed: {err}"),
    }
}

fn event_matches_binary(event: &notify::Event, binary_name: &OsStr) -> bool {
    use notify::EventKind;
    match &event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
        _ => return false,
    }
    event
        .paths
        .iter()
        .any(|p| p.file_name() == Some(binary_name))
}
