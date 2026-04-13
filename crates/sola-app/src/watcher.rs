use std::ffi::CString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE_MS: u64 = 500;

/// Watch the current process's binary and exec_self when it changes on disk.
///
/// Spawns a background thread. When the binary is replaced (e.g. by rsync),
/// the process re-executes itself. This never returns on success — the current
/// process image is replaced by a fresh one.
pub fn watch_own_binary() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(?err, "cannot resolve binary path, skipping self-watch");
            return;
        }
    };

    let bin_dir = match exe.parent() {
        Some(d) => d.to_path_buf(),
        None => {
            tracing::warn!("binary has no parent directory, skipping self-watch");
            return;
        }
    };

    let bin_name = match exe.file_name() {
        Some(n) => n.to_string_lossy().to_string(),
        None => {
            tracing::warn!("binary has no file name, skipping self-watch");
            return;
        }
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
            tracing::warn!(?err, "failed to create file watcher, skipping self-watch");
            return;
        }
    };

    if let Err(err) = watcher.watch(&bin_dir, RecursiveMode::NonRecursive) {
        tracing::warn!(?err, path = %bin_dir.display(), "failed to watch binary directory");
        return;
    }

    tracing::info!(binary = %bin_name, "watching for binary changes");

    std::thread::spawn(move || {
        let _watcher = watcher; // keep alive

        loop {
            let event = match event_rx.recv() {
                Ok(ev) => ev,
                Err(_) => return,
            };

            // Only react to events for our binary
            if !event_matches_binary(&event, &bin_name) {
                continue;
            }

            // Debounce: wait for rsync temp + rename to settle
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

            tracing::info!(binary = %bin_name, "binary changed on disk, restarting");
            exec_self();
        }
    });
}

/// Replace the current process with a fresh copy of itself.
fn exec_self() -> ! {
    let mut exe = std::env::current_exe().expect("cannot resolve binary for restart");

    // On Linux, /proc/self/exe gets " (deleted)" when the binary is replaced.
    let path_str = exe.to_string_lossy();
    if path_str.ends_with(" (deleted)") {
        exe = PathBuf::from(path_str.trim_end_matches(" (deleted)"));
    }

    let exe_cstr = CString::new(exe.as_os_str().as_encoded_bytes().to_vec())
        .expect("binary path contains null byte");

    let args: Vec<CString> = std::env::args()
        .map(|a| CString::new(a).expect("arg contains null byte"))
        .collect();

    tracing::info!(path = %exe.display(), "execv (self-restart)");

    match nix::unistd::execv(&exe_cstr, &args) {
        Ok(infallible) => match infallible {},
        Err(err) => panic!("execv failed: {err}"),
    }
}

fn event_matches_binary(event: &notify::Event, bin_name: &str) -> bool {
    use notify::EventKind;
    match &event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
        _ => return false,
    }
    event.paths.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == bin_name)
    })
}
