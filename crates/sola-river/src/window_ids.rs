//! Window ids that outlive a restart of sola-river. River names each window
//! with an identifier of its own (`river_window_v1.identifier`, unique and
//! never reused while the compositor runs); sola-river keeps which id it
//! gave each identifier in the runtime directory, so the windows of apps
//! that kept running through its restart (an apply that moves `pkg:sola`)
//! keep their ids. The apps name those ids when they announce their tabs
//! again, and the shell pairs each tab with its own window, not by order.
//!
//! It keeps the size each window was last given too: a window found again
//! is offered that size, not 0×0 ("pick your own"), so it does not resize
//! twice on the way back (a browser page left without a frame at its last
//! size stays blank).
//!
//! The map belongs to one compositor (its Wayland socket, by inode and
//! change time): a new rivulet starts it over.

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    compositor: String,
    ids: HashMap<String, u32>,
    /// The size each window was last given, by identifier.
    #[serde(default)]
    sizes: HashMap<String, (i32, i32)>,
}

/// Sizes are written at most this often (a drag resizes every frame).
const SIZE_FLUSH: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Default)]
pub struct WindowIds {
    path: Option<PathBuf>,
    saved: Saved,
    /// A size changed since the last write, and when that was.
    dirty: bool,
    flushed: Option<std::time::Instant>,
}

impl WindowIds {
    /// The map this compositor's last sola-river left, if any.
    pub fn load() -> Self {
        let path = sola_core::env::runtime_dir().join("sola-river-windows.json");
        let compositor = compositor().unwrap_or_default();
        let saved = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Saved>(&b).ok())
            .filter(|s| !compositor.is_empty() && s.compositor == compositor)
            .unwrap_or(Saved {
                compositor,
                ..Saved::default()
            });
        Self {
            path: Some(path),
            saved,
            ..Self::default()
        }
    }

    /// The highest id given out: new windows take ids above it.
    pub fn highest(&self) -> u32 {
        self.saved.ids.values().copied().max().unwrap_or(0)
    }

    /// The id the window with `identifier` had before.
    pub fn id_for(&self, identifier: &str) -> Option<u32> {
        self.saved.ids.get(identifier).copied()
    }

    pub fn remember(&mut self, identifier: &str, id: u32) {
        if self.saved.ids.insert(identifier.to_string(), id) != Some(id) {
            self.save();
        }
    }

    /// The size the window with `identifier` was last given.
    pub fn size_for(&self, identifier: &str) -> Option<(i32, i32)> {
        self.saved.sizes.get(identifier).copied()
    }

    /// Window `id` was given `size` (written by [`Self::flush`]).
    pub fn note_size(&mut self, id: u32, size: (i32, i32)) {
        let Some(identifier) = self.identifier_of(id) else {
            return;
        };
        if self.saved.sizes.insert(identifier, size) != Some(size) {
            self.dirty = true;
        }
    }

    /// Write the sizes noted since the last write, at most every
    /// [`SIZE_FLUSH`].
    pub fn flush(&mut self) {
        if !self.dirty || self.flushed.is_some_and(|t| t.elapsed() < SIZE_FLUSH) {
            return;
        }
        self.save();
    }

    /// The window `id` closed.
    pub fn forget(&mut self, id: u32) {
        let Some(identifier) = self.identifier_of(id) else {
            return;
        };
        self.saved.ids.remove(&identifier);
        self.saved.sizes.remove(&identifier);
        self.save();
    }

    fn identifier_of(&self, id: u32) -> Option<String> {
        self.saved
            .ids
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(k, _)| k.clone())
    }

    fn save(&mut self) {
        self.dirty = false;
        self.flushed = Some(std::time::Instant::now());
        let Some(path) = &self.path else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        let result = serde_json::to_vec(&self.saved)
            .map_err(std::io::Error::other)
            .and_then(|b| std::fs::write(&tmp, b))
            .and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = result {
            warn!(path = %path.display(), %e, "window ids: could not save");
        }
    }
}

/// The compositor sola-river runs on: its Wayland socket's inode and change
/// time (as `sola-session` tells compositors apart).
fn compositor() -> Option<String> {
    let name = sola_core::env::child_wayland_display()?;
    let path = if name.starts_with('/') {
        PathBuf::from(&name)
    } else {
        sola_core::env::runtime_dir().join(&name)
    };
    let m = std::fs::metadata(&path).ok()?;
    Some(format!("{}-{}.{:09}", m.ino(), m.ctime(), m.ctime_nsec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(pairs: &[(&str, u32)]) -> WindowIds {
        WindowIds {
            path: None,
            saved: Saved {
                compositor: "c".into(),
                ids: pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
                sizes: HashMap::new(),
            },
            ..WindowIds::default()
        }
    }

    #[test]
    fn a_window_keeps_its_id_and_new_ones_go_above() {
        let mut w = ids(&[("a", 7), ("b", 12)]);
        assert_eq!(w.id_for("a"), Some(7));
        assert_eq!(w.highest(), 12);
        w.forget(12);
        assert_eq!(w.id_for("b"), None);
        w.remember("c", 13);
        assert_eq!(w.highest(), 13);
        w.note_size(7, (1200, 2100));
        assert_eq!(w.size_for("a"), Some((1200, 2100)));
        w.forget(7);
        assert_eq!(w.size_for("a"), None);
    }
}
