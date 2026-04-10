/// Central state for sola-x.
///
/// Holds both the server side (Wayland compositor for XWayland) and
/// the client side (Wayland client connecting to sola). The server
/// side is long-lived; the client side is rebuilt on each reconnection.
use std::collections::{HashMap, HashSet};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

use smithay::input::{Seat, SeatState};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::xwayland_shell::XWaylandShellState;
use smithay::xwayland::X11Wm;

pub struct State {
    // -- Server side (Wayland compositor for XWayland) --

    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, Self>,
    pub compositor_state: CompositorState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<Self>,
    pub seat: Seat<Self>,
    pub data_device_state: DataDeviceState,
    pub xdg_shell_state: XdgShellState,
    pub xwm: Option<X11Wm>,
    pub xwayland_shell_state: Option<XWaylandShellState>,
    pub xwayland_mapped: HashSet<smithay::xwayland::xwm::X11Window>,

    // -- Bus --

    /// Connection to the Sola Bus for lifecycle coordination.
    pub bus: Option<sola_bus::BusClient>,

    // -- Bridge state --

    /// Maps server-side WlSurface (from XWayland) to X11 window ID.
    /// Populated when `surface_associated` fires.
    pub surface_to_x11: HashMap<WlSurface, u32>,

    // -- Client side --

    /// Wayland client connection to sola-compositor.
    /// None when disconnected; rebuilt on reconnection.
    pub client: Option<crate::client::ClientConnection>,

    /// Whether the main loop should keep running.
    pub running: bool,
}


impl State {
    pub fn new(
        dh: DisplayHandle,
        loop_handle: LoopHandle<'static, Self>,
    ) -> Self {
        let compositor_state = CompositorState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat-0");
        seat.add_keyboard(Default::default(), 200, 25)
            .expect("failed to add keyboard to seat");
        seat.add_pointer();

        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);

        Self {
            display_handle: dh,
            loop_handle,
            compositor_state,
            shm_state,
            seat_state,
            seat,
            data_device_state,
            xdg_shell_state,
            xwm: None,
            xwayland_shell_state: None,
            xwayland_mapped: HashSet::new(),
            bus: None,
            surface_to_x11: HashMap::new(),
            client: None,
            running: true,
        }
    }
}
