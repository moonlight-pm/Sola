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

    pub fn set_focus(&mut self, action: FocusAction) {
        self.focus = Some(action);
        self.render_dirty = true;
    }

    pub fn clear(&mut self) {
        self.manage.clear();
        self.render_positions.clear();
        self.composition = None;
        self.focus = None;
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
