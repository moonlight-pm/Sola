//! Where the shell put each window (shell → sola-river and apps,
//! `Topic::WindowPlace`): tiled, floating, Cinema, the compositor's rim,
//! its workspace, its tab and whether that is on screen. One record per
//! window but the shell's own. The shell recomputes the whole set whenever
//! any of it may have changed and sends only what did.

use std::collections::HashMap;

use sola_bus::topics::WindowPlace;

/// What was last published, per window.
#[derive(Debug, Default)]
pub struct PlaceState {
    sent: HashMap<u32, WindowPlace>,
}

/// What to emit (new or changed) and what to retract (windows gone).
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub emit: Vec<WindowPlace>,
    pub retract: Vec<WindowPlace>,
}

impl PlaceState {
    /// Take the full current set; return the difference from the last one.
    pub fn update(&mut self, next: impl IntoIterator<Item = WindowPlace>) -> Changes {
        let next: HashMap<u32, WindowPlace> = next.into_iter().map(|p| (p.window_id, p)).collect();
        let mut changes = Changes::default();
        for (id, place) in &next {
            if self.sent.get(id) != Some(place) {
                changes.emit.push(place.clone());
            }
        }
        for (id, place) in &self.sent {
            if !next.contains_key(id) {
                changes.retract.push(place.clone());
            }
        }
        changes.emit.sort_by_key(|p| p.window_id);
        changes.retract.sort_by_key(|p| p.window_id);
        self.sent = next;
        changes
    }

    /// What was last published for `window_id`.
    #[cfg(test)]
    pub fn sent(&self, window_id: u32) -> Option<&WindowPlace> {
        self.sent.get(&window_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(window_id: u32, tiled: bool, tab: Option<&str>) -> WindowPlace {
        WindowPlace {
            window_id,
            tiled,
            floating: !tiled,
            cinema: false,
            rim: false,
            workspace: Some("main".into()),
            tab: tab.map(String::from),
            shown: tab.is_some(),
        }
    }

    #[test]
    fn sends_new_and_changed_windows_once() {
        let mut s = PlaceState::default();
        let first = s.update([place(1, true, None), place(2, true, Some("app/x"))]);
        assert_eq!(first.emit, vec![place(1, true, None), place(2, true, Some("app/x"))]);
        assert!(s.update([place(1, true, None), place(2, true, Some("app/x"))]) == Changes::default());
        let floated = s.update([place(1, false, None), place(2, true, Some("app/x"))]);
        assert_eq!(floated.emit, vec![place(1, false, None)]);
        assert!(floated.retract.is_empty());
    }

    #[test]
    fn retracts_windows_that_left_with_what_they_last_were() {
        let mut s = PlaceState::default();
        s.update([place(1, true, None), place(2, true, Some("app/x"))]);
        let gone = s.update([place(1, true, None)]);
        assert!(gone.emit.is_empty());
        assert_eq!(gone.retract, vec![place(2, true, Some("app/x"))]);
    }
}
