/// XDG shell protocol handler.
///
/// The XDG shell defines desktop window roles:
/// - **Toplevel**: a regular window (maximize, minimize, fullscreen)
/// - **Popup**: a transient surface anchored to a parent (menus, tooltips)
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/shell/xdg/trait.XdgShellHandler.html
use smithay::desktop::Window;
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

    /// A client created a new toplevel window.
    ///
    /// We wrap it in a Smithay `Window`, map it into the `Space` at (0, 0),
    /// and send an initial configure event so the client knows it can start
    /// rendering. In later phases this is where zone-based positioning and
    /// size negotiation will happen.
    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        tracing::info!("new toplevel window from client");

        // Send an initial configure event — the client won't render until
        // it receives this. An empty configure lets the client choose its
        // own size.
        surface.send_configure();

        let window = Window::new_wayland_window(surface);
        self.space.map_element(window, (0, 0), false);
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}
    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}
    fn reposition_request(&mut self, _surface: PopupSurface, _positioner: PositionerState, _token: u32) {}
}

smithay::delegate_xdg_shell!(Sola);
