/// Central state for sola-x.
///
/// Holds both the server side (Wayland compositor for XWayland) and
/// the client side (Wayland client connecting to sola). The server
/// side is long-lived; the client side is rebuilt on each reconnection.
use std::collections::{HashMap, HashSet};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

use smithay::input::{Seat, SeatState};
use smithay::output::{Mode as WlMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::utils::Transform;
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::dmabuf::DmabufState;
use smithay::wayland::output::OutputManagerState;
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
    #[allow(dead_code)]
    pub output_manager_state: OutputManagerState,
    pub xdg_shell_state: XdgShellState,
    pub dmabuf_state: Option<DmabufState>,
    pub xwm: Option<X11Wm>,
    pub xwayland_shell_state: Option<XWaylandShellState>,
    pub xwayland_mapped: HashSet<smithay::xwayland::xwm::X11Window>,

    // -- Bus --

    /// Connection to the Sola Bus for lifecycle coordination.
    pub bus: sola_bus::BusClient,

    // -- Bridge state --

    /// Maps server-side WlSurface (from XWayland) to X11 window ID.
    /// Populated when `surface_associated` fires.
    pub surface_to_x11: HashMap<WlSurface, u32>,

    /// X11 window metadata, keyed by window ID. Used to re-create proxy
    /// surfaces after compositor reconnection.
    pub x11_windows: HashMap<u32, X11WindowInfo>,

    // -- Client side --

    /// Wayland client connection to sola-compositor.
    /// None when disconnected; rebuilt on reconnection.
    pub client: Option<crate::client::ClientConnection>,

    /// Whether the main loop should keep running.
    pub running: bool,
}


/// Initialize dmabuf v4 by opening the primary GPU render node and querying
/// its supported formats. Returns None if no GPU is available.
fn init_dmabuf(dh: &DisplayHandle) -> Option<DmabufState> {
    use smithay::backend::allocator::gbm::GbmDevice;
    use smithay::backend::drm::{DrmNode, NodeType};
    use smithay::backend::egl::{EGLDisplay, EGLContext};
    use smithay::backend::renderer::gles::GlesRenderer;
    use smithay::backend::renderer::ImportDma;
    use smithay::backend::udev;
    use smithay::wayland::dmabuf::DmabufFeedbackBuilder;

    // Use the same GPU discovery as sola-compositor: find the primary
    // GPU for the default seat, then get its render node.
    let gpu_path = match udev::primary_gpu("seat0") {
        Ok(Some(path)) => path,
        _ => {
            tracing::warn!("no primary GPU found, dmabuf disabled");
            return None;
        }
    };
    let drm_node = match DrmNode::from_path(&gpu_path) {
        Ok(node) => node,
        Err(e) => {
            tracing::warn!(?e, "failed to resolve DRM node, dmabuf disabled");
            return None;
        }
    };
    let primary = drm_node
        .node_with_type(NodeType::Render)
        .and_then(|n| n.ok())
        .unwrap_or(drm_node);

    // Open GBM device.
    let gbm_fd = match std::fs::File::options().read(true).write(true).open(primary.dev_path()?) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(?e, "failed to open render node, dmabuf disabled");
            return None;
        }
    };
    let gbm = match GbmDevice::new(gbm_fd) {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(?e, "failed to create GBM device, dmabuf disabled");
            return None;
        }
    };

    // Create EGL display + context + renderer to query formats.
    let egl_display = match unsafe { EGLDisplay::new(gbm) } {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(?e, "failed to create EGL display, dmabuf disabled");
            return None;
        }
    };
    let egl_context = match EGLContext::new(&egl_display) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(?e, "failed to create EGL context, dmabuf disabled");
            return None;
        }
    };
    let renderer = match unsafe { GlesRenderer::new(egl_context) } {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(?e, "failed to create renderer, dmabuf disabled");
            return None;
        }
    };

    let formats: Vec<_> = renderer.dmabuf_formats().into_iter().collect();
    let format_count = formats.len();

    let feedback = match DmabufFeedbackBuilder::new(primary.dev_id(), formats).build() {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(?e, "failed to build dmabuf feedback");
            return None;
        }
    };

    let mut dmabuf_state = DmabufState::new();
    dmabuf_state.create_global_with_default_feedback::<State>(dh, &feedback);

    tracing::info!(format_count, ?primary, "dmabuf v4 initialized from GPU");
    Some(dmabuf_state)
}

/// Metadata about an X11 window, retained for proxy re-creation on reconnect.
pub struct X11WindowInfo {
    pub title: String,
    pub class: String,
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
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);

        // Virtual output so XWayland initializes its input handling.
        let output = Output::new(
            "sola-x-virtual".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "sola-x".into(),
                model: "virtual".into(),
            },
        );
        let mode = WlMode {
            size: (1920, 1080).into(),
            refresh: 60000,
        };
        output.change_current_state(Some(mode), Some(Transform::Normal), None, None);
        output.set_preferred(mode);
        output.create_global::<Self>(&dh);

        // Dmabuf v4 support — query the real GPU for supported formats so
        // XWayland/Mesa can use GPU buffers instead of falling back to SHM.
        let dmabuf_state = init_dmabuf(&dh);

        Self {
            display_handle: dh,
            loop_handle,
            compositor_state,
            shm_state,
            seat_state,
            seat,
            data_device_state,
            output_manager_state,
            xdg_shell_state,
            dmabuf_state,
            xwm: None,
            xwayland_shell_state: None,
            xwayland_mapped: HashSet::new(),
            bus: sola_bus::BusClient::new(),
            surface_to_x11: HashMap::new(),
            x11_windows: HashMap::new(),
            client: None,
            running: true,
        }
    }
}
