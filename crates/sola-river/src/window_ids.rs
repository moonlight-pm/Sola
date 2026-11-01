//! Window ids that outlive a restart of sola-river. River names each window
//! with an identifier of its own (`river_window_v1.identifier`, unique and
//! never reused while the compositor runs); sola-river keeps which id it
//! gave each identifier in the runtime directory, so the windows of apps
//! that kept running through its restart (an apply that moves `pkg:sola`)
//! keep their ids. The apps name those ids when they announce their tabs
//! again, and the shell pairs each tab with its own window, not by order.
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
}

#[derive(Default)]
pub struct WindowIds {
    path: Option<PathBuf>,
    saved: Saved,
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
                ids: HashMap::new(),
            });
        Self {
            path: Some(path),
            saved,
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

    /// The window `id` closed.
    pub fn forget(&mut self, id: u32) {
        let before = self.saved.ids.len();
        self.saved.ids.retain(|_, v| *v != id);
        if self.saved.ids.len() != before {
            self.save();
        }
    }

    fn save(&self) {
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
            },
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
    }
}
