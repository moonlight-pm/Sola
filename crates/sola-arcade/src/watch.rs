//! Debounced Steam `steamapps/` watch → silent library rescan.
//!
//! Non-recursive on each library's `steamapps/` so `compatdata/` / shader
//! noise does not fire. Coalesces ACF bursts (install/uninstall) with a
//! 1s debounce. The scan itself stays off the UI thread.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use iced::Subscription;
use iced::futures::Stream;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::steam::steamapps_watch_dirs;

const DEBOUNCE: Duration = Duration::from_secs(1);

static FIRE_TX: Mutex<Option<Sender<()>>> = Mutex::new(None);
static FIRE_RX: Mutex<Option<Receiver<()>>> = Mutex::new(None);
static DIRS_TX: Mutex<Option<Sender<Vec<PathBuf>>>> = Mutex::new(None);

/// True when a notify path is an install-state file Arcade already parses.
pub fn path_is_library_change(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    (name.starts_with("appmanifest_") && name.ends_with(".acf"))
        || name.eq_ignore_ascii_case("libraryfolders.vdf")
}

fn event_is_library_change(event: &Event) -> bool {
    use notify::EventKind;
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event.paths.iter().any(|p| path_is_library_change(p))
}

/// Arm the watcher (idempotent). Call once from UI boot.
pub fn start() {
    if FIRE_TX.lock().map(|g| g.is_some()).unwrap_or(true) {
        refresh_dirs();
        return;
    }
    let (fire_tx, fire_rx) = mpsc::channel();
    let (dirs_tx, dirs_rx) = mpsc::channel();
    if let Ok(mut g) = FIRE_TX.lock() {
        *g = Some(fire_tx.clone());
    }
    if let Ok(mut g) = FIRE_RX.lock() {
        *g = Some(fire_rx);
    }
    if let Ok(mut g) = DIRS_TX.lock() {
        *g = Some(dirs_tx);
    }
    thread::Builder::new()
        .name("arcade-lib-watch".into())
        .spawn(move || watch_loop(dirs_rx, fire_tx))
        .expect("spawn arcade-lib-watch");
    refresh_dirs();
}

/// Re-read Steam library roots after a scan (new drives from `libraryfolders.vdf`).
pub fn refresh_dirs() {
    let dirs = steamapps_watch_dirs();
    if let Ok(g) = DIRS_TX.lock()
        && let Some(tx) = g.as_ref()
    {
        let _ = tx.send(dirs);
    }
}

pub fn subscription() -> Subscription<()> {
    Subscription::run(watch_stream)
}

fn watch_stream() -> impl Stream<Item = ()> {
    let rx_opt = match FIRE_RX.lock() {
        Ok(mut g) => g.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded();
    match rx_opt {
        Some(std_rx) => {
            thread::Builder::new()
                .name("arcade-lib-watch-fwd".into())
                .spawn(move || {
                    while !iced_tx.is_closed() {
                        match std_rx.recv() {
                            Ok(()) => {
                                if iced_tx.unbounded_send(()).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
                .ok();
        }
        None => drop(iced_tx),
    }
    iced_rx
}

fn watch_loop(dirs_rx: Receiver<Vec<PathBuf>>, fire_tx: Sender<()>) {
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = event_tx.send(event);
            }
        },
        Config::default(),
    ) {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!(?err, "arcade library watcher: create failed");
            return;
        }
    };

    let mut watched: Vec<PathBuf> = Vec::new();
    loop {
        let event = match event_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ev) => Some(ev),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };

        while let Ok(dirs) = dirs_rx.try_recv() {
            apply_dirs(&mut watcher, &mut watched, dirs);
        }

        let Some(event) = event else {
            continue;
        };
        if !event_is_library_change(&event) {
            continue;
        }

        let deadline = Instant::now() + DEBOUNCE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match event_rx.recv_timeout(remaining) {
                Ok(ev) => {
                    if event_is_library_change(&ev) {
                        // extend the window by continuing until idle; keep the
                        // original deadline so a long Steam burst still fires.
                    }
                }
                Err(_) => break,
            }
        }
        while let Ok(dirs) = dirs_rx.try_recv() {
            apply_dirs(&mut watcher, &mut watched, dirs);
        }
        tracing::info!("arcade library dirtied — scheduling rescan");
        if fire_tx.send(()).is_err() {
            return;
        }
    }
}

fn apply_dirs(watcher: &mut RecommendedWatcher, watched: &mut Vec<PathBuf>, dirs: Vec<PathBuf>) {
    for old in watched.drain(..) {
        let _ = watcher.unwatch(&old);
    }
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        match watcher.watch(&dir, RecursiveMode::NonRecursive) {
            Ok(()) => {
                tracing::info!(path = %dir.display(), "watching steamapps");
                watched.push(dir);
            }
            Err(err) => {
                tracing::warn!(?err, path = %dir.display(), "arcade library watch failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acf_and_libraryfolders_count() {
        assert!(path_is_library_change(Path::new(
            "/steam/steamapps/appmanifest_400.acf"
        )));
        assert!(path_is_library_change(Path::new(
            "/steam/steamapps/libraryfolders.vdf"
        )));
        assert!(!path_is_library_change(Path::new(
            "/steam/steamapps/appmanifest_400.acf.tmp"
        )));
        assert!(!path_is_library_change(Path::new(
            "/steam/steamapps/compatdata/400/pfx"
        )));
        assert!(!path_is_library_change(Path::new(
            "/steam/appcache/librarycache/400/library_hero.jpg"
        )));
    }
}
