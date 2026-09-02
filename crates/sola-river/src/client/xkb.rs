//! Dispatch for `river_input_manager_v1` and `river_xkb_config_v1`.
//!
//! River starts each keyboard with NumLock off. On a PC keymap that makes
//! the number pad emit navigation keysyms (`KP_Home`, `KP_End`, arrows, …)
//! instead of digits, so clients never see `0`–`9` from those keys.
//!
//! We bind the xkb-config global and turn NumLock on for every keyboard as
//! it appears (session start, hotplug, TTY return). The user can still
//! press NumLock to switch the pad to navigation. Super+Numpad zoning
//! already registers both keysym sets, so it is unaffected.

use tracing::{debug, info};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};

use crate::client::AppData;
use crate::protocol::river_input_management_v1::{
    river_input_device_v1::{self, RiverInputDeviceV1},
    river_input_manager_v1::{self as mgr_iface, RiverInputManagerV1},
};
use crate::protocol::river_xkb_config_v1::{
    river_xkb_config_v1::{self as xkb_iface, RiverXkbConfigV1},
    river_xkb_keyboard_v1::{self, RiverXkbKeyboardV1},
};

// Event opcodes match XML order (`finished` is 0, child-object event is 1).
const EVT_INPUT_DEVICE_OPCODE: u16 = 1;
const EVT_XKB_KEYBOARD_OPCODE: u16 = 1;

impl Dispatch<RiverInputManagerV1, ()> for AppData {
    fn event(
        _state: &mut Self,
        _mgr: &RiverInputManagerV1,
        event: <RiverInputManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            mgr_iface::Event::Finished => {
                info!("river_input_manager_v1 finished");
            }
            _ => {}
        }
    }

    event_created_child!(AppData, RiverInputManagerV1, [
        EVT_INPUT_DEVICE_OPCODE => (RiverInputDeviceV1, ()),
    ]);
}

impl Dispatch<RiverInputDeviceV1, ()> for AppData {
    fn event(
        _state: &mut Self,
        device: &RiverInputDeviceV1,
        event: <RiverInputDeviceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            river_input_device_v1::Event::Removed => {
                device.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<RiverXkbConfigV1, ()> for AppData {
    fn event(
        _state: &mut Self,
        _cfg: &RiverXkbConfigV1,
        event: <RiverXkbConfigV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            xkb_iface::Event::Finished => {
                info!("river_xkb_config_v1 finished");
            }
            xkb_iface::Event::XkbKeyboard { id: keyboard } => {
                // Enable here (not only on the child's `input_device`) so
                // NumLock still turns on if that object-ref event is
                // delayed relative to our input-manager bind.
                keyboard.numlock_enable();
                info!("numlock enabled by default");
            }
        }
    }

    event_created_child!(AppData, RiverXkbConfigV1, [
        EVT_XKB_KEYBOARD_OPCODE => (RiverXkbKeyboardV1, ()),
    ]);
}

impl Dispatch<RiverXkbKeyboardV1, ()> for AppData {
    fn event(
        _state: &mut Self,
        keyboard: &RiverXkbKeyboardV1,
        event: <RiverXkbKeyboardV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            river_xkb_keyboard_v1::Event::NumlockEnabled => {
                debug!("numlock on");
            }
            river_xkb_keyboard_v1::Event::NumlockDisabled => {
                debug!("numlock off");
            }
            river_xkb_keyboard_v1::Event::Removed => {
                keyboard.destroy();
            }
            _ => {}
        }
    }
}
