//! Pure data for the selection marquee.

/// Minimum drag size (logical px) before a release counts as a capture.
pub const MIN_REGION: f32 = 2.0;

#[derive(Clone, Default)]
pub struct SelectionState {
    pub active: bool,
    /// Super+Shift+4 has started a freeze capture; overlay is not shown yet.
    pub pending: bool,
    /// Drop freeze replies whose generation does not match (Escape mid-capture).
    pub freeze_generation: u64,
    /// Frozen full-output frame the marquee is drawn over. Cheap to clone
    /// (`Handle` holds refcounted RGBA).
    pub freeze: Option<iced::widget::image::Handle>,
    /// GPU upload of `freeze` finished; overlay may join composition.
    /// Stays false until the freeze layer has loaded the texture so the
    /// first visible frame is the still, not an empty/transparent flash.
    pub presentable: bool,
    /// Window that held keyboard focus when the marquee opened — restored
    /// on cancel / finish so we don't leave focus stuck on the hidden
    /// selection surface (which drops keyboard routing for the desktop).
    pub prior_focus: Option<u32>,
    /// Pointer down position in overlay/window space (compositor space
    /// when the surface is framed at 0,0 full output).
    pub drag_start: Option<(f32, f32)>,
    pub drag_current: Option<(f32, f32)>,
}

impl std::fmt::Debug for SelectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionState")
            .field("active", &self.active)
            .field("pending", &self.pending)
            .field("freeze_generation", &self.freeze_generation)
            .field("has_freeze", &self.freeze.is_some())
            .field("presentable", &self.presentable)
            .field("prior_focus", &self.prior_focus)
            .field("drag_start", &self.drag_start)
            .field("drag_current", &self.drag_current)
            .finish()
    }
}

impl SelectionState {
    /// Arm a freeze capture. Overlay stays hidden until [`Self::apply_freeze`].
    pub fn start_freeze(&mut self, prior_focus: Option<u32>) -> u64 {
        self.active = false;
        self.pending = true;
        self.prior_focus = prior_focus;
        self.freeze = None;
        self.presentable = false;
        self.drag_start = None;
        self.drag_current = None;
        self.freeze_generation = self.freeze_generation.wrapping_add(1);
        self.freeze_generation
    }

    /// Install the freeze frame and show the marquee. Returns false when the
    /// reply is stale (cancelled or superseded).
    pub fn apply_freeze(&mut self, generation: u64, handle: iced::widget::image::Handle) -> bool {
        if !self.pending || generation != self.freeze_generation {
            return false;
        }
        self.pending = false;
        self.freeze = Some(handle);
        self.presentable = false;
        self.active = true;
        self.drag_start = None;
        self.drag_current = None;
        true
    }

    pub fn begin(&mut self, prior_focus: Option<u32>) {
        self.active = true;
        self.pending = false;
        self.prior_focus = prior_focus;
        self.drag_start = None;
        self.drag_current = None;
    }

    /// End selection and return the focus window to restore (if any).
    pub fn cancel(&mut self) -> Option<u32> {
        self.active = false;
        self.pending = false;
        self.freeze = None;
        self.presentable = false;
        self.freeze_generation = self.freeze_generation.wrapping_add(1);
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
        assert!(!s.pending);
        assert_eq!(s.prior_focus, None);
    }

    #[test]
    fn stale_freeze_is_ignored() {
        let mut s = SelectionState::default();
        let generation = s.start_freeze(Some(1));
        assert!(s.pending);
        assert!(!s.active);
        let _ = s.cancel();
        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]);
        assert!(!s.apply_freeze(generation, handle));
        assert!(!s.active);
        assert!(s.freeze.is_none());
    }

    #[test]
    fn apply_freeze_shows_marquee() {
        let mut s = SelectionState::default();
        let generation = s.start_freeze(Some(9));
        let handle = iced::widget::image::Handle::from_rgba(1, 1, vec![1, 2, 3, 255]);
        assert!(s.apply_freeze(generation, handle));
        assert!(s.active);
        assert!(!s.pending);
        assert!(s.freeze.is_some());
        assert_eq!(s.prior_focus, Some(9));
    }
}
