/// Core compositor state.
///
/// `Sola` is the central state struct — it owns all Wayland protocol state,
/// backend resources, and runtime bookkeeping. Smithay's event loop passes
/// `&mut Sola` to every callback, so all mutable state lives here.
///
/// Note: `Display<Sola>` is intentionally NOT stored here. Smithay's
/// `dispatch_clients` needs `&mut Display` and `&mut State` simultaneously,
/// which would violate Rust's borrowing rules if both lived in the same struct.
/// The `Display` is kept as a separate local in `run()`.
use std::collections::HashMap;

use smithay::backend::drm::DrmNode;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::input::SeatState;
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;

use crate::backend::device::Device;
use crate::backend::gpu::SolaGpuManager;

pub struct Sola {
    /// Controls the main event loop. Set to `false` to trigger shutdown.
    pub running: bool,

    /// Handle for creating Wayland globals and accessing the display.
    /// Unlike `Display`, a `DisplayHandle` can be freely cloned and used
    /// without conflicting with display dispatch.
    pub display_handle: DisplayHandle,

    /// Handle to the calloop event loop for registering event sources.
    pub loop_handle: LoopHandle<'static, Self>,

    // -- Hardware backend state --

    /// The libseat session for opening device files with proper privileges.
    pub session: LibSeatSession,

    /// Manages GPU renderers. Wraps one or more GPUs and provides scoped
    /// OpenGL ES renderers on demand.
    pub gpu_manager: SolaGpuManager,

    /// The primary GPU node (the one connected to displays).
    pub primary_gpu: DrmNode,

    /// Per-GPU device state, keyed by DRM node.
    pub devices: HashMap<DrmNode, Device>,

    // -- Wayland protocol state --

    /// Tracks `wl_compositor` — surface creation and management.
    pub compositor_state: CompositorState,

    /// Tracks `wl_shm` — shared memory buffer support.
    pub shm_state: ShmState,

    /// Tracks `wl_seat` — input devices (keyboard, pointer, touch).
    pub seat_state: SeatState<Self>,

    /// Tracks `wl_data_device_manager` — clipboard and drag-and-drop.
    pub data_device_state: DataDeviceState,

    /// Tracks `wl_output` and `xdg_output` — display information.
    pub output_manager_state: OutputManagerState,

    /// Tracks `xdg_wm_base` — desktop window management (toplevel + popup).
    pub xdg_shell_state: XdgShellState,
}

impl Sola {
    pub fn new(
        dh: DisplayHandle,
        loop_handle: LoopHandle<'static, Self>,
        session: LibSeatSession,
        gpu_manager: SolaGpuManager,
        primary_gpu: DrmNode,
    ) -> Self {
        let compositor_state = CompositorState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);

        Self {
            running: true,
            display_handle: dh,
            loop_handle,
            session,
            gpu_manager,
            primary_gpu,
            devices: HashMap::new(),
            compositor_state,
            shm_state,
            seat_state,
            data_device_state,
            output_manager_state,
            xdg_shell_state,
        }
    }
}
