/// Input device plumbing via libinput.
///
/// Sets up libinput, forwards keyboard/pointer events through the Wayland
/// seat. Keybinding logic (what actions keys trigger) lives in
/// `input::binding`.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/libinput/index.html
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, Event, InputEvent, KeyState, KeyboardKeyEvent,
    PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Session;
use smithay::desktop::WindowSurfaceType;
use smithay::input::keyboard::FilterResult;
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
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
                    pointer.frame(sola);
                }

                InputEvent::PointerAxis { event } => {
                    let source = event.source();
                    let mut frame = AxisFrame::new(event.time_msec()).source(source);

                    // Horizontal axis.
                    let h_amount = event.amount(Axis::Horizontal)
                        .or_else(|| event.amount_v120(Axis::Horizontal).map(|v| v * 3.0 / 120.0));
                    if let Some(h) = h_amount {
                        frame = frame.value(Axis::Horizontal, h);
                        if let Some(v120) = event.amount_v120(Axis::Horizontal) {
                            frame = frame.v120(Axis::Horizontal, v120 as i32);
                        }
                        frame = frame.relative_direction(
                            Axis::Horizontal,
                            event.relative_direction(Axis::Horizontal),
                        );
                    }

                    // Vertical axis.
                    let v_amount = event.amount(Axis::Vertical)
                        .or_else(|| event.amount_v120(Axis::Vertical).map(|v| v * 3.0 / 120.0));
                    if let Some(v) = v_amount {
                        frame = frame.value(Axis::Vertical, -v);
                        if let Some(v120) = event.amount_v120(Axis::Vertical) {
                            frame = frame.v120(Axis::Vertical, -(v120 as i32));
                        }
                        frame = frame.relative_direction(
                            Axis::Vertical,
                            event.relative_direction(Axis::Vertical),
                        );
                    }

                    // Finger source sends a stop event when the finger lifts.
                    if source == AxisSource::Finger {
                        if event.amount(Axis::Horizontal) == Some(0.0) {
                            frame = frame.stop(Axis::Horizontal);
                        }
                        if event.amount(Axis::Vertical) == Some(0.0) {
                            frame = frame.stop(Axis::Vertical);
                        }
                    }

                    let pointer = sola.seat.get_pointer().unwrap();
                    pointer.axis(sola, frame);
                    pointer.frame(sola);
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

    let under = sola.space.element_under((x, y)).and_then(|(window, loc)| {
        window
            .surface_under(
                (x - loc.x as f64, y - loc.y as f64),
                WindowSurfaceType::ALL,
            )
            .map(|(surface, offset)| (surface, (loc + offset).to_f64()))
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
    pointer.frame(sola);
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
