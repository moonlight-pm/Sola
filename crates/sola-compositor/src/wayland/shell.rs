/// XDG shell protocol handler.
///
/// The XDG shell is the standard Wayland protocol for desktop windows. It
/// defines two surface roles:
///
/// - **Toplevel**: a regular window (can be maximized, minimized, fullscreen)
/// - **Popup**: a transient surface anchored to a parent (menus, tooltips)
///
/// When a client wants to show a window, it creates a `wl_surface`, then
/// assigns it the `xdg_toplevel` role via this protocol. The compositor can
/// then configure the window (suggest size, state) and the client commits
/// in response.
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

    /// A client created a new toplevel (window).
    fn new_toplevel(&mut self, _surface: ToplevelSurface) {
        // Will handle window management in a later phase.
    }

    /// A client created a new popup.
    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {
        // Will handle popups in a later phase.
    }

    /// A client requested an interactive grab on a popup (e.g., a dropdown menu
    /// that should dismiss when clicking outside it).
    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {
        // Will handle popup grabs in a later phase.
    }

    /// A client requested repositioning of a popup.
    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
        // Will handle popup repositioning in a later phase.
    }
}

smithay::delegate_xdg_shell!(Sola);
