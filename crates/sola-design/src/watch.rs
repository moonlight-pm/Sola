//! Filesystem watches.
//!
//! [`subscription`] watches `design/` for the open canvas: its
//! `canvas.toml` → re-layout; any other file in it, or a shared file
//! outside every canvas (`design/tokens.css`) → reload the canvas's boards.
//! Edits inside *other* canvases are ignored. Debounced so an editor's
//! write-temp-then-rename lands as one change.
//!
//! [`canvases`] rescans every project's `design/` for the rail and only
//! messages iced when the list changed (agents create canvases too).

use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use iced::futures::{SinkExt, StreamExt as _};
use iced::{Subscription, stream};
use iced_futures::subscription::{self, EventStream, Recipe};
use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecursiveMode, Watcher};

use crate::canvas::{self, CANVAS_FILE};

const DEBOUNCE: Duration = Duration::from_millis(120);
/// How often an idle watcher checks that iced still wants it.
const IDLE_CHECK: Duration = Duration::from_secs(1);
/// Rail rescan period. A read_dir per project; no message unless changed.
const SCAN_EVERY: Duration = Duration::from_secs(2);

pub type Canvases = HashMap<String, Vec<canvas::Entry>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Layout,
    Content,
}

pub fn subscription<M: Send + 'static>(
    dir: PathBuf,
    on_change: fn(PathBuf, Change) -> M,
) -> Subscription<M> {
    subscription::from_recipe(WatchRecipe { dir, on_change })
}

struct WatchRecipe<M> {
    dir: PathBuf,
    on_change: fn(PathBuf, Change) -> M,
}

impl<M: Send + 'static> Recipe for WatchRecipe<M> {
    type Output = M;

    fn hash(&self, state: &mut subscription::Hasher) {
        "design-watch".hash(state);
        self.dir.hash(state);
    }

    fn stream(self: Box<Self>, _input: EventStream) -> iced_futures::BoxStream<M> {
        let Self { dir, on_change } = *self;
        stream::channel(8, async move |mut output| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Change>();
            let canvas = dir.clone();
            std::thread::Builder::new()
                .name("design-watch".into())
                .spawn(move || run(&canvas, tx))
                .expect("spawn design-watch thread");
            while let Some(change) = rx.recv().await {
                if output.send(on_change(dir.clone(), change)).await.is_err() {
                    break;
                }
            }
        })
        .boxed()
    }
}

fn run(canvas: &Path, tx: tokio::sync::mpsc::UnboundedSender<Change>) {
    let Some(dir) = canvas.parent() else {
        return;
    };
    let (ev_tx, ev_rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = ev_tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(dir = %dir.display(), error = %e, "canvas watcher");
            return;
        }
    };
    if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
        tracing::warn!(dir = %dir.display(), error = %e, "watch canvas dir");
        return;
    }
    tracing::debug!(dir = %dir.display(), "watching canvas");
    loop {
        let first = match ev_rx.recv_timeout(IDLE_CHECK) {
            Ok(res) => res,
            Err(RecvTimeoutError::Timeout) if tx.is_closed() => return,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut seen = None;
        let mut note = |res: notify::Result<notify::Event>| {
            if let Some(c) = res.ok().and_then(|ev| classify(&ev, canvas, dir)) {
                seen = Some(if c == Change::Layout || seen == Some(Change::Layout) {
                    Change::Layout
                } else {
                    Change::Content
                });
            }
        };
        note(first);
        let deadline = Instant::now() + DEBOUNCE;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match ev_rx.recv_timeout(left) {
                Ok(res) => note(res),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        if let Some(change) = seen {
            if tx.send(change).is_err() {
                return;
            }
        }
    }
}

/// Canvas lists for `(project id, root)` pairs; first scan always reports.
pub fn canvases<M: Send + 'static>(
    projects: Vec<(String, PathBuf)>,
    on_change: fn(Canvases) -> M,
) -> Subscription<M> {
    subscription::from_recipe(ScanRecipe {
        projects,
        on_change,
    })
}

struct ScanRecipe<M> {
    projects: Vec<(String, PathBuf)>,
    on_change: fn(Canvases) -> M,
}

