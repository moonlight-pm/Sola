//! Filesystem watches.
//!
//! [`subscription`] watches `design/` for the open canvas: its
//! `canvas.toml` → re-layout; any other file in it, or a shared file
//! outside every canvas (`design/tokens.css`) → reload the canvas's boards.
//! Edits inside *other* canvases are ignored. Files the boards loaded from
//! outside `design/` (a product stylesheet) are watched too. Debounced so an
//! editor's write-temp-then-rename lands as one change.
//!
//! [`newest_input`] is the same set as file times, so `wait` can tell that
//! the boards are older than the files before the watcher has acted.
//!
//! [`canvases`] rescans every project's `design/` for the rail and only
//! messages iced when the list changed (agents create canvases too).

use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

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

/// What one debounced burst touched. Both can be set (an agent rewrote
/// `canvas.toml` and the CSS together).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Change {
    pub layout: bool,
    pub content: bool,
}

impl Change {
    fn any(self) -> bool {
        self.layout || self.content
    }

    fn merge(&mut self, other: Change) {
        self.layout |= other.layout;
        self.content |= other.content;
    }
}

/// Watch `canvas`'s `design/` plus `linked` files (absolute, outside it).
pub fn subscription<M: Send + 'static>(
    canvas: PathBuf,
    linked: Vec<PathBuf>,
    on_change: fn(PathBuf, Change) -> M,
) -> Subscription<M> {
    subscription::from_recipe(WatchRecipe {
        canvas,
        linked,
        on_change,
    })
}

struct WatchRecipe<M> {
    canvas: PathBuf,
    linked: Vec<PathBuf>,
    on_change: fn(PathBuf, Change) -> M,
}

impl<M: Send + 'static> Recipe for WatchRecipe<M> {
    type Output = M;

    fn hash(&self, state: &mut subscription::Hasher) {
        "design-watch".hash(state);
        self.canvas.hash(state);
        self.linked.hash(state);
    }

    fn stream(self: Box<Self>, _input: EventStream) -> iced_futures::BoxStream<M> {
        let Self {
            canvas,
            linked,
            on_change,
        } = *self;
        stream::channel(8, async move |mut output| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Change>();
            let watched = canvas.clone();
            std::thread::Builder::new()
                .name("design-watch".into())
                .spawn(move || run(&watched, &linked, tx))
                .expect("spawn design-watch thread");
            while let Some(change) = rx.recv().await {
                if output.send(on_change(canvas.clone(), change)).await.is_err() {
                    break;
                }
            }
        })
        .boxed()
    }
}

fn run(canvas: &Path, linked: &[PathBuf], tx: tokio::sync::mpsc::UnboundedSender<Change>) {
    let Some(design) = canvas.parent() else {
        return;
    };
    let (ev_tx, ev_rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = ev_tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(dir = %design.display(), error = %e, "canvas watcher");
            return;
        }
    };
    if let Err(e) = watcher.watch(design, RecursiveMode::Recursive) {
        tracing::warn!(dir = %design.display(), error = %e, "watch design dir");
        return;
    }
    // A linked file's directory, not the file: editors replace files by
    // rename, which drops a watch on the old inode.
    let linked: BTreeSet<PathBuf> = linked.iter().cloned().collect();
    let dirs: BTreeSet<&Path> = linked.iter().filter_map(|f| f.parent()).collect();
    for dir in dirs {
        if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
            tracing::debug!(dir = %dir.display(), error = %e, "watch linked dir");
        }
    }
    tracing::debug!(dir = %design.display(), linked = linked.len(), "watching canvas");
    loop {
        let first = match ev_rx.recv_timeout(IDLE_CHECK) {
            Ok(res) => res,
            Err(RecvTimeoutError::Timeout) if tx.is_closed() => return,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut seen = Change::default();
        let mut note = |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                seen.merge(classify(&ev, canvas, design, &linked));
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
        if seen.any() && tx.send(seen).is_err() {
            return;
        }
    }
}

