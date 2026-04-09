/// Input device handling via libinput.
///
/// Sets up libinput, tracks modifier state, and dispatches raw key events.
/// Keybinding logic (what actions keys trigger) lives here temporarily
/// but will move to a shell layer as the compositor grows.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/libinput/index.html
use smithay::backend::input::{InputEvent, KeyState, KeyboardKeyEvent};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Session;
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::input::Libinput;

use crate::Sola;
use crate::error::InputError;

/// Key codes as reported by libinput on canto's Mac keyboard.
///
/// Evdev codes offset by +8 from raw Linux input codes (XKB convention).
/// Discovered empirically via key logging.
pub mod keycode {
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
}

/// Check if a key event triggers a compositor-level action.
pub fn check_binding(code: u32, pressed: bool, modifiers: &ModifierState) -> Action {
    // Super + Shift + Backspace → quit (on release).
    if !pressed
        && code == keycode::BACKSPACE
        && modifiers.super_held
        && modifiers.shift_held
    {
        return Action::Quit;
    }
    Action::None
}

/// Set up libinput and register it as a calloop event source.
pub fn setup(
    loop_handle: &LoopHandle<'static, Sola>,
    session: &LibSeatSession,
) -> Result<(), InputError> {
    let seat_name = session.seat();

    let mut libinput_context =
        Libinput::new_with_udev(LibinputSessionInterface::from(session.clone()));

    libinput_context
        .udev_assign_seat(&seat_name)
        .map_err(|_| InputError::SeatAssign {
            seat: seat_name.clone(),
        })?;

    let libinput_backend = LibinputInputBackend::new(libinput_context);
    let mut modifiers = ModifierState::default();

    loop_handle
        .insert_source(libinput_backend, move |event, _, sola| {
            if let InputEvent::Keyboard { event } = event {
                let code = event.key_code().raw();
                let pressed = event.state() == KeyState::Pressed;

                modifiers.update(code, pressed);

                tracing::debug!(
                    code,
                    state = if pressed { "pressed" } else { "released" },
                    super_held = modifiers.super_held,
                    shift_held = modifiers.shift_held,
                    "key event"
                );

                match check_binding(code, pressed, &modifiers) {
                    Action::Quit => {
                        tracing::info!(
                            "kill chord (Super+Shift+Backspace released), shutting down"
                        );
                        sola.running = false;
                    }
                    Action::None => {}
                }
            }
        })
        .map_err(|e| InputError::EventSource(e.to_string()))?;

    tracing::info!("libinput initialized for seat '{seat_name}'");
    Ok(())
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
        assert_eq!(check_binding(keycode::BACKSPACE, true, &m), Action::None);
        assert_eq!(check_binding(keycode::BACKSPACE, false, &m), Action::Quit);
    }

    #[test]
    fn kill_chord_requires_both_modifiers() {
        let mut m = ModifierState::default();
        m.update(keycode::LEFT_SUPER, true);
        assert_eq!(check_binding(keycode::BACKSPACE, false, &m), Action::None);

        m = ModifierState::default();
        m.update(keycode::LEFT_SHIFT, true);
        assert_eq!(check_binding(keycode::BACKSPACE, false, &m), Action::None);

        m = ModifierState::default();
        assert_eq!(check_binding(keycode::BACKSPACE, false, &m), Action::None);
    }

    #[test]
    fn non_backspace_with_modifiers_does_nothing() {
        let mut m = ModifierState::default();
        m.update(keycode::LEFT_SUPER, true);
        m.update(keycode::LEFT_SHIFT, true);
        assert_eq!(check_binding(42, false, &m), Action::None);
    }
}
