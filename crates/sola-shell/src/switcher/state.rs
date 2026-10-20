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

/// Pointer travel (overlay-local px) before hover may change the
/// selection. Wayland `enter` synthesizes a move at the cursor's
/// current point, and iced fires `on_enter` when the surface appears
/// under that still pointer — neither is the user choosing an icon.
const HOVER_ARM_DISTANCE_PX: f32 = 4.0;

#[derive(Default)]
pub struct SwitcherState {
    pub active: bool,
    pub apps: Vec<SwitcherApp>,
    pub selected: usize,
    /// Overlay-local cursor at the first sample after open.
    /// `None` until that sample arrives.
    hover_origin: Option<(f32, f32)>,
    /// Set once the pointer has moved away from [`Self::hover_origin`].
    hover_armed: bool,
    /// Icon under the pointer. Applied when hover arms, and on later
    /// enters while armed.
    pending_hover: Option<usize>,
}

impl SwitcherState {
    /// Drop hover tracking for a fresh open. The preselected index is
    /// left alone — callers set that to the previous app.
    pub fn prepare_open(&mut self) {
        self.hover_origin = None;
        self.hover_armed = false;
        self.pending_hover = None;
    }

    pub fn selected_app_id(&self) -> Option<&str> {
        self.apps.get(self.selected).map(|a| a.app_id.as_str())
    }

    /// Icon under the pointer. Ignored until [`Self::note_pointer`]
    /// sees real travel; a surface appearing under a still cursor
    /// must not replace the preselected app.
    pub fn note_hover(&mut self, index: usize) {
        if !self.active || self.apps.is_empty() {
            return;
        }
        let index = index.min(self.apps.len() - 1);
        self.pending_hover = Some(index);
        if self.hover_armed {
            self.selected = index;
        }
    }

    /// Overlay-local pointer sample. The first sample is wherever the
    /// cursor already was; selection changes only after a later sample
    /// moves at least [`HOVER_ARM_DISTANCE_PX`].
    pub fn note_pointer(&mut self, x: f32, y: f32) {
        if !self.active || self.hover_armed {
            return;
        }
        let Some((origin_x, origin_y)) = self.hover_origin else {
            self.hover_origin = Some((x, y));
            return;
        };
        let delta_x = x - origin_x;
        let delta_y = y - origin_y;
        if delta_x * delta_x + delta_y * delta_y < HOVER_ARM_DISTANCE_PX * HOVER_ARM_DISTANCE_PX {
            return;
        }
        self.hover_armed = true;
        if let Some(index) = self.pending_hover {
            self.selected = index;
        }
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
pub fn rebuild_apps(state: &mut SwitcherState, mru: &[String], known: &[sola_bus::topics::Window]) {
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
        .map(|id| SwitcherApp {
            app_id: id.to_string(),
        })
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
            .map(|id| SwitcherApp {
                app_id: id.to_string(),
            })
            .collect()
    }

    #[test]
    fn select_next_wraps_around() {
        let mut s = SwitcherState {
            active: true,
            apps: make_apps(&["a", "b", "c"]),
            selected: 2,
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        };
        s.select_next();
        assert_eq!(s.selected, 1);
        s.select_next();
        assert_eq!(s.selected, 2);
        s.select_next();
        assert_eq!(s.selected, 0);
    }

    fn open_on_previous() -> SwitcherState {
        SwitcherState {
            active: true,
            apps: make_apps(&["current", "previous", "other"]),
            selected: 1,
            ..Default::default()
        }
    }

    #[test]
    fn still_pointer_keeps_preselected_app() {
        let mut switcher = open_on_previous();
        // Surface appeared under the cursor: enter + synthesized move.
        switcher.note_hover(2);
        switcher.note_pointer(400.0, 300.0);
        assert_eq!(switcher.selected, 1);
        // Same point again (layout refresh, or a second enter sample).
        switcher.note_hover(2);
        switcher.note_pointer(400.0, 300.0);
        assert_eq!(switcher.selected, 1);
    }

    #[test]
    fn pointer_jitter_below_arm_distance_does_not_select() {
        let mut switcher = open_on_previous();
        switcher.note_hover(2);
        switcher.note_pointer(400.0, 300.0);
        switcher.note_pointer(402.0, 301.0);
        assert_eq!(switcher.selected, 1);
    }

    #[test]
    fn pointer_move_selects_icon_already_under_cursor() {
        let mut switcher = open_on_previous();
        // Child hover is published before the parent pointer sample.
        switcher.note_hover(2);
        switcher.note_pointer(400.0, 300.0);
        switcher.note_pointer(410.0, 300.0);
        assert_eq!(switcher.selected, 2);
    }

    #[test]
    fn pointer_move_selects_icon_entered_on_that_move() {
        let mut switcher = open_on_previous();
        switcher.note_hover(2);
        switcher.note_pointer(400.0, 300.0);
        // Moved onto a different icon before the arming sample.
        switcher.note_hover(0);
        switcher.note_pointer(420.0, 300.0);
        assert_eq!(switcher.selected, 0);
    }

    #[test]
    fn hover_after_arm_follows_the_pointer() {
        let mut switcher = open_on_previous();
        switcher.note_hover(2);
        switcher.note_pointer(0.0, 0.0);
        switcher.note_pointer(20.0, 0.0);
        assert_eq!(switcher.selected, 2);
        switcher.note_hover(0);
        assert_eq!(switcher.selected, 0);
    }

    #[test]
    fn prepare_open_requires_a_fresh_move() {
        let mut switcher = open_on_previous();
        switcher.note_hover(2);
        switcher.note_pointer(0.0, 0.0);
        switcher.note_pointer(20.0, 0.0);
        assert_eq!(switcher.selected, 2);

        switcher.selected = 1;
        switcher.prepare_open();
        switcher.note_hover(2);
        switcher.note_pointer(80.0, 80.0);
        assert_eq!(switcher.selected, 1);
    }

    #[test]
    fn hover_while_inactive_does_not_change_selection() {
        let mut switcher = open_on_previous();
        switcher.active = false;
        switcher.note_hover(2);
        switcher.note_pointer(0.0, 0.0);
        switcher.note_pointer(40.0, 0.0);
        assert_eq!(switcher.selected, 1);
    }
}
