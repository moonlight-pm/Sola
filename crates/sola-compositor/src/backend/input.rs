/// Input device handling via libinput.
///
/// Sets up libinput, tracks modifier state, checks compositor keybindings,
/// and forwards keyboard/pointer events through the Wayland seat so
/// focused clients receive them.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/libinput/index.html
use smithay::backend::input::{
    AbsolutePositionEvent, Event, InputEvent, KeyState, KeyboardKeyEvent,
    PointerButtonEvent, PointerMotionEvent,
};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Session;
use smithay::input::keyboard::FilterResult;
use smithay::input::pointer::{ButtonEvent, MotionEvent};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::input::Libinput;
use smithay::utils::SERIAL_COUNTER;

use crate::Sola;
use crate::error::InputError;

/// Key codes (evdev + 8 offset, XKB convention).
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
            match event {
                InputEvent::Keyboard { event } => {
                    let code = event.key_code().raw();
                    let pressed = event.state() == KeyState::Pressed;

                    modifiers.update(code, pressed);

                    tracing::debug!(code, pressed, "key event");

                    match check_binding(code, pressed, &modifiers) {
                        Action::Quit => {
                            tracing::info!("kill chord, shutting down");
                            sola.running = false;
                            return;
                        }
                        Action::None => {}
                    }

                    // Forward keyboard event through the seat to the focused client.
                    let serial = SERIAL_COUNTER.next_serial();
                    let time = event.time_msec();
                    {
                        let keyboard = sola.seat.get_keyboard().unwrap();
                        keyboard.input::<(), _>(
                            sola,
                            event.key_code(),
                            event.state(),
                            serial,
                            time,
                            |_, _, _| FilterResult::Forward,
                        );
                    }
                }

                InputEvent::PointerMotion { event } => {
                    // Relative motion (mouse). Accumulate into pointer position.
                    let delta = event.delta();
                    let (max_x, max_y) = output_size(sola);
                    sola.pointer_location.0 = (sola.pointer_location.0 + delta.x).clamp(0.0, max_x);
                    sola.pointer_location.1 = (sola.pointer_location.1 + delta.y).clamp(0.0, max_y);

                    forward_pointer_motion(sola);
                }

                InputEvent::PointerMotionAbsolute { event } => {
                    // Absolute motion (touchpad or tablet).
                    let (max_x, max_y) = output_size(sola);
                    sola.pointer_location = (
                        event.x_transformed(max_x as i32) as f64,
                        event.y_transformed(max_y as i32) as f64,
                    );

                    forward_pointer_motion(sola);
                }

                InputEvent::PointerButton { event } => {
                    let serial = SERIAL_COUNTER.next_serial();
                    let pointer = sola.seat.get_pointer().unwrap();
                    pointer.button(
                        sola,
                        &ButtonEvent {
                            serial,
                            time: event.time_msec(),
                            button: event.button_code(),
                            state: event.state(),
                        },
                    );
                }

                _ => {}
            }
        })
        .map_err(|e| InputError::EventSource(e.to_string()))?;

    tracing::info!("libinput initialized for seat '{seat_name}'");
    Ok(())
}

/// Forward the current pointer position through the seat to the client.
fn forward_pointer_motion(sola: &mut Sola) {
    let (x, y) = sola.pointer_location;
    let serial = SERIAL_COUNTER.next_serial();

    // Find what's under the pointer in the Space.
    // Use WaylandFocus::wl_surface() which works for both Wayland
    // toplevels and X11 windows (via XWayland).
    let under = sola.space.element_under((x, y)).and_then(|(window, loc)| {
        use smithay::wayland::seat::WaylandFocus;
        let surface = window.wl_surface()?.into_owned();
        Some((surface, loc.to_f64()))
    });

    let pointer = sola.seat.get_pointer().unwrap();
    pointer.motion(
        sola,
        under,
        &MotionEvent {
            location: (x, y).into(),
            serial,
            time: 0,
        },
    );
}

/// Get the output size for clamping pointer position.
fn output_size(sola: &Sola) -> (f64, f64) {
    sola.space
        .outputs()
        .next()
        .and_then(|o| o.current_mode())
        .map(|m| (m.size.w as f64, m.size.h as f64))
        .unwrap_or((1920.0, 1080.0))
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