/// Newest modification time over everything a reload would pick up: the
/// canvas dir, shared files under `design/` (not other canvases), and the
/// linked files. Symlinks are followed (the target's time counts). The
/// canvas's own `canvas.toml` is left out: a layout edit re-lays out, it
/// does not make the pages stale (a reload then would cancel a board's
/// navigation to a new `src`).
pub fn newest_input(canvas: &Path, linked: &[PathBuf]) -> Option<SystemTime> {
    let design = canvas.parent()?;
    let mut newest = None;
    let mut bump = |path: &Path| {
        if let Ok(t) = std::fs::metadata(path).and_then(|m| m.modified()) {
            newest = newest.max(Some(t));
        }
    };
    let mut stack = vec![design.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if path == canvas || !path.join(CANVAS_FILE).is_file() {
                    stack.push(path);
                }
            } else if path != canvas.join(CANVAS_FILE) {
                bump(&path);
            }
        }
    }
    for f in linked {
        bump(f);
    }
    newest
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
/// the open canvas dir, `design` its parent, `linked` files outside it.
fn classify(ev: &notify::Event, canvas: &Path, design: &Path, linked: &BTreeSet<PathBuf>) -> Change {
    let mut out = Change::default();
    match ev.kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => {}
        EventKind::Access(_) => return out,
        _ => {}
    }
    for p in &ev.paths {
        if p.starts_with(canvas) {
            if p.parent() == Some(canvas) && p.file_name().is_some_and(|n| n == CANVAS_FILE) {
                out.layout = true;
            } else {
                out.content = true;
            }
        } else if p.starts_with(design) {
            if !in_other_canvas(p, canvas, design) {
                out.content = true;
            }
        } else if linked.contains(p) {
            out.content = true;
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

    fn ch(layout: bool, content: bool) -> Change {
        Change { layout, content }
    }

    #[test]
    fn layout_vs_content_vs_reads() {
        let (canvas, design) = (Path::new("/d/home"), Path::new("/d"));
        let none = BTreeSet::new();
        let toml = ev(EventKind::Modify(ModifyKind::Any), "/d/home/canvas.toml");
        assert_eq!(classify(&toml, canvas, design, &none), ch(true, false));
        let nested = ev(EventKind::Modify(ModifyKind::Any), "/d/home/old/canvas.toml");
        assert_eq!(classify(&nested, canvas, design, &none), ch(false, true));
        let css = ev(EventKind::Create(CreateKind::File), "/d/home/site.css");
        assert_eq!(classify(&css, canvas, design, &none), ch(false, true));
        let read = ev(EventKind::Access(AccessKind::Open(AccessMode::Read)), "/d/home/a.html");
        assert_eq!(classify(&read, canvas, design, &none), ch(false, false));
        let both = ev(EventKind::Modify(ModifyKind::Any), "/d/home/canvas.toml")
            .add_path(PathBuf::from("/d/home/site.css"));
        assert_eq!(classify(&both, canvas, design, &none), ch(true, true));
    }

    #[test]
    fn linked_files_outside_design_reload() {
        let (canvas, design) = (Path::new("/p/design/home"), Path::new("/p/design"));
        let linked: BTreeSet<PathBuf> = [PathBuf::from("/p/web/foundation.css")].into();
        let edit = ev(EventKind::Modify(ModifyKind::Any), "/p/web/foundation.css");
        assert_eq!(classify(&edit, canvas, design, &linked), ch(false, true));
        let other = ev(EventKind::Modify(ModifyKind::Any), "/p/web/other.css");
        assert_eq!(classify(&other, canvas, design, &linked), ch(false, false));
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
        let none = BTreeSet::new();
        assert_eq!(
            classify(&modify(design.join("tokens.css")), &canvas, &design, &none),
            ch(false, true)
        );
        assert_eq!(classify(&modify(other.join("a.html")), &canvas, &design, &none), ch(false, false));
        assert_eq!(
            classify(&modify(other.join(CANVAS_FILE)), &canvas, &design, &none),
            ch(false, false)
        );

        // newest_input: shared + own files count, other canvases do not.
        std::fs::write(canvas.join("a.html"), "a").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(other.join("late.html"), "b").unwrap();
        let own = std::fs::metadata(canvas.join("a.html")).unwrap().modified().unwrap();
        assert_eq!(newest_input(&canvas, &[]), Some(own));
        // A layout edit does not make the pages stale.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(canvas.join(CANVAS_FILE), "").unwrap();
        assert_eq!(newest_input(&canvas, &[]), Some(own));
        let _ = std::fs::remove_dir_all(design);
    }
}
