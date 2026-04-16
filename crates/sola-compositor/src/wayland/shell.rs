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

use crate::state::State;

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    /// A client created a new toplevel window.
    ///
    /// Surfaces are held in a pending buffer until their app_id is known
    /// and can be matched against window policies. This prevents incorrect
    /// sizing and unwanted focus stealing.
    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        tracing::info!("new toplevel window from client");

        // Send configure with no size suggestion — the client uses its
        // preferred size. Real size comes from policy or SetWindowGeometry.
        surface.send_configure();

        let window = Window::new_wayland_window(surface);
        self.pending_surfaces.push(window);
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}
    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}
    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
    }
}

smithay::delegate_xdg_shell!(State);
