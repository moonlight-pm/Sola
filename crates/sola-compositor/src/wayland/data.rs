/// Wayland data device (clipboard and drag-and-drop) protocol handler.
///
/// Manages clipboard (copy/paste) and drag-and-drop between clients.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/selection/data_device/trait.DataDeviceHandler.html
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;

use crate::state::Sola;

impl SelectionHandler for Sola {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Sola {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Sola {}
impl ServerDndGrabHandler for Sola {}

smithay::delegate_data_device!(Sola);
