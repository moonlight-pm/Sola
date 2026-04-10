use std::collections::HashSet;
use std::ffi::{CString, OsStr};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE_MS: u64 = 500;

/// Watch a directory of binaries for changes.
///
/// When any watched binary is modified, sends its name through the channel.
/// The caller is responsible for restarting the appropriate process (or
/// execv'ing if the changed binary is sola itself).
pub fn watch_binaries(bin_dir: &Path, names: &[&str], tx: mpsc::Sender<String>) {
    let watched: HashSet<String> = names.iter().map(|n| n.to_string()).collect();
    let bin_dir = bin_dir.to_path_buf();

    let (event_tx, event_rx) = mpsc::channel::<notify::Event>();

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

    if let Err(err) = watcher.watch(&bin_dir, RecursiveMode::NonRecursive) {
        tracing::warn!(?err, path = %bin_dir.display(), "failed to watch directory");
        return;
    }

    tracing::info!(path = %bin_dir.display(), "watching binaries for changes");

    std::thread::spawn(move || {
        let _watcher = watcher; // keep alive

        loop {
            let event = match event_rx.recv() {
                Ok(ev) => ev,
                Err(_) => return,
            };

            let changed = match extract_binary_name(&event, &watched) {
                Some(name) => name,
                None => continue,
            };

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

            tracing::info!(binary = %changed, "binary changed on disk");
            if tx.send(changed).is_err() {
                return; // receiver dropped
            }
        }
    });
}

/// Replace the current process with a fresh copy of itself.
pub fn exec_self() -> ! {
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

fn extract_binary_name(event: &notify::Event, watched: &HashSet<String>) -> Option<String> {
    use notify::EventKind;
    match &event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
        _ => return None,
    }
    for path in &event.paths {
        if let Some(name) = path.file_name().and_then(OsStr::to_str) {
            if watched.contains(name) {
                return Some(name.to_string());
            }
        }
    }
    None
}
