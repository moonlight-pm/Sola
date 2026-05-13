//! File-change watchers and in-place self-restart for Sola binaries.
//!
//! Two entry points:
//! - [`watch_binaries`] watches a directory and reports changes to a set
//!   of named files through an mpsc channel — used by the process
//!   manager to restart managed children when their binary is replaced.
//! - [`watch_own_binary`] watches the current executable and
//!   [`exec_self`]s when it changes — used by sola-app so each running
//!   app reloads on redeploy.
//!
//! Both share the same `notify` + debounce plumbing; only the match
//! predicate and the on-match action differ.

use std::collections::HashSet;
use std::ffi::{CString, OsStr};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

/// Time to wait after a change event to let rsync temp-file + rename
/// settle before acting. Too short and we race the writer; too long and
/// the user notices the restart lag.
const DEBOUNCE_MS: u64 = 500;

/// Watch a directory of binaries for changes.
///
/// Whenever any file whose basename appears in `names` is created,
/// modified, or removed, the basename is sent through `tx` after a
/// debounce settle. The caller is responsible for what to do with the
/// name (kill/respawn a child, exec_self, etc.).
pub fn watch_binaries(bin_dir: &Path, names: &[&str], tx: mpsc::Sender<String>) {
    let watched: HashSet<String> = names.iter().map(|n| n.to_string()).collect();
    spawn_watch_thread(
        bin_dir,
        move |ev| extract_watched_name(ev, &watched),
        move |name| tx.send(name).is_ok(),
    );
}

/// Watch the current process's binary and [`exec_self`] when it changes.
///
/// Spawns a background thread. No-op if we can't resolve the binary
/// path or start the watcher — caller keeps running.
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

    let match_name = bin_name.clone();
    spawn_watch_thread(
        &bin_dir,
        move |ev| {
            ev.paths
                .iter()
                .any(|p| {
                    p.file_name()
                        .and_then(OsStr::to_str)
                        .is_some_and(|n| n == match_name)
                })
                .then(|| match_name.clone())
        },
        move |_| {
            tracing::info!(binary = %bin_name, "binary changed on disk, restarting");
            exec_self();
        },
    );
}

/// Replace the current process with a fresh copy of itself (`execv`).
/// Trims the `" (deleted)"` suffix Linux appends to `/proc/self/exe`
/// after the file has been replaced.
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

/// Spawn a background thread that owns a non-recursive `notify` watcher
/// on `dir`. For each create/modify/remove event, it calls `predicate`
/// to produce a payload to hand to `action`; if the action returns
/// false (or the channel closes), the thread exits. A 500ms debounce
/// drains further events after the first match to avoid firing twice
/// for rsync's write+rename.
fn spawn_watch_thread<P, A>(dir: &Path, predicate: P, mut action: A)
where
    P: Fn(&notify::Event) -> Option<String> + Send + 'static,
    A: FnMut(String) -> bool + Send + 'static,
{
    let dir = dir.to_path_buf();
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

    if let Err(err) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        tracing::warn!(?err, path = %dir.display(), "failed to watch directory");
        return;
    }

    tracing::info!(path = %dir.display(), "watching for binary changes");

    thread::spawn(move || {
        let _watcher = watcher; // keep alive for the thread lifetime

        loop {
            let event = match event_rx.recv() {
                Ok(ev) => ev,
                Err(_) => return,
            };

            let Some(name) = event_kind_matches(&event)
                .then(|| predicate(&event))
                .flatten()
            else {
                continue;
            };

            // Debounce: drain further events until the deadline.
            let deadline = Instant::now() + Duration::from_millis(DEBOUNCE_MS);
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                if event_rx.recv_timeout(remaining).is_err() {
                    break;
                }
            }

            tracing::info!(binary = %name, "binary changed on disk");
            if !action(name) {
                return;
            }
        }
    });
}

/// True for create/modify/remove events — the kinds that indicate a
/// binary has been swapped out. Attribute/access events don't matter.
fn event_kind_matches(event: &notify::Event) -> bool {
    use notify::EventKind;
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

/// If the event touches any file whose basename is in `watched`, return
/// that basename. Used by [`watch_binaries`].
fn extract_watched_name(event: &notify::Event, watched: &HashSet<String>) -> Option<String> {
    for path in &event.paths {
        if let Some(name) = path.file_name().and_then(OsStr::to_str)
            && watched.contains(name)
        {
            return Some(name.to_string());
        }
    }
    None
}
