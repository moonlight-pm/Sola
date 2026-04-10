/// Server-side Wayland protocols for XWayland.
///
/// sola-x acts as a minimal Wayland compositor that XWayland connects to.
/// Only the protocols XWayland needs are implemented here.
pub mod compositor;
pub mod seat;
pub mod shm;
pub mod xwayland;

use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::xdg::{XdgShellHandler, XdgShellState};

use crate::state::State;

// -- Data device (clipboard/DnD) --

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

// -- Output --

impl smithay::wayland::output::OutputHandler for State {}
smithay::delegate_output!(State);

// -- XDG shell (needed for XWayland surface management) --

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(
        &mut self,
        _surface: smithay::wayland::shell::xdg::ToplevelSurface,
    ) {
    }

    fn new_popup(
        &mut self,
        _surface: smithay::wayland::shell::xdg::PopupSurface,
        _positioner: smithay::wayland::shell::xdg::PositionerState,
    ) {
    }

    fn grab(
        &mut self,
        _surface: smithay::wayland::shell::xdg::PopupSurface,
        _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
        _serial: smithay::utils::Serial,
    ) {
    }

    fn reposition_request(
        &mut self,
        _surface: smithay::wayland::shell::xdg::PopupSurface,
        _positioner: smithay::wayland::shell::xdg::PositionerState,
        _token: u32,
    ) {
    }
}

smithay::delegate_xdg_shell!(State);
