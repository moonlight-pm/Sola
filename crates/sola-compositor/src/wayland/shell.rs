/// XDG shell protocol handler.
///
/// The XDG shell defines desktop window roles:
/// - **Toplevel**: a regular window (maximize, minimize, fullscreen)
/// - **Popup**: a transient surface anchored to a parent (menus, tooltips)
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/shell/xdg/trait.XdgShellHandler.html
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::utils::Serial;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};

use crate::state::Sola;

impl XdgShellHandler for Sola {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, _surface: ToplevelSurface) {}
    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}
    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}
    fn reposition_request(&mut self, _surface: PopupSurface, _positioner: PositionerState, _token: u32) {}
}

smithay::delegate_xdg_shell!(Sola);
