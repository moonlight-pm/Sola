/// Central state for sola-x.
///
/// Holds both the server side (Wayland compositor for XWayland) and
/// the client side (Wayland client connecting to sola). The server
/// side is long-lived; the client side is rebuilt on each reconnection.
use std::collections::{HashMap, HashSet};

use smithay::input::{Seat, SeatState};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::xwayland_shell::XWaylandShellState;
use smithay::xwayland::X11Wm;

pub struct SolaX {
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

    /// Tracks X11 windows and their proxy surfaces in sola.
    pub windows: HashMap<u32, WindowBridge>,

    /// Whether the main loop should keep running.
    pub running: bool,
}

/// Per-X11-window state linking the server-side X11 surface to
/// the client-side proxy surface in sola.
pub struct WindowBridge {
    pub title: String,
    pub class: String,
    // Client-side proxy objects will be added in Phase 2.
}

impl SolaX {
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
            windows: HashMap::new(),
            running: true,
        }
    }
}
