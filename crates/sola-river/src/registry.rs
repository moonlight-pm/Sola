//! Per-window metadata keyed by our minted u32 ID.
//!
//! The Wayland proxies (RiverWindowV1, RiverNodeV1) are stored separately
//! on `AppData` to keep this module free of Wayland dependencies, so it
//! can be unit-tested without a compositor.
use std::collections::HashMap;

use sola_bus::topics::{Window, WindowGeometry};
use wayland_client::backend::ObjectId;

use crate::protocol::river_xkb_bindings_v1::river_xkb_binding_v1::RiverXkbBindingV1;

/// Map our `(keysym, modifiers)` chords to their live
/// `river_xkb_binding_v1` proxies.
#[derive(Default)]
pub struct ChordRegistry {
    pub by_chord: HashMap<(u32, u32), RiverXkbBindingV1>,
    /// Reverse lookup: binding object id → the pair we registered it for.
    /// Used when a `pressed` event carries only the binding object.
    pub by_object: HashMap<ObjectId, (u32, u32)>,
}

#[derive(Default)]
pub struct WindowRegistry {
    next_id: u32,
    by_id: HashMap<u32, Entry>,
}

pub struct Entry {
    pub app_id: Option<String>,
    pub title: Option<String>,
    /// Max-size dimensions hint from `river_window_v1.dimensions_hint`.
    /// Used to center unzoned windows; `0` means "no preference".
    pub max_size: (i32, i32),
    /// PID from `river_window_v1.unreliable_pid`. Best-effort only — the
    /// protocol explicitly marks this unreliable (PID reuse, late
    /// arrival). Forwarded to the bus so shell/settings can look up the
    /// binary via `/proc/<pid>/…`.
    pub pid: Option<u32>,
    /// Last frame received from the shell as `(x, y, width, height)`.
    /// Used by solactl's per-window screenshot to pass a region to
    /// `grim`. May be stale immediately after a resize but converges.
    pub frame: Option<(i32, i32, i32, i32)>,
    /// Actual content size from `river_window_v1.dimensions`. `None` until the
    /// first dimensions event. Distinct from `frame` (the shell's requested
    /// rect) — this is what the window actually reports.
    pub size: Option<(i32, i32)>,
    /// Actual on-screen position from the last `node.set_position`. `None`
    /// until the window is first placed.
    pub position: Option<(i32, i32)>,
}

impl WindowRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mint(&mut self) -> u32 {
        self.next_id += 1;
        self.by_id.insert(
            self.next_id,
            Entry {
                app_id: None,
                title: None,
                max_size: (0, 0),
                pid: None,
                frame: None,
                size: None,
                position: None,
            },
        );
        self.next_id
    }

    pub fn set_max_size(&mut self, id: u32, width: i32, height: i32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.max_size = (width, height);
        }
    }

    pub fn get(&self, id: u32) -> Option<&Entry> {
        self.by_id.get(&id)
    }

    /// Look up the app_id for a window, if known.
    pub fn app_id_for(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id).and_then(|e| e.app_id.as_deref())
    }

    pub fn set_app_id(&mut self, id: u32, value: String) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.app_id = Some(value);
        }
    }

    pub fn set_title(&mut self, id: u32, value: String) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.title = Some(value);
        }
    }

    pub fn set_pid(&mut self, id: u32, pid: u32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.pid = Some(pid);
        }
    }

    pub fn pid_for(&self, id: u32) -> Option<u32> {
        self.by_id.get(&id).and_then(|e| e.pid)
    }

    /// If `app_id` is empty/missing and the window's pid looks like gamescope,
    /// force `app_id = "gamescope"` (and a floor title when still unset).
    ///
    /// gamescope's wayland host often reports `app_id=""` under River even
    /// though libdecor calls `set_app_id("gamescope")` — without this the
    /// shell menubar/switcher show a blank name and host sizing never takes
    /// the gamescope pin path.
    ///
    /// Returns true when identity was rewritten (caller should re-publish
    /// Windows).
    pub fn maybe_infer_gamescope_identity(&mut self, id: u32) -> bool {
        use crate::proc_identity::{
            GAMESCOPE_APP_ID, GAMESCOPE_DEFAULT_TITLE, process_is_gamescope,
        };
        let Some(e) = self.by_id.get_mut(&id) else {
            return false;
        };
        let empty_app = e
            .app_id
            .as_deref()
            .map(|s| s.is_empty())
            .unwrap_or(true);
        if !empty_app {
            return false;
        }
        let Some(pid) = e.pid else {
            return false;
        };
        if !process_is_gamescope(pid) {
            return false;
        }
        e.app_id = Some(GAMESCOPE_APP_ID.into());
        let title_empty = e
            .title
            .as_deref()
            .map(|s| s.is_empty())
            .unwrap_or(true);
        if title_empty {
            e.title = Some(GAMESCOPE_DEFAULT_TITLE.into());
        }
        true
    }

    pub fn set_frame(&mut self, id: u32, x: i32, y: i32, w: i32, h: i32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.frame = Some((x, y, w, h));
        }
    }

    /// Record the window's actual content size (from the `dimensions` event).
    /// Returns true if it changed, so the caller emits geometry only on change.
    pub fn set_size(&mut self, id: u32, width: i32, height: i32) -> bool {
        let Some(e) = self.by_id.get_mut(&id) else {
            return false;
        };
        if e.size == Some((width, height)) {
            return false;
        }
        e.size = Some((width, height));
        true
    }

    /// Record the window's actual on-screen position (from `set_position`).
    /// Returns true if it changed.
    pub fn set_position(&mut self, id: u32, x: i32, y: i32) -> bool {
        let Some(e) = self.by_id.get_mut(&id) else {
            return false;
        };
        if e.position == Some((x, y)) {
            return false;
        }
        e.position = Some((x, y));
        true
    }

    /// The window's full rectangle, available once both size and position are
    /// known. `None` while either is still unset.
    pub fn geometry(&self, id: u32) -> Option<WindowGeometry> {
        let e = self.by_id.get(&id)?;
        let (width, height) = e.size?;
        let (x, y) = e.position?;
        Some(WindowGeometry {
            window_id: id,
            x,
            y,
            width,
            height,
        })
    }

    /// Find a window matching `app_id` and (optionally) `title`. If
    /// `title` is `None`, returns the first window for that app. Used by
    /// `solactl screenshot --app/--window`.
    pub fn find_by_app_title(&self, app_id: &str, title: Option<&str>) -> Option<&Entry> {
        self.by_id
            .values()
            .filter(|e| e.app_id.as_deref() == Some(app_id))
            .find(|e| match title {
                Some(t) => e.title.as_deref() == Some(t),
                None => true,
            })
    }

    pub fn remove(&mut self, id: u32) {
        self.by_id.remove(&id);
    }

    /// Iterate all known windows (including partial app_id/title).
    pub fn entries(&self) -> impl Iterator<Item = (&u32, &Entry)> {
        self.by_id.iter()
    }

    /// Snapshot as the bus `Window` list, skipping entries that haven't yet
    /// received both their `app_id` and `title` events — those are still
    /// in flight and would produce spurious sticky transitions.
    pub fn as_windows(&self) -> Vec<Window> {
        let mut v: Vec<Window> = self
            .by_id
            .iter()
            .filter_map(|(id, e)| {
                let (Some(app_id), Some(title)) = (e.app_id.clone(), e.title.clone()) else {
                    return None;
                };
                Some(Window {
                    window_id: *id,
                    app_id,
                    title,
                    pid: e.pid,
                })
            })
            .collect();
        v.sort_by_key(|a| a.window_id);
        v
    }
}

