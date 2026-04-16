use smithay::wayland::selection::SelectionHandler;
/// Wayland data device (clipboard and drag-and-drop) protocol handler.
///
/// Manages clipboard (copy/paste) and drag-and-drop between clients.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/selection/data_device/trait.DataDeviceHandler.html
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};

use crate::state::State;

impl SelectionHandler for State {
    type SelectionUserData = ();
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for State {}
impl ServerDndGrabHandler for State {}

smithay::delegate_data_device!(State);
