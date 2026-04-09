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
mod keycode {
    pub const BACKSPACE: u32 = 22;
    pub const LEFT_SHIFT: u32 = 50;
    // pub const RIGHT_SHIFT: u32 = 62;
    pub const LEFT_SUPER: u32 = 133;  // Command (⌘) on Mac keyboard
    // pub const RIGHT_SUPER: u32 = 134;
}

/// Tracks which modifier keys are currently held down.
/// Updated on every key press/release event.
#[derive(Default)]
struct ModifierState {
    super_held: bool,
    shift_held: bool,
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

                // Update modifier tracking before logging.
                match code {
                    keycode::LEFT_SUPER => modifiers.super_held = pressed,
                    keycode::LEFT_SHIFT => modifiers.shift_held = pressed,
                    _ => {}
                }

                tracing::debug!(
                    code,
                    state = if pressed { "pressed" } else { "released" },
                    super_held = modifiers.super_held,
                    shift_held = modifiers.shift_held,
                    "key event"
                );

                // Super + Shift + Backspace → kill compositor.
                // Triggers on Backspace RELEASE while Super and Shift are held.
                // Release-based so you can't accidentally fire it mid-combo.
                if !pressed
                    && code == keycode::BACKSPACE
                    && modifiers.super_held
                    && modifiers.shift_held
                {
                    tracing::info!("kill chord (Super+Shift+Backspace released), shutting down");
                    sola.running = false;
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to insert libinput source: {e}"))?;

    tracing::info!("libinput initialized for seat '{seat_name}'");
    Ok(())
}