/// Diff two `(keysym, modifiers)` sets. Returns `(added, removed)`, both
/// sorted for determinism.
pub fn chord_diff(old: &[(u32, u32)], new: &[(u32, u32)]) -> (Vec<(u32, u32)>, Vec<(u32, u32)>) {
    use std::collections::HashSet;
    let old_set: HashSet<(u32, u32)> = old.iter().copied().collect();
    let new_set: HashSet<(u32, u32)> = new.iter().copied().collect();
    let mut added: Vec<_> = new_set.difference(&old_set).copied().collect();
    let mut removed: Vec<_> = old_set.difference(&new_set).copied().collect();
    added.sort();
    removed.sort();
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_assigns_monotonic_ids() {
        let mut r = WindowRegistry::new();
        assert_eq!(r.mint(), 1);
        assert_eq!(r.mint(), 2);
    }

    #[test]
    fn geometry_is_some_only_when_size_and_position_known() {
        let mut r = WindowRegistry::new();
        let id = r.mint();
        assert!(r.geometry(id).is_none());
        assert!(r.set_size(id, 800, 600)); // changed
        assert!(r.geometry(id).is_none()); // position still unknown
        assert!(r.set_position(id, 10, 20)); // changed
        let g = r.geometry(id).expect("both known now");
        assert_eq!(
            (g.window_id, g.x, g.y, g.width, g.height),
            (id, 10, 20, 800, 600)
        );
        assert!(!r.set_size(id, 800, 600)); // unchanged → false
        assert!(!r.set_position(id, 10, 20)); // unchanged → false
    }

    #[test]
    fn set_and_get_app_id() {
        let mut r = WindowRegistry::new();
        let id = r.mint();
        r.set_app_id(id, "zen".into());
        assert_eq!(r.get(id).unwrap().app_id.as_deref(), Some("zen"));
    }

    #[test]
    fn remove_drops_entry() {
        let mut r = WindowRegistry::new();
        let id = r.mint();
        r.remove(id);
        assert!(r.get(id).is_none());
    }

    #[test]
    fn as_windows_returns_only_fully_populated() {
        let mut r = WindowRegistry::new();
        let a = r.mint();
        r.set_app_id(a, "zen".into());
        r.set_title(a, "Browser".into());
        let _b = r.mint();
        let apps = r.as_windows();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].window_id, a);
        assert_eq!(apps[0].app_id, "zen");
    }

    #[test]
    fn as_windows_sorted_by_id() {
        let mut r = WindowRegistry::new();
        let a = r.mint();
        let b = r.mint();
        r.set_app_id(b, "b".into());
        r.set_title(b, "B".into());
        r.set_app_id(a, "a".into());
        r.set_title(a, "A".into());
        let apps = r.as_windows();
        assert_eq!(apps[0].window_id, a);
        assert_eq!(apps[1].window_id, b);
    }

    #[test]
    fn chord_diff_added_and_removed() {
        let old: Vec<(u32, u32)> = vec![(0x61, 64), (0x62, 64)];
        let new: Vec<(u32, u32)> = vec![(0x62, 64), (0x63, 64)];
        let (added, removed) = chord_diff(&old, &new);
        assert_eq!(added, vec![(0x63, 64)]);
        assert_eq!(removed, vec![(0x61, 64)]);
    }

    #[test]
    fn chord_diff_no_change() {
        let same: Vec<(u32, u32)> = vec![(0x61, 64)];
        let (added, removed) = chord_diff(&same, &same);
        assert!(added.is_empty());
        assert!(removed.is_empty());
    }
}
