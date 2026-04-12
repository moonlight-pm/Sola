/// Core compositor state.
///
/// `State` is the central state struct — it owns all Wayland protocol state,
/// backend resources, and runtime bookkeeping. Smithay's event loop passes
/// `&mut State` to every callback, so all mutable state lives here.
///
/// Note: `Display<State>` is intentionally NOT stored here. Smithay's
/// `dispatch_clients` needs `&mut Display` and `&mut State` simultaneously,
/// which would violate Rust's borrowing rules if both lived in the same struct.
/// The `Display` is kept as a separate local in `run()`.
use std::collections::HashMap;

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
use smithay::wayland::dmabuf::DmabufState;
use smithay::wayland::shm::ShmState;

use crate::backend::device::Device;
use crate::backend::gpu::SolaGpuManager;

pub struct State {
    /// Controls the main event loop. Set to `false` to trigger shutdown.
    pub running: bool,

    /// Connection to the Sola Bus. `None` if the bus isn't available yet.
    pub bus: Option<sola_bus::BusClient>,

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

    /// The render node for the primary GPU. Used for GpuManager lookups
    /// and dmabuf import. Distinct from `primary_gpu` which is the
    /// primary/display node.
    pub primary_render_node: DrmNode,

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

    /// The app_id that currently has exclusive input grab, if any.
    /// While set, all input goes to this app's surface and other clients
    /// are excluded. The grabbed surface is shown above all others.
    pub input_grab: Option<String>,

    /// Most-recently-used app list, ordered by last focus time.
    /// The app that most recently had keyboard focus is at index 0.
    pub mru_apps: Vec<String>,

    /// Window positions received from sola-x before the window appeared.
    /// Applied in `new_toplevel` when the window is first mapped.
    pub pending_geometries: HashMap<String, (i32, i32)>,

    // -- Protocol state --

    /// Tracks `zwp_linux_dmabuf` — GPU buffer sharing with clients.
    pub dmabuf_state: Option<DmabufState>,

    /// The cursor image loaded from the xcursor theme. `None` if loading failed.
    pub cursor_buffer: Option<MemoryRenderBuffer>,

    /// The cursor hotspot — the pixel offset within the cursor image that
    /// represents the actual click point.
    pub cursor_hotspot: (i32, i32),

    /// Desktop wallpaper. Rendered behind all windows.
    pub wallpaper_buffer: Option<MemoryRenderBuffer>,
}

impl State {
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
            bus: None,
            display_handle: dh,
            loop_handle,
            session,
            gpu_manager,
            primary_gpu,
            primary_render_node: primary_gpu, // Updated in udev::init_device
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
            input_grab: None,
            mru_apps: Vec::new(),
            pending_geometries: HashMap::new(),
            cursor_buffer: None,
            cursor_hotspot: (0, 0),
            wallpaper_buffer: None,
            dmabuf_state: None,
        }
    }

    /// Get the app_id of a window, if set.
    pub fn app_id(window: &Window) -> Option<String> {
        window_app_id(window)
    }

    /// Find the first window with the given app_id.
    pub fn window_by_app_id(&self, target: &str) -> Option<Window> {
        self.space.elements().find(|window| {
            window_app_id(window).is_some_and(|id| id == target)
        }).cloned()
    }

    /// Find all windows with the given app_id.
    pub fn windows_by_app_id(&self, target: &str) -> Vec<Window> {
        self.space.elements().filter(|window| {
            window_app_id(window).is_some_and(|id| id == target)
        }).cloned().collect()
    }
}

/// Extract the app_id from a Window.
///
/// For Wayland windows: uses the xdg_toplevel app_id.
/// For X11 windows: uses WM_CLASS.
fn window_app_id(window: &Window) -> Option<String> {
    // Try Wayland xdg_toplevel app_id first.
    if let Some(toplevel) = window.toplevel() {
        use smithay::wayland::compositor::with_states;
        use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;

        let app_id = with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|data| data.lock().ok())
                .and_then(|attrs| attrs.app_id.clone())
        });
        if app_id.is_some() {
            return app_id;
        }
    }

    // Fall back to X11 WM_CLASS.
    if let Some(x11) = window.x11_surface() {
        let class = x11.class();
        if !class.is_empty() {
            return Some(class);
        }
    }

    None
}