impl<M: Send + 'static> Recipe for ScanRecipe<M> {
    type Output = M;

    fn hash(&self, state: &mut subscription::Hasher) {
        "design-scan".hash(state);
        self.projects.hash(state);
    }

    fn stream(self: Box<Self>, _input: EventStream) -> iced_futures::BoxStream<M> {
        let Self {
            projects,
            on_change,
        } = *self;
        stream::channel(4, async move |mut output| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Canvases>();
            std::thread::Builder::new()
                .name("design-scan".into())
                .spawn(move || {
                    let mut last: Option<Canvases> = None;
                    while !tx.is_closed() {
                        let now: Canvases = projects
                            .iter()
                            .map(|(id, root)| (id.clone(), canvas::scan(root)))
                            .collect();
                        if last.as_ref() != Some(&now) {
                            if tx.send(now.clone()).is_err() {
                                return;
                            }
                            last = Some(now);
                        }
                        std::thread::sleep(SCAN_EVERY);
                    }
                })
                .expect("spawn design-scan thread");
            while let Some(found) = rx.recv().await {
                if output.send(on_change(found)).await.is_err() {
                    break;
                }
            }
        })
        .boxed()
    }
}

/// Writes, creates, removes, and renames count; reads do not. `canvas` is
/// the open canvas dir, `design` its parent.
fn classify(ev: &notify::Event, canvas: &Path, design: &Path) -> Option<Change> {
    match ev.kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => {}
        EventKind::Access(_) => return None,
        _ => {}
    }
    let mut out = None;
    for p in &ev.paths {
        let change = if p.starts_with(canvas) {
            if p.parent() == Some(canvas) && p.file_name().is_some_and(|n| n == CANVAS_FILE) {
                Change::Layout
            } else {
                Change::Content
            }
        } else if in_other_canvas(p, canvas, design) {
            continue;
        } else {
            Change::Content
        };
        if out != Some(Change::Layout) {
            out = Some(change);
        }
    }
    out
}

/// `p` sits inside a sibling canvas (a `design/<x>/` with its own toml).
fn in_other_canvas(p: &Path, canvas: &Path, design: &Path) -> bool {
    let Some(first) = p.strip_prefix(design).ok().and_then(|r| r.components().next()) else {
        return false;
    };
    let top = design.join(first);
    top != canvas && top != p && top.join(CANVAS_FILE).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind};

    fn ev(kind: EventKind, path: &str) -> notify::Event {
        notify::Event::new(kind).add_path(PathBuf::from(path))
    }

    #[test]
    fn layout_vs_content_vs_reads() {
        let (canvas, design) = (Path::new("/d/home"), Path::new("/d"));
        let toml = ev(EventKind::Modify(ModifyKind::Any), "/d/home/canvas.toml");
        assert_eq!(classify(&toml, canvas, design), Some(Change::Layout));
        let nested = ev(EventKind::Modify(ModifyKind::Any), "/d/home/old/canvas.toml");
        assert_eq!(classify(&nested, canvas, design), Some(Change::Content));
        let css = ev(EventKind::Create(CreateKind::File), "/d/home/site.css");
        assert_eq!(classify(&css, canvas, design), Some(Change::Content));
        let read = ev(EventKind::Access(AccessKind::Open(AccessMode::Read)), "/d/home/a.html");
        assert_eq!(classify(&read, canvas, design), None);
        let wrote = ev(
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            "/d/home/a.html",
        );
        assert_eq!(classify(&wrote, canvas, design), Some(Change::Content));
    }

    #[test]
    fn shared_files_reload_and_sibling_canvases_do_not() {
        let design = std::env::temp_dir().join(format!(
            "sola-design-watch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let canvas = design.join("home");
        let other = design.join("settings");
        std::fs::create_dir_all(&canvas).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join(CANVAS_FILE), "").unwrap();
        let modify = |p: PathBuf| ev(EventKind::Modify(ModifyKind::Any), p.to_str().unwrap());
        assert_eq!(
            classify(&modify(design.join("tokens.css")), &canvas, &design),
            Some(Change::Content)
        );
        assert_eq!(classify(&modify(other.join("a.html")), &canvas, &design), None);
        assert_eq!(classify(&modify(other.join(CANVAS_FILE)), &canvas, &design), None);
        let _ = std::fs::remove_dir_all(design);
    }
}
