/// Input device plumbing via libinput.
///
/// Sets up libinput, forwards keyboard/pointer events through the Wayland
/// seat. Super+key events are sent directly to sola-shell's keyboard_target
/// surface via wl_keyboard.key, bypassing Smithay's focus mechanism.
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
use smithay::reexports::wayland_server::Resource;
use smithay::utils::SERIAL_COUNTER;

use crate::State;
use crate::error::InputError;
use crate::input::binding::ModifierState;

/// Set up libinput and register it as a calloop event source.
pub fn setup(
    loop_handle: &LoopHandle<'static, State>,
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
        .insert_source(libinput_backend, move |event, _, state| {
            match event {
                InputEvent::Keyboard { event } => {
                    let pressed = event.state() == KeyState::Pressed;
                    let was_super_held = modifiers.super_held;

                    modifiers.update(event.key_code().raw(), pressed);

                    let route_to_shell = modifiers.super_held
                        || (was_super_held && !modifiers.super_held);

                    if route_to_shell {
                        send_to_shell(state, event.key_code(), event.state(), event.time_msec());
                        return;
                    }

                    let serial = SERIAL_COUNTER.next_serial();
                    let time = event.time_msec();
                    let keyboard = state.seat.get_keyboard().unwrap();
                    keyboard.input::<(), _>(
                        state,
                        event.key_code(),
                        event.state(),
                        serial,
                        time,
                        |_, _, _| FilterResult::Forward,
                    );
                }

                InputEvent::PointerMotion { event } => {
                    let delta = event.delta();
                    let (max_x, max_y) = output_size(state);
                    state.pointer_location.0 = (state.pointer_location.0 + delta.x).clamp(0.0, max_x);
                    state.pointer_location.1 = (state.pointer_location.1 + delta.y).clamp(0.0, max_y);
                    forward_pointer_motion(state);
                }

                InputEvent::PointerMotionAbsolute { event } => {
                    let (max_x, max_y) = output_size(state);
                    state.pointer_location = (
                        event.x_transformed(max_x as i32) as f64,
                        event.y_transformed(max_y as i32) as f64,
                    );
                    forward_pointer_motion(state);
                }

                InputEvent::PointerButton { event } => {
                    let serial = SERIAL_COUNTER.next_serial();
                    let pointer = state.seat.get_pointer().unwrap();
                    pointer.button(
                        state,
                        &ButtonEvent {
                            serial,
                            time: event.time_msec(),
                            button: event.button_code(),
                            state: event.state(),
                        },
                    );
                    pointer.frame(state);
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

                    let pointer = state.seat.get_pointer().unwrap();
                    pointer.axis(state, frame);
                    pointer.frame(state);
                }

                _ => {}
            }
        })
        .map_err(|e| InputError::EventSource(e.to_string()))?;

    tracing::info!("libinput initialized for seat '{seat_name}'");
    Ok(())
}

/// Send a key event directly to sola-shell's keyboard_target surface.
///
/// Uses wl_keyboard.key on the shell client's keyboard resources,
/// bypassing Smithay's focus mechanism. The focused app never sees
/// these events.
fn send_to_shell(
    state: &mut State,
    keycode: smithay::input::keyboard::Keycode,
    key_state: KeyState,
    time: u32,
) {
    use smithay::reexports::wayland_server::protocol::wl_keyboard;

    let surface = match state.shell_keyboard_target {
        Some(ref s) => s.clone(),
        None => return,
    };

    let client = match surface.client() {
        Some(c) => c,
        None => {
            state.shell_keyboard_target = None;
            return;
        }
    };

    let keyboard = state.seat.get_keyboard().unwrap();

    let ((), mods_changed) = keyboard.input_intercept(
        state,
        keycode,
        key_state,
        |_, _, _| (),
    );
    let mods = keyboard.modifier_state();

    let serial = SERIAL_COUNTER.next_serial();
    let evdev_code = keycode.raw() - 8;
    let wl_state = match key_state {
        KeyState::Pressed => wl_keyboard::KeyState::Pressed,
        KeyState::Released => wl_keyboard::KeyState::Released,
    };

    for kbd in keyboard.client_keyboards(&client) {
        kbd.key(serial.into(), time, evdev_code, wl_state);
        if mods_changed {
            kbd.modifiers(
                serial.into(),
                mods.serialized.depressed,
                mods.serialized.latched,
                mods.serialized.locked,
                mods.serialized.layout_effective,
            );
        }
    }
}

/// Forward the current pointer position through the seat to the client.
fn forward_pointer_motion(state: &mut State) {
    let (x, y) = state.pointer_location;
    let serial = SERIAL_COUNTER.next_serial();

    let under = state.space.element_under((x, y)).and_then(|(window, loc)| {
        window
            .surface_under(
                (x - loc.x as f64, y - loc.y as f64),
                WindowSurfaceType::ALL,
            )
            .map(|(surface, offset)| (surface, (loc + offset).to_f64()))
    });

    let pointer = state.seat.get_pointer().unwrap();
    pointer.motion(
        state,
        under,
        &MotionEvent {
            location: (x, y).into(),
            serial,
            time: 0,
        },
    );
    pointer.frame(state);
}

/// Get the output size for clamping pointer position.
fn output_size(state: &State) -> (f64, f64) {
    state.space
        .outputs()
        .next()
        .and_then(|o| o.current_mode())
        .map(|m| (m.size.w as f64, m.size.h as f64))
        .unwrap_or((1920.0, 1080.0))
}
