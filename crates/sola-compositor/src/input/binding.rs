/// Compositor input routing — tracks modifier state for bus dispatch.
///
/// The compositor's only input policy: if Meta is held, send the key
/// to the bus instead of the focused Wayland client. All key binding
/// logic lives in shell apps, not here.

/// Key codes (evdev + 8 offset, XKB convention).
pub mod keycode {
    use sola_core::KeyCode;

    pub const LEFT_CTRL: u32 = KeyCode::LEFT_CTRL.raw();
    pub const RIGHT_CTRL: u32 = KeyCode::RIGHT_CTRL.raw();
    pub const LEFT_SHIFT: u32 = KeyCode::LEFT_SHIFT.raw();
    pub const RIGHT_SHIFT: u32 = KeyCode::RIGHT_SHIFT.raw();
    pub const LEFT_ALT: u32 = KeyCode::LEFT_ALT.raw();
    pub const RIGHT_ALT: u32 = KeyCode::RIGHT_ALT.raw();
    pub const LEFT_META: u32 = KeyCode::LEFT_META.raw();
    pub const RIGHT_META: u32 = KeyCode::RIGHT_META.raw();
}

/// Tracks which modifier keys are currently held down.
#[derive(Default, Debug, Clone)]
pub struct ModifierState {
    pub meta_held: bool,
    pub shift_held: bool,
    pub ctrl_held: bool,
    pub alt_held: bool,
}

impl ModifierState {
    /// Update modifier tracking for a key event.
    /// Returns `true` if the key was a modifier (and was consumed).
    pub fn update(&mut self, code: u32, pressed: bool) -> bool {
        match code {
            keycode::LEFT_META | keycode::RIGHT_META => {
                self.meta_held = pressed;
                true
            }
            keycode::LEFT_SHIFT | keycode::RIGHT_SHIFT => {
                self.shift_held = pressed;
                true
            }
            keycode::LEFT_CTRL | keycode::RIGHT_CTRL => {
                self.ctrl_held = pressed;
                true
            }
            keycode::LEFT_ALT | keycode::RIGHT_ALT => {
                self.alt_held = pressed;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_tracks_meta() {
        let mut m = ModifierState::default();
        assert!(!m.meta_held);
        m.update(keycode::LEFT_META, true);
        assert!(m.meta_held);
        m.update(keycode::LEFT_META, false);
        assert!(!m.meta_held);
    }

    #[test]
    fn modifier_tracks_shift() {
        let mut m = ModifierState::default();
        assert!(!m.shift_held);
        m.update(keycode::LEFT_SHIFT, true);
        assert!(m.shift_held);
        m.update(keycode::LEFT_SHIFT, false);
        assert!(!m.shift_held);
        assert!(!m.ctrl_held);
        assert!(!m.alt_held);
    }

    #[test]
    fn modifier_ignores_other_keys() {
        let mut m = ModifierState::default();
        let consumed = m.update(42, true);
        assert!(!consumed);
        assert!(!m.meta_held);
        assert!(!m.shift_held);
    }

    #[test]
    fn meta_press_is_consumed() {
        let mut m = ModifierState::default();
        assert!(m.update(keycode::LEFT_META, true));
        assert!(m.update(keycode::RIGHT_META, true));
    }

    #[test]
    fn ctrl_tracks_left_and_right() {
        let mut m = ModifierState::default();
        assert!(!m.ctrl_held);
        assert!(m.update(keycode::LEFT_CTRL, true));
        assert!(m.ctrl_held);
        assert!(m.update(keycode::RIGHT_CTRL, true));
        assert!(m.ctrl_held);
        assert!(m.update(keycode::LEFT_CTRL, false));
        assert!(!m.ctrl_held);
    }

    #[test]
    fn alt_tracks_left_and_right() {
        let mut m = ModifierState::default();
        assert!(!m.alt_held);
        assert!(m.update(keycode::LEFT_ALT, true));
        assert!(m.alt_held);
        assert!(m.update(keycode::RIGHT_ALT, true));
        assert!(m.alt_held);
        assert!(m.update(keycode::LEFT_ALT, false));
        assert!(!m.alt_held);
    }

    #[test]
    fn regular_key_not_consumed() {
        let mut m = ModifierState::default();
        assert!(!m.update(23, true)); // Tab
        assert!(!m.meta_held);
        assert!(!m.shift_held);
        assert!(!m.ctrl_held);
        assert!(!m.alt_held);
    }
}
