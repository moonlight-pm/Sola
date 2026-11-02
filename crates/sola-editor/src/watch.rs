//! Telling the app when an open file changes on disk: an agent edited it,
//! say. Folders are what's watched, since an atomic save replaces the file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use iced::Subscription;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

struct Shared {
    watcher: RecommendedWatcher,
    folders: HashSet<PathBuf>,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);
static EVENTS: OnceLock<mpsc::UnboundedSender<PathBuf>> = OnceLock::new();

/// Report changes to the files in the folder of `file`.
pub fn add(file: &Path) {
    let Some(folder) = file.parent() else {
        return;
    };
    let mut shared = SHARED.lock().unwrap_or_else(|p| p.into_inner());
    if shared.is_none() {
        let watcher = notify::recommended_watcher(|event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                return;
            };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if let Some(events) = EVENTS.get() {
                for path in event.paths {
                    let _ = events.unbounded_send(path);
                }
            }
        });
        match watcher {
            Ok(watcher) => *shared = Some(Shared { watcher, folders: HashSet::new() }),
            Err(e) => {
                tracing::warn!(error = %e, "can't watch files; changes on disk won't show");
                return;
            }
        }
    }
    let Some(shared) = shared.as_mut() else {
        return;
    };
    if shared.folders.insert(folder.to_path_buf())
        && let Err(e) = shared.watcher.watch(folder, RecursiveMode::NonRecursive)
    {
        tracing::warn!(folder = %folder.display(), error = %e, "can't watch this folder");
    }
}

/// Paths that changed in a watched folder.
pub fn changes() -> Subscription<PathBuf> {
    Subscription::run(|| {
        iced::stream::channel(32, async |mut out: mpsc::Sender<PathBuf>| {
            let (tx, mut rx) = mpsc::unbounded();
            if EVENTS.set(tx).is_err() {
                return;
            }
            while let Some(path) = rx.next().await {
                if out.send(path).await.is_err() {
                    return;
                }
            }
        })
    })
}
