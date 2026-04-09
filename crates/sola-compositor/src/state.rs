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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use smithay::backend::drm::DrmNode;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::desktop::{Space, Window};
use smithay::input::{Seat, SeatState};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::xwayland_shell::XWaylandShellState;
use smithay::xwayland::X11Wm;

use crate::backend::device::Device;
use crate::backend::gpu::SolaGpuManager;

pub struct Sola {
    /// Controls the main event loop. Set to `false` to trigger shutdown.
    pub running: bool,

    /// Set by the binary watcher thread when a new binary is detected.
    /// The main loop checks this after shutdown to decide whether to execv.
    pub restart_requested: Arc<AtomicBool>,

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

    /// The compositor's seat (keyboard + pointer). Stored directly
    /// since we always have exactly one seat.
    pub seat: Seat<Self>,

    /// Tracks `wl_data_device_manager` — clipboard and drag-and-drop.
    pub data_device_state: DataDeviceState,

    /// Tracks `wl_output` and `xdg_output` — display information.
    pub output_manager_state: OutputManagerState,

    /// Tracks `xdg_wm_base` — desktop window management (toplevel + popup).
    pub xdg_shell_state: XdgShellState,

    /// Tracks `xdg_decoration_manager` — controls client vs server decorations.
    #[allow(dead_code)]
    pub xdg_decoration_state: XdgDecorationState,

    // -- Desktop state --

    /// Tracks mapped windows and their positions on outputs.
    /// `Space` is Smithay's built-in window manager: it handles z-order,
    /// output assignment, and provides render elements for compositing.
    pub space: Space<Window>,

    /// Current pointer position in compositor-space coordinates.
    pub pointer_location: (f64, f64),

    // -- XWayland state --

    /// The X11 window manager instance. `None` until XWayland is ready.
    pub xwm: Option<X11Wm>,

    /// XWayland shell protocol state for surface pairing. `None` until init.
    pub xwayland_shell_state: Option<XWaylandShellState>,

    /// X11 window IDs that have requested mapping but may not have a
    /// wl_surface yet. Used to defer Space insertion until both
    /// `map_window_request` and `surface_associated` have fired.
    pub xwayland_mapped: HashSet<smithay::xwayland::xwm::X11Window>,

    /// The cursor image loaded from the xcursor theme. `None` if loading failed.
    pub cursor_buffer: Option<MemoryRenderBuffer>,

    /// The cursor hotspot — the pixel offset within the cursor image that
    /// represents the actual click point.
    pub cursor_hotspot: (i32, i32),
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
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat-0");
        seat.add_keyboard(Default::default(), 200, 25)
            .expect("failed to add keyboard to seat");
        seat.add_pointer();

        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);

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
            xdg_decoration_state,
            seat,
            space: Space::default(),
            pointer_location: (0.0, 0.0),
            cursor_buffer: None,
            cursor_hotspot: (0, 0),
            restart_requested: Arc::new(AtomicBool::new(false)),
            xwm: None,
            xwayland_shell_state: None,
            xwayland_mapped: HashSet::new(),
        }
    }
}
