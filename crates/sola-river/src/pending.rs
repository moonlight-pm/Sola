//! Accumulates pending updates between bus events and the next
//! manage/render sequence pair emitted by River.
use std::collections::HashMap;

#[derive(Default)]
pub struct PendingUpdate {
    /// window_id -> (width, height) for propose_dimensions.
    pub manage: HashMap<u32, (i32, i32)>,
    /// window_id -> (x, y) for node.set_position.
    pub render_positions: HashMap<u32, (i32, i32)>,
    /// New z-order (bottom to top).
    pub composition: Option<Vec<u32>>,
    /// Pending focus change.
    pub focus: Option<FocusAction>,
    /// Latest `RegisteredChords` payload to apply in the next manage
    /// sequence. River requires `enable`/`disable` on bindings during
    /// a manage sequence.
    pub chords: Option<Vec<(u32, u32)>>,
    /// Windows to close in the next manage sequence via `river_window_v1.close`.
    pub close_windows: Vec<u32>,
    /// Windows whose `fullscreen_requested` event we received and want to
    /// honor in the next manage sequence via `river_window_v1.fullscreen`.
    /// Granting the request keeps Xwayland clients on the WM-managed
    /// surface — without it Wine falls back to spawning a separate
    /// override-redirect surface that bypasses the WM (no focus, no
    /// zoning, no input routing).
    pub fullscreen_requests: Vec<u32>,
    /// Windows whose `exit_fullscreen_requested` event we received.
    pub exit_fullscreen_requests: Vec<u32>,
    pub manage_dirty: bool,
    pub render_dirty: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum FocusAction {
    Window(u32),
    None,
}

impl PendingUpdate {
    pub fn frame(&mut self, id: u32, x: i32, y: i32, w: i32, h: i32) {
        self.manage.insert(id, (w, h));
        self.render_positions.insert(id, (x, y));
        self.manage_dirty = true;
        self.render_dirty = true;
    }

    pub fn set_composition(&mut self, order: Vec<u32>) {
        self.composition = Some(order);
        self.render_dirty = true;
    }

    /// When sola-shell re-execs it rebuilds MRU from window-id order and
    /// would restack every app. If the previous bottom surface is gone
    /// (old menubar) and a new one sits at `new[0]`, keep last's relative
    /// order for surviving ids and only splice in newcomers. A real raise
    /// keeps the same menubar id, so we honor `new` as-is.
    pub fn stabilize_composition(last: &[u32], new: &[u32]) -> Vec<u32> {
        use std::collections::HashSet;
        if last.is_empty() || new.is_empty() {
            return new.to_vec();
        }
        let last_set: HashSet<u32> = last.iter().copied().collect();
        let new_set: HashSet<u32> = new.iter().copied().collect();
        let menubar_replaced = !last_set.contains(&new[0]) && !new_set.contains(&last[0]);
        if !menubar_replaced {
            return new.to_vec();
        }
        let kept: Vec<u32> = last
            .iter()
            .copied()
            .filter(|id| new_set.contains(id))
            .collect();
        if kept.is_empty() {
            return new.to_vec();
        }
        let mut kept_i = 0usize;
        let mut out = Vec::with_capacity(new.len());
        for &id in new {
            if last_set.contains(&id) {
                if kept_i < kept.len() {
                    out.push(kept[kept_i]);
                    kept_i += 1;
                }
            } else {
                out.push(id);
            }
        }
        out.extend(kept.iter().copied().skip(kept_i));
        out
    }

    pub fn set_focus(&mut self, action: FocusAction) {
        // Focus is applied inside `handle_manage_start` because
        // `seat.focus_window` / `clear_focus` are manage-sequence requests
        // per the River protocol.
        self.focus = Some(action);
        self.manage_dirty = true;
    }

    pub fn set_chords(&mut self, chords: Vec<(u32, u32)>) {
        self.chords = Some(chords);
        self.manage_dirty = true;
    }

    pub fn queue_close(&mut self, window_ids: Vec<u32>) {
        self.close_windows.extend(window_ids);
        self.manage_dirty = true;
    }

    pub fn queue_fullscreen(&mut self, window_id: u32) {
        self.fullscreen_requests.push(window_id);
        self.manage_dirty = true;
    }

    pub fn queue_exit_fullscreen(&mut self, window_id: u32) {
        self.exit_fullscreen_requests.push(window_id);
        self.manage_dirty = true;
    }

    pub fn clear(&mut self) {
        self.manage.clear();
        self.render_positions.clear();
        self.composition = None;
        self.focus = None;
        self.chords = None;
        self.close_windows.clear();
        self.fullscreen_requests.clear();
        self.exit_fullscreen_requests.clear();
        self.manage_dirty = false;
        self.render_dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_marks_manage_and_render_dirty() {
        let mut p = PendingUpdate::default();
        p.frame(1, 100, 200, 800, 600);
        assert_eq!(p.manage.get(&1).copied(), Some((800, 600)));
        assert_eq!(p.render_positions.get(&1).copied(), Some((100, 200)));
        assert!(p.manage_dirty);
        assert!(p.render_dirty);
    }

    #[test]
    fn stabilize_preserves_app_order_across_menubar_reexec() {
        // last: menubar 1, apps A=10, B=20, C=30 (C on top)
        // new:  menubar 2, apps shuffled by window-id MRU rebuild
        let last = vec![1, 10, 20, 30];
        let new = vec![2, 10, 30, 20];
        assert_eq!(
            PendingUpdate::stabilize_composition(&last, &new),
            vec![2, 10, 20, 30]
        );
    }

    #[test]
    fn stabilize_honors_raise_when_menubar_is_unchanged() {
        let last = vec![1, 10, 20];
        let new = vec![1, 20, 10];
        assert_eq!(
            PendingUpdate::stabilize_composition(&last, &new),
            vec![1, 20, 10]
        );
    }

    #[test]
    fn stabilize_places_a_new_app_where_the_shell_asked() {
        let last = vec![1, 10, 20];
        let new = vec![2, 10, 20, 40];
        assert_eq!(
            PendingUpdate::stabilize_composition(&last, &new),
            vec![2, 10, 20, 40]
        );
    }

    #[test]
    fn composition_replaces_z_order_and_marks_render_dirty() {
        let mut p = PendingUpdate::default();
        p.set_composition(vec![3, 1, 2]);
        assert_eq!(p.composition.as_deref(), Some([3u32, 1, 2].as_slice()));
        assert!(p.render_dirty);
    }

    #[test]
    fn clear_resets_everything() {
        let mut p = PendingUpdate::default();
        p.frame(1, 0, 0, 10, 10);
        p.set_composition(vec![1]);
        p.clear();
        assert!(p.manage.is_empty());
        assert!(p.render_positions.is_empty());
        assert!(p.composition.is_none());
        assert!(!p.manage_dirty);
        assert!(!p.render_dirty);
    }
}
