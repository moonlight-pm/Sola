//! Pure data for the selection marquee.

/// Minimum drag size (logical px) before a release counts as a capture.
pub const MIN_REGION: f32 = 2.0;

#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    pub active: bool,
    /// Pointer down position in overlay/window space (compositor space
    /// when the surface is framed at 0,0 full output).
    pub drag_start: Option<(f32, f32)>,
    pub drag_current: Option<(f32, f32)>,
}

impl SelectionState {
    pub fn begin(&mut self) {
        self.active = true;
        self.drag_start = None;
        self.drag_current = None;
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.drag_start = None;
        self.drag_current = None;
    }

    pub fn press(&mut self, x: f32, y: f32) {
        self.drag_start = Some((x, y));
        self.drag_current = Some((x, y));
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        if self.drag_start.is_some() {
            self.drag_current = Some((x, y));
        }
    }

    /// Normalized integer region `(x, y, w, h)` if the drag is large
    /// enough; otherwise `None` (treat as cancel).
    pub fn finish_region(&mut self) -> Option<(i32, i32, i32, i32)> {
        let region = self.current_region();
        self.cancel();
        region
    }

    /// Current axis-aligned rect from start→current, if large enough.
    pub fn current_region(&self) -> Option<(i32, i32, i32, i32)> {
        let (x0, y0) = self.drag_start?;
        let (x1, y1) = self.drag_current?;
        let left = x0.min(x1);
        let top = y0.min(y1);
        let w = (x0 - x1).abs();
        let h = (y0 - y1).abs();
        if w < MIN_REGION || h < MIN_REGION {
            return None;
        }
        Some((
            left.round() as i32,
            top.round() as i32,
            w.round().max(1.0) as i32,
            h.round().max(1.0) as i32,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_drag_is_none() {
        let mut s = SelectionState::default();
        s.begin();
        s.press(10.0, 10.0);
        s.move_to(11.0, 10.5);
        assert!(s.finish_region().is_none());
    }

    #[test]
    fn normalizes_inverted_drag() {
        let mut s = SelectionState::default();
        s.begin();
        s.press(100.0, 80.0);
        s.move_to(40.0, 20.0);
        assert_eq!(s.finish_region(), Some((40, 20, 60, 60)));
    }
}
