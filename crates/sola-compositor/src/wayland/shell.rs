/// XDG shell protocol handler.
///
/// The XDG shell defines desktop window roles:
/// - **Toplevel**: a regular window (maximize, minimize, fullscreen)
/// - **Popup**: a transient surface anchored to a parent (menus, tooltips)
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/shell/xdg/trait.XdgShellHandler.html
use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::utils::{Serial, Size, SERIAL_COUNTER};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};

use crate::state::State;

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    /// A client created a new toplevel window.
    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        tracing::info!("new toplevel window from client");

        // Suggest the output size so the client can open at full screen dimensions.
        // This is a suggestion — clients are free to ignore it.
        if let Some(mode) = self.space.outputs().next().and_then(|o| o.current_mode()) {
            surface.with_pending_state(|state| {
                state.size = Some(Size::from((mode.size.w, mode.size.h)));
            });
        }

        // Send initial configure so the client knows it can start rendering.
        surface.send_configure();

        let wl_surface = surface.wl_surface().clone();
        let window = Window::new_wayland_window(surface);
        self.space.map_element(window, (0, 0), true);

        let serial = SERIAL_COUNTER.next_serial();
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, Some(wl_surface), serial);
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}
    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}
    fn reposition_request(&mut self, _surface: PopupSurface, _positioner: PositionerState, _token: u32) {}
}

smithay::delegate_xdg_shell!(State);
