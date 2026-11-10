//! Re-reading when packages move: Oath writes a line to its apply log
//! after every apply and update. The line itself is not read; any new one
//! means read the catalog again. (`oath store` and `oath build` write
//! none, and change no package.) An update that moves nothing may still
//! write the heads it took (`/oath/heads.json`), which changes what is an
//! overlay, so that file counts too.

use iced::Subscription;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use notify::{EventKind, RecursiveMode, Watcher};

use crate::oath;

pub fn applies() -> Subscription<()> {
    Subscription::run(|| {
        iced::stream::channel(8, async |mut out: mpsc::Sender<()>| {
            let files = [oath::apply_log(), oath::heads_file()];
            let folders: Vec<std::path::PathBuf> = files.iter().filter_map(|f| f.parent().map(std::path::Path::to_path_buf)).collect();
            let (tx, mut rx) = mpsc::unbounded();
            let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event
                    && !matches!(event.kind, EventKind::Access(_))
                    && event.paths.iter().any(|p| files.iter().any(|f| p.file_name() == f.file_name()))
                {
                    let _ = tx.unbounded_send(());
                }
            });
            let mut watcher = match watcher {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!(error = %e, "can't watch Oath's apply log; reopen the app after an apply");
                    return;
                }
            };
            for folder in &folders {
                if let Err(e) = watcher.watch(folder, RecursiveMode::NonRecursive) {
                    tracing::warn!(folder = %folder.display(), error = %e, "can't watch Oath's files");
                    return;
                }
            }
            while rx.next().await.is_some() {
                if out.send(()).await.is_err() {
                    return;
                }
            }
        })
    })
}
