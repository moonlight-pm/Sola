/// Wayland data device (clipboard and drag-and-drop) protocol handler.
///
/// The data device protocol manages two things:
/// - **Clipboard**: copy/paste between clients
/// - **Drag and drop (DnD)**: dragging content between surfaces
///
/// Smithay splits DnD into two handler traits:
/// - `ClientDndGrabHandler` — the client initiated a drag
/// - `ServerDndGrabHandler` — the compositor initiated a drag
///
/// Both are required by `DataDeviceHandler`. The `SelectionHandler` trait
/// manages clipboard state and is also required as a supertrait.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/selection/data_device/trait.DataDeviceHandler.html
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;

use crate::state::Sola;

impl SelectionHandler for Sola {
    /// User data attached to server-side selections. We don't use server-side
    /// selections in Phase 1, so unit type suffices.
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
