/// Input device handling via libinput.
///
/// `libinput` is the standard Linux library for handling input devices:
/// keyboards, mice, touchpads, tablets, etc. It abstracts over the raw
/// kernel evdev interface and provides higher-level events like key
/// presses, pointer motion, and gestures.
///
/// In Phase 1, we handle compositor-level keybindings (like the kill chord)
/// and log other events. Later phases will route input to the focused
/// Wayland client via the seat protocol.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/libinput/index.html
/// See: https://wayland.freedesktop.org/libinput/doc/latest/
use smithay::backend::input::{InputEvent, KeyState, KeyboardKeyEvent};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::session::Session;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::reexports::calloop::LoopHandle;

use crate::Sola;

/// Key codes as reported by libinput on canto's Mac keyboard.
///
/// These are evdev codes offset by +8 from the raw Linux input codes
/// (libinput/XKB convention). Discovered empirically via key logging.
pub mod keycode {
    pub const BACKSPACE: u32 = 22;
    pub const LEFT_SHIFT: u32 = 50;
    // pub const RIGHT_SHIFT: u32 = 62;
    pub const LEFT_SUPER: u32 = 133; // Command (⌘) on Mac keyboard
    // pub const RIGHT_SUPER: u32 = 134;
}

/// Tracks which modifier keys are currently held down.
/// Updated on every key press/release event.
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
    /// No compositor action — pass the event through to clients.
    None,
    /// Shut down the compositor.
    Quit,
}

/// Check if a key event triggers a compositor-level action.
///
/// Keybindings are checked after modifiers are updated, so `modifiers`
/// reflects the current state including this key event.
pub fn check_binding(code: u32, pressed: bool, modifiers: &ModifierState) -> Action {
    // Super + Shift + Backspace → kill compositor.
    // Triggers on Backspace RELEASE while Super and Shift are held.
    // Release-based so you can't accidentally fire it mid-combo.
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
///
/// This creates a libinput context bound to the session's seat, which
/// automatically discovers all input devices attached to that seat.
pub fn setup(
    loop_handle: &LoopHandle<'static, Sola>,
    session: &LibSeatSession,
) -> anyhow::Result<()> {
    let seat_name = session.seat();

    // Create a libinput context that uses the libseat session for device access.
    // `LibinputSessionInterface` adapts our session to libinput's interface.
    let mut libinput_context =
        smithay::reexports::input::Libinput::new_with_udev(LibinputSessionInterface::from(
            session.clone(),
        ));

    // Assign the libinput context to our seat — this triggers device discovery.
    libinput_context
        .udev_assign_seat(&seat_name)
        .map_err(|_| anyhow::anyhow!("failed to assign libinput seat '{seat_name}'"))?;

    let libinput_backend = LibinputInputBackend::new(libinput_context);

    // Track modifier state across events. This lives inside the closure
    // because the event loop callback is the only consumer.
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
        .map_err(|e| anyhow::anyhow!("failed to insert libinput source: {e}"))?;

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

        // Backspace press — should NOT trigger (we want release).
        assert_eq!(check_binding(keycode::BACKSPACE, true, &m), Action::None);

        // Backspace release — should trigger.
        assert_eq!(check_binding(keycode::BACKSPACE, false, &m), Action::Quit);
    }

    #[test]
    fn kill_chord_requires_both_modifiers() {
        let mut m = ModifierState::default();

        // Only super held.
        m.update(keycode::LEFT_SUPER, true);
        assert_eq!(check_binding(keycode::BACKSPACE, false, &m), Action::None);

        // Only shift held.
        m = ModifierState::default();
        m.update(keycode::LEFT_SHIFT, true);
        assert_eq!(check_binding(keycode::BACKSPACE, false, &m), Action::None);

        // Neither held.
        m = ModifierState::default();
        assert_eq!(check_binding(keycode::BACKSPACE, false, &m), Action::None);
    }

    #[test]
    fn non_backspace_with_modifiers_does_nothing() {
        let mut m = ModifierState::default();
        m.update(keycode::LEFT_SUPER, true);
        m.update(keycode::LEFT_SHIFT, true);

        // Random key code 42 — not backspace.
        assert_eq!(check_binding(42, false, &m), Action::None);
    }
}
