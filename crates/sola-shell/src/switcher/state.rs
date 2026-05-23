//! Switcher state: the list of apps and the current selection.
//!
//! `SwitcherState` is pure data — no window logic, no iced widgets.
//! Window management (iced surface, key handling) lands in Task 6.
//!
//! `SwitcherApp` is also defined here; in the legacy shell it lived in
//! `app.rs`, but moving it here keeps the switcher module self-contained.
//! `Shell` will use `switcher::state::SwitcherApp` directly.

/// Lightweight app entry for the switcher (grouped by app_id).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitcherApp {
    pub app_id: String,
}

#[derive(Default)]
pub struct SwitcherState {
    pub active: bool,
    pub apps: Vec<SwitcherApp>,
    pub selected: usize,
}

impl SwitcherState {
    pub fn selected_app_id(&self) -> Option<&str> {
        self.apps.get(self.selected).map(|a| a.app_id.as_str())
    }

    pub fn select_next(&mut self) {
        if !self.apps.is_empty() {
            self.selected = (self.selected + 1) % self.apps.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.apps.is_empty() {
            self.selected = (self.selected + self.apps.len() - 1) % self.apps.len();
        }
    }
}

/// Derive the ordered app list for the switcher.
///
/// Contract:
/// - `mru` is the shell's `mru_apps` (most-recently-used app_ids, front = most recent).
/// - `known` is `shell.known_windows` (all currently open windows from sola-river).
/// - `applications` is the shell's app catalog (for label/icon look-up, not needed here).
///
/// Result: MRU-ordered `SwitcherApp` entries for apps that have ≥1 open window,
/// followed by any open apps not yet in the MRU list.
/// The shell itself (`app_id == "sola-shell"`) is excluded.
pub fn rebuild_apps(
    state: &mut SwitcherState,
    mru: &[String],
    known: &[sola_bus::topics::Window],
) {
    use std::collections::HashSet;

    // Unique app_ids that have at least one open window, excluding shell itself.
    let open_ids: HashSet<&str> = known
        .iter()
        .filter(|w| w.app_id != "sola-shell")
        .map(|w| w.app_id.as_str())
        .collect();

    // MRU-ordered first.
    let mut apps: Vec<SwitcherApp> = mru
        .iter()
        .filter(|id| open_ids.contains(id.as_str()))
        .map(|id| SwitcherApp { app_id: id.clone() })
        .collect();

    // Append any open apps not yet in MRU.
    let in_result: HashSet<String> = apps.iter().map(|a| a.app_id.clone()).collect();
    let to_add: Vec<SwitcherApp> = open_ids
        .iter()
        .filter(|id| !in_result.contains(**id))
        .map(|id| SwitcherApp { app_id: id.to_string() })
        .collect();
    apps.extend(to_add);

    state.apps = apps;
    // Clamp selection to valid range.
    if state.apps.is_empty() {
        state.selected = 0;
    } else {
        state.selected = state.selected.min(state.apps.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_apps(ids: &[&str]) -> Vec<SwitcherApp> {
        ids.iter()
            .map(|id| SwitcherApp { app_id: id.to_string() })
            .collect()
    }

    #[test]
    fn select_next_wraps_around() {
        let mut s = SwitcherState {
            active: true,
            apps: make_apps(&["a", "b", "c"]),
            selected: 2,
        };
        s.select_next();
        assert_eq!(s.selected, 0, "should wrap from last to first");
    }

    #[test]
    fn select_prev_wraps_around() {
        let mut s = SwitcherState {
            active: true,
            apps: make_apps(&["a", "b", "c"]),
            selected: 0,
        };
        s.select_prev();
        assert_eq!(s.selected, 2, "should wrap from first to last");
    }

    #[test]
    fn select_next_is_noop_on_empty() {
        let mut s = SwitcherState::default();
        s.select_next(); // must not panic
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn select_prev_is_noop_on_empty() {
        let mut s = SwitcherState::default();
        s.select_prev(); // must not panic
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn selected_app_id_returns_correct_entry() {
        let mut s = SwitcherState {
            active: true,
            apps: make_apps(&["firefox", "sola-terminal", "zed"]),
            selected: 1,
        };
        assert_eq!(s.selected_app_id(), Some("sola-terminal"));
        s.select_next();
        assert_eq!(s.selected_app_id(), Some("zed"));
    }

    #[test]
    fn selected_app_id_none_on_empty() {
        let s = SwitcherState::default();
        assert!(s.selected_app_id().is_none());
    }

    #[test]
    fn sequential_next_cycles_all_entries() {
        let mut s = SwitcherState {
            active: true,
            apps: make_apps(&["a", "b", "c"]),
            selected: 0,
        };
        s.select_next();
        assert_eq!(s.selected, 1);
        s.select_next();
        assert_eq!(s.selected, 2);
        s.select_next();
        assert_eq!(s.selected, 0);
    }
}
