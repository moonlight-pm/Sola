/// Compositor input routing — tracks modifier state for bus dispatch.
///
/// The compositor's only input policy: if Super is held, send the key
/// to the bus instead of the focused Wayland client. All key binding
/// logic lives in shell apps, not here.

/// Key codes (evdev + 8 offset, XKB convention).
pub mod keycode {
    pub const LEFT_SHIFT: u32 = 50;
    pub const LEFT_SUPER: u32 = 133;
}

/// Tracks which modifier keys are currently held down.
#[derive(Default, Debug, Clone)]
pub struct ModifierState {
    pub super_held: bool,
    pub shift_held: bool,
}

impl ModifierState {
    /// Update modifier tracking for a key event.
    /// Returns `true` if the key was a modifier (and was consumed).
    pub fn update(&mut self, code: u32, pressed: bool) -> bool {
        match code {
            keycode::LEFT_SUPER => {
                self.super_held = pressed;
                true
            }
            keycode::LEFT_SHIFT => {
                self.shift_held = pressed;
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
    fn modifier_tracks_super() {
        let mut m = ModifierState::default();
        assert!(!m.super_held);
        m.update(keycode::LEFT_SUPER, true);
        assert!(m.super_held);
        m.update(keycode::LEFT_SUPER, false);
        assert!(!m.super_held);
    }

    #[test]
    fn modifier_tracks_shift() {
        let mut m = ModifierState::default();
        assert!(!m.shift_held);
        m.update(keycode::LEFT_SHIFT, true);
        assert!(m.shift_held);
        m.update(keycode::LEFT_SHIFT, false);
        assert!(!m.shift_held);
    }

    #[test]
    fn modifier_ignores_other_keys() {
        let mut m = ModifierState::default();
        let consumed = m.update(42, true);
        assert!(!consumed);
        assert!(!m.super_held);
        assert!(!m.shift_held);
    }

    #[test]
    fn super_press_is_consumed() {
        let mut m = ModifierState::default();
        assert!(m.update(keycode::LEFT_SUPER, true));
    }

    #[test]
    fn regular_key_not_consumed() {
        let mut m = ModifierState::default();
        assert!(!m.update(23, true)); // Tab
    }
}
