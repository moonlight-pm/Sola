/// Input device plumbing via libinput.
///
/// Sets up libinput, forwards keyboard/pointer events through the Wayland
/// seat. Keybinding logic (what actions keys trigger) lives in
/// `input::binding`.
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
use crate::input::binding::{self, Action, ModifierState};

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

                    match binding::check(code, pressed, &modifiers) {
                        Action::Quit => {
                            tracing::info!("kill chord, shutting down");
                            sola.running = false;
                            return;
                        }
                        Action::None => {}
                    }

                    // Forward to focused client.
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
                    let delta = event.delta();
                    let (max_x, max_y) = output_size(sola);
                    sola.pointer_location.0 = (sola.pointer_location.0 + delta.x).clamp(0.0, max_x);
                    sola.pointer_location.1 = (sola.pointer_location.1 + delta.y).clamp(0.0, max_y);
                    forward_pointer_motion(sola);
                }

                InputEvent::PointerMotionAbsolute { event } => {
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
    use smithay::wayland::seat::WaylandFocus;

    let (x, y) = sola.pointer_location;
    let serial = SERIAL_COUNTER.next_serial();

    let under = sola.space.element_under((x, y)).and_then(|(window, loc)| {
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
