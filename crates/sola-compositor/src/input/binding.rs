/// Compositor keybindings — maps key events to compositor actions.
///
/// This is "novel" code (Sola-specific logic), separate from the
/// "standard" libinput plumbing in `backend/input.rs`.

/// Key codes (evdev + 8 offset, XKB convention).
/// Discovered empirically on canto's Mac keyboard.
pub mod keycode {
    pub const TAB: u32 = 23;
    pub const BACKSPACE: u32 = 22;
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

/// Compositor-level action triggered by a keybinding.
#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Quit,
    ShowSwitcher,
}

/// Check if a key event triggers a compositor-level action.
pub fn check(code: u32, pressed: bool, modifiers: &ModifierState) -> Action {
    // Super + Shift + Backspace → quit (on release).
    if !pressed
        && code == keycode::BACKSPACE
        && modifiers.super_held
        && modifiers.shift_held
    {
        return Action::Quit;
    }

    // Super + Tab → show switcher (on press).
    if pressed && code == keycode::TAB && modifiers.super_held {
        return Action::ShowSwitcher;
    }

    Action::None
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
        let consumed = m.update(keycode::BACKSPACE, true);
        assert!(!consumed);
        assert!(!m.super_held);
        assert!(!m.shift_held);
    }

    #[test]
    fn kill_chord_triggers_on_backspace_release_with_modifiers() {
        let mut m = ModifierState::default();
        m.update(keycode::LEFT_SUPER, true);
        m.update(keycode::LEFT_SHIFT, true);
        assert_eq!(check(keycode::BACKSPACE, true, &m), Action::None);
        assert_eq!(check(keycode::BACKSPACE, false, &m), Action::Quit);
    }

    #[test]
    fn kill_chord_requires_both_modifiers() {
        let mut m = ModifierState::default();
        m.update(keycode::LEFT_SUPER, true);
        assert_eq!(check(keycode::BACKSPACE, false, &m), Action::None);

        m = ModifierState::default();
        m.update(keycode::LEFT_SHIFT, true);
        assert_eq!(check(keycode::BACKSPACE, false, &m), Action::None);

        m = ModifierState::default();
        assert_eq!(check(keycode::BACKSPACE, false, &m), Action::None);
    }

    #[test]
    fn non_backspace_with_modifiers_does_nothing() {
        let mut m = ModifierState::default();
        m.update(keycode::LEFT_SUPER, true);
        m.update(keycode::LEFT_SHIFT, true);
        assert_eq!(check(42, false, &m), Action::None);
    }

    #[test]
    fn super_tab_triggers_show_switcher_on_press() {
        let mut m = ModifierState::default();
        m.update(keycode::LEFT_SUPER, true);
        assert_eq!(check(keycode::TAB, true, &m), Action::ShowSwitcher);
    }

    #[test]
    fn super_tab_does_not_trigger_on_release() {
        let mut m = ModifierState::default();
        m.update(keycode::LEFT_SUPER, true);
        assert_eq!(check(keycode::TAB, false, &m), Action::None);
    }

    #[test]
    fn tab_without_super_does_nothing() {
        let m = ModifierState::default();
        assert_eq!(check(keycode::TAB, true, &m), Action::None);
    }
}
