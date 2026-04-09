/// Binary watcher — restarts the compositor when the binary is replaced.
///
/// Runs in a separate thread, polling the binary's modification time.
/// When the binary changes (e.g., after `cargo make deploy canto`), the
/// compositor replaces itself with the new binary via `execv`.
///
/// This is intentionally independent of the calloop event loop — if the
/// compositor stalls or deadlocks, the watcher still works.
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Start watching the current binary for changes in a background thread.
///
/// Checks the binary's mtime every 2 seconds. When it changes,
/// replaces the current process with the new binary.
pub fn watch_binary() {
    let binary_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(?err, "could not determine binary path, skipping watcher");
            return;
        }
    };

    let initial_mtime = file_mtime(&binary_path);
    tracing::info!(?binary_path, "watching binary for changes");

    std::thread::spawn(move || {
        let mut last_mtime = initial_mtime;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let current_mtime = file_mtime(&binary_path);
            if current_mtime != last_mtime {
                tracing::info!("binary changed on disk, restarting");
                restart(&binary_path);
            }
            last_mtime = current_mtime;
        }
    });
}

fn file_mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Replace the current process with the binary at `path`.
///
/// Uses the Unix `exec` syscall via `Command::exec()` — the current
/// process image is replaced entirely. This function does not return
/// on success.
fn restart(path: &PathBuf) -> ! {
    use std::os::unix::process::CommandExt;

    let args: Vec<String> = std::env::args().collect();

    // `Command::exec()` calls execvp — replaces the process, no shell involved.
    let err = std::process::Command::new(path).args(&args[1..]).exec();

    tracing::error!(?err, "failed to restart, exiting");
    std::process::exit(1);
}
