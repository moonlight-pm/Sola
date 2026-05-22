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
