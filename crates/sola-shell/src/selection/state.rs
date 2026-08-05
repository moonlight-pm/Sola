//! Pure data for the selection marquee.

/// Minimum drag size (logical px) before a release counts as a capture.
pub const MIN_REGION: f32 = 2.0;

#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    pub active: bool,
    /// Window that held keyboard focus when the marquee opened — restored
    /// on cancel / finish so we don't leave focus stuck on the hidden
    /// selection surface (which drops keyboard routing for the desktop).
    pub prior_focus: Option<u32>,
    /// Pointer down position in overlay/window space (compositor space
    /// when the surface is framed at 0,0 full output).
    pub drag_start: Option<(f32, f32)>,
    pub drag_current: Option<(f32, f32)>,
}

impl SelectionState {
    pub fn begin(&mut self, prior_focus: Option<u32>) {
        self.active = true;
        self.prior_focus = prior_focus;
        self.drag_start = None;
        self.drag_current = None;
    }

    /// End selection and return the focus window to restore (if any).
    pub fn cancel(&mut self) -> Option<u32> {
        self.active = false;
        self.drag_start = None;
        self.drag_current = None;
        self.prior_focus.take()
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

    /// End the drag: returns `(region?, prior_focus_to_restore)`.
    /// Region is `None` when the drag is too small (cancel without capture).
    pub fn finish_region(&mut self) -> (Option<(i32, i32, i32, i32)>, Option<u32>) {
        let region = self.current_region();
        let prior = self.cancel();
        (region, prior)
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
        s.begin(Some(7));
        s.press(10.0, 10.0);
        s.move_to(11.0, 10.5);
        let (region, prior) = s.finish_region();
        assert!(region.is_none());
        assert_eq!(prior, Some(7));
    }

    #[test]
    fn normalizes_inverted_drag() {
        let mut s = SelectionState::default();
        s.begin(None);
        s.press(100.0, 80.0);
        s.move_to(40.0, 20.0);
        let (region, prior) = s.finish_region();
        assert_eq!(region, Some((40, 20, 60, 60)));
        assert_eq!(prior, None);
    }

    #[test]
    fn cancel_returns_prior_focus() {
        let mut s = SelectionState::default();
        s.begin(Some(42));
        assert_eq!(s.cancel(), Some(42));
        assert!(!s.active);
        assert_eq!(s.prior_focus, None);
    }
}
