//! Per-window metadata keyed by our minted u32 ID.
//!
//! The Wayland proxies (RiverWindowV1, RiverNodeV1) are stored separately
//! on `AppData` to keep this module free of Wayland dependencies, so it
//! can be unit-tested without a compositor.
use std::collections::HashMap;

use sola_bus::topics::App;
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
            },
        );
        self.next_id
    }

    pub fn get(&self, id: u32) -> Option<&Entry> {
        self.by_id.get(&id)
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

    pub fn remove(&mut self, id: u32) {
        self.by_id.remove(&id);
    }

    /// Snapshot as the bus `App` list, skipping entries that haven't yet
    /// received both their `app_id` and `title` events — those are still
    /// in flight and would produce spurious sticky transitions.
    pub fn as_apps(&self) -> Vec<App> {
        let mut v: Vec<App> = self
            .by_id
            .iter()
            .filter_map(|(id, e)| {
                let (Some(app_id), Some(title)) = (e.app_id.clone(), e.title.clone())
                else {
                    return None;
                };
                Some(App {
                    window_id: *id,
                    app_id,
                    title,
                })
            })
            .collect();
        v.sort_by_key(|a| a.window_id);
        v
    }
}

/// Diff two `(keysym, modifiers)` sets. Returns `(added, removed)`, both
/// sorted for determinism.
pub fn chord_diff(
    old: &[(u32, u32)],
    new: &[(u32, u32)],
) -> (Vec<(u32, u32)>, Vec<(u32, u32)>) {
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
    fn as_apps_returns_only_fully_populated() {
        let mut r = WindowRegistry::new();
        let a = r.mint();
        r.set_app_id(a, "zen".into());
        r.set_title(a, "Browser".into());
        let _b = r.mint();
        let apps = r.as_apps();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].window_id, a);
        assert_eq!(apps[0].app_id, "zen");
    }

    #[test]
    fn as_apps_sorted_by_id() {
        let mut r = WindowRegistry::new();
        let a = r.mint();
        let b = r.mint();
        r.set_app_id(b, "b".into());
        r.set_title(b, "B".into());
        r.set_app_id(a, "a".into());
        r.set_title(a, "A".into());
        let apps = r.as_apps();
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
