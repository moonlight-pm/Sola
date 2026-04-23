//! Dispatch for `river_libinput_config_v1` and its child devices.
//!
//! We bind this global to apply session-wide libinput preferences — for
//! now, natural scroll on every device that supports it. Devices without
//! natural-scroll support (keyboards, for example) silently skip the
//! `set_natural_scroll` call.

use tracing::{debug, info, warn};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};

use crate::client::AppData;
use crate::protocol::river_libinput_config_v1::{
    river_libinput_config_v1::{self as cfg_iface, RiverLibinputConfigV1},
    river_libinput_device_v1::{self, NaturalScrollState, RiverLibinputDeviceV1},
    river_libinput_result_v1::{self, RiverLibinputResultV1},
};

// Opcode of the `libinput_device` event on `river_libinput_config_v1`,
// matching the order in river-libinput-config-v1.xml (`finished` is 0,
// `libinput_device` is 1). The scanner uses this to initialise new
// child proxies when the event fires.
const EVT_LIBINPUT_DEVICE_OPCODE: u16 = 1;

impl Dispatch<RiverLibinputConfigV1, ()> for AppData {
    fn event(
        _state: &mut Self,
        _cfg: &RiverLibinputConfigV1,
        event: <RiverLibinputConfigV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            cfg_iface::Event::Finished => {
                info!("river_libinput_config_v1 finished");
            }
            _ => {}
        }
    }

    event_created_child!(AppData, RiverLibinputConfigV1, [
        EVT_LIBINPUT_DEVICE_OPCODE => (RiverLibinputDeviceV1, ()),
    ]);
}

impl Dispatch<RiverLibinputDeviceV1, ()> for AppData {
    fn event(
        _state: &mut Self,
        device: &RiverLibinputDeviceV1,
        event: <RiverLibinputDeviceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            river_libinput_device_v1::Event::NaturalScrollSupport { supported } => {
                if supported != 0 {
                    debug!(device = ?device.id(), "enabling natural scroll");
                    device.set_natural_scroll(NaturalScrollState::Enabled, qh, ());
                }
            }
            river_libinput_device_v1::Event::Removed => {
                device.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<RiverLibinputResultV1, ()> for AppData {
    fn event(
        _state: &mut Self,
        _result: &RiverLibinputResultV1,
        event: <RiverLibinputResultV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // All three variants are destructor events — the server tears the
        // object down for us. Just log the outcome.
        match event {
            river_libinput_result_v1::Event::Success => {
                debug!("libinput config applied");
            }
            river_libinput_result_v1::Event::Unsupported => {
                debug!("libinput config unsupported by device");
            }
            river_libinput_result_v1::Event::Invalid => {
                warn!("libinput config rejected as invalid");
            }
        }
    }
}
