/// Sola compositor — a Wayland compositor built on Smithay.
///
/// This crate contains the compositor core: backend initialization,
/// Wayland protocol handling, output management, and rendering.

pub mod backend;
pub mod cursor;
pub mod error;
pub mod output;
pub mod state;
mod wayland;

use std::collections::HashMap;

use drm_fourcc::DrmFourcc;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags};
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::output::DrmOutputRenderElements;
use smithay::backend::drm::{DrmEvent, DrmNode, NodeType};
use smithay::backend::renderer::ImportDma;
use smithay::backend::session::Session;
use smithay::backend::udev::{UdevBackend, UdevEvent};
use smithay::output::{Mode as WlMode, Output, OutputModeSource, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::utils::Transform;

use error::{CompositorError, DeviceError};
use output::render::{self, Element, SolaRenderer, CLEAR_COLOR};

pub use state::Sola;

/// Start the compositor.
///
/// This is the main entry point called by the `sola` binary. Initialization
/// proceeds in order:
///
/// 1. Create the calloop event loop and Wayland display
/// 2. Open a libseat session (for hardware access)
/// 3. Discover GPUs via udev
/// 4. Initialize each GPU (DRM + GBM devices, output manager, first frame)
/// 5. Enter the dispatch loop
///
/// See: https://docs.rs/calloop/0.14
pub fn run() -> Result<(), CompositorError> {
    tracing::info!("sola compositor starting");

    let mut event_loop: EventLoop<Sola> =
        EventLoop::try_new().map_err(|e| CompositorError::EventLoop(e.to_string()))?;

    let mut display: Display<Sola> =
        Display::new().map_err(|e| CompositorError::Display(e.to_string()))?;
    let dh = display.handle();

    // -- Session --
    let (session, session_notifier) = backend::session::create()?;
    let seat_name = session.seat();

    // -- GPU discovery --
    let primary_gpu = backend::gpu::find_primary(&seat_name)?;
    let gpu_manager = backend::gpu::create_manager()?;

    let mut sola = Sola::new(dh, event_loop.handle(), session, gpu_manager, primary_gpu);

    // Load the cursor image from the system xcursor theme.
    if let Some((buffer, hotspot)) = cursor::load_default() {
        sola.cursor_buffer = Some(buffer);
        sola.cursor_hotspot = hotspot;
    } else {
        tracing::warn!("failed to load cursor from xcursor theme");
    }

    event_loop
        .handle()
        .insert_source(session_notifier, |_, _, _| {})
        .map_err(|e| CompositorError::EventLoop(format!("session source: {e}")))?;

    // -- Device initialization --
    // Register ALL GPUs with the GpuManager (for cross-GPU buffer import),
    // but only create DRM outputs on GPUs that have connected displays.
    let udev_backend = UdevBackend::new(&seat_name)?;
    for (device_id, path) in udev_backend.device_list() {
        if let Ok(node) = DrmNode::from_dev_id(device_id) {
            if node.node_with_type(NodeType::Primary).is_some() {
                if !backend::gpu::has_connected_display(&path) {
                    // No display, but still register with GpuManager so we
                    // can import buffers from clients that render on this GPU
                    // (e.g., Steam choosing the "wrong" GPU in a multi-GPU system).
                    if let Err(err) = register_gpu(&mut sola, &path, node) {
                        tracing::warn!(?err, ?node, "failed to register non-display GPU");
                    } else {
                        tracing::info!(?node, ?path, "registered non-display GPU for buffer import");
                    }
                    continue;
                }
                if let Err(err) = init_device(&mut sola, node, &path) {
                    tracing::error!(?err, ?node, "failed to initialize GPU");
                }
            }
        }
    }

    // -- Input --
    backend::input::setup(&event_loop.handle(), &sola.session)?;

    // -- Binary watcher --
    // Restart the compositor when the binary is replaced on disk (deploy).
    // Runs in a separate thread, independent of the event loop.
    backend::watcher::watch_binary();

    // -- Wayland socket --
    // Create the socket that clients connect to. Set WAYLAND_DISPLAY so
    // child processes (and clients launched from the same session) can
    // find the compositor.
    let socket_name = backend::socket::listen(&event_loop.handle())?;
    // Safety: we set this before spawning any threads or child processes,
    // and no other thread is reading environment variables concurrently.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &socket_name) };

    // -- XWayland --
    // Spawn XWayland so X11 apps (Steam, etc.) can run.
    // XWayland connects as a Wayland client and provides an X11 display.
    {
        use smithay::wayland::xwayland_shell::XWaylandShellState;
        use smithay::xwayland::XWayland;

        sola.xwayland_shell_state = Some(XWaylandShellState::new::<Sola>(&sola.display_handle));

        let (xwayland, xwayland_client) = XWayland::spawn(
            &sola.display_handle,
            Some(0),         // Pin to :0 for stable $DISPLAY
            std::iter::empty::<(String, String)>(),
            true,            // abstract socket
            std::process::Stdio::null(),
            std::process::Stdio::null(),
            |_| {},
        )
        .map_err(|e| CompositorError::EventLoop(format!("XWayland spawn: {e}")))?;

        let handle = event_loop.handle();
        handle
            .insert_source(xwayland, move |event, _, sola| match event {
                smithay::xwayland::XWaylandEvent::Ready {
                    x11_socket,
                    display_number,
                } => {
                    tracing::info!(display_number, "XWayland ready");
                    unsafe { std::env::set_var("DISPLAY", format!(":{display_number}")) };

                    match smithay::xwayland::X11Wm::start_wm(
                        sola.loop_handle.clone(),
                        x11_socket,
                        xwayland_client.clone(),
                    ) {
                        Ok(wm) => {
                            sola.xwm = Some(wm);
                            tracing::info!("X11 window manager started");
                        }
                        Err(err) => {
                            tracing::error!(?err, "failed to start X11 window manager");
                        }
                    }
                }
                smithay::xwayland::XWaylandEvent::Error => {
                    tracing::error!("XWayland failed to start");
                }
            })
            .map_err(|e| CompositorError::EventLoop(format!("XWayland source: {e}")))?;
    }

    // Listen for GPU hotplug events.
    event_loop
        .handle()
        .insert_source(udev_backend, |event, _, sola| {
            if let UdevEvent::Added { device_id, path } = event {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    let _ = init_device(sola, node, &path);
                }
            }
        })
        .map_err(|e| CompositorError::EventLoop(format!("udev source: {e}")))?;

    tracing::info!("entering event loop");
    while sola.running {
        // Update Space bookkeeping — sends output enter/leave events to
        // clients and cleans up dead windows.
        sola.space.refresh();

        display
            .dispatch_clients(&mut sola)
            .map_err(|e| CompositorError::Display(e.to_string()))?;
        display
            .flush_clients()
            .map_err(|e| CompositorError::Display(e.to_string()))?;

        // Render all outputs. If new damage exists (window mapped, surface
        // committed), this will composite and page-flip. If no damage,
        // render_frame returns is_empty and we skip the flip — no waste.
        render::render_all(&mut sola);

        event_loop
            .dispatch(Some(std::time::Duration::from_millis(16)), &mut sola)
            .map_err(|e| CompositorError::EventLoop(e.to_string()))?;
    }

    tracing::info!("sola compositor shutting down");

    // Drop DRM devices while we still hold the session and event loop.
    // See comment in error.rs about the Smithay/libseat DRM master quirk.
    sola.devices.clear();

    Ok(())
}

/// Register a GPU with the GpuManager without creating DRM outputs.
///
/// Used for GPUs that have no connected displays but may be used by
/// clients for rendering (e.g., Steam in a multi-GPU system). The
/// GpuManager needs to know about these GPUs to import their buffers.
fn register_gpu(sola: &mut Sola, path: &std::path::Path, node: DrmNode) -> Result<(), DeviceError> {
    let (_drm, _notifier, gbm, render_node) =
        backend::device::open(&mut sola.session, path, node)?;

    sola.gpu_manager
        .as_mut()
        .add_node(render_node, gbm)
        .map_err(|e| DeviceError::Open {
            path: path.to_owned(),
            reason: format!("GPU registration: {e:?}"),
        })?;

    Ok(())
}

/// Initialize a single DRM GPU device end-to-end.
fn init_device(sola: &mut Sola, node: DrmNode, path: &std::path::Path) -> Result<(), DeviceError> {
    let (drm, drm_notifier, gbm, render_node) =
        backend::device::open(&mut sola.session, path, node)?;

    let mut scanner = smithay_drm_extras::drm_scanner::DrmScanner::new();
    let connected_outputs = output::scan::find_connected_outputs(&mut scanner, &drm);

    sola.gpu_manager
        .as_mut()
        .add_node(render_node, gbm.clone())
        .map_err(|e| DeviceError::Open {
            path: path.to_owned(),
            reason: format!("GPU registration: {e:?}"),
        })?;

    let token = sola
        .loop_handle
        .insert_source(drm_notifier, move |event, _metadata, sola| match event {
            DrmEvent::VBlank(crtc) => {
                render::on_vblank(sola, node, crtc);
            }
            DrmEvent::Error(err) => {
                tracing::error!(?err, "DRM device error");
            }
        })
        .map_err(|e| DeviceError::EventSource {
            node,
            reason: e.to_string(),
        })?;

    let renderer_formats = {
        let renderer = sola.gpu_manager.single_renderer(&render_node).map_err(|e| {
            DeviceError::OutputInit {
                node,
                reason: format!("renderer: {e:?}"),
            }
        })?;
        renderer.dmabuf_formats().into_iter().collect::<Vec<_>>()
    };

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), None);
    let mut output_manager = render::SolaOutputManager::new(
        drm,
        allocator,
        exporter,
        Some(gbm.clone()),
        [DrmFourcc::Xrgb8888, DrmFourcc::Argb8888],
        renderer_formats,
    );

    let mut outputs = HashMap::new();
    for (connector, crtc, mode, name) in connected_outputs {
        let wl_output = Output::new(
            name,
            PhysicalProperties {
                size: (
                    connector.size().unwrap_or((0, 0)).0 as i32,
                    connector.size().unwrap_or((0, 0)).1 as i32,
                )
                    .into(),
                subpixel: Subpixel::Unknown,
                make: "Unknown".into(),
                model: "Unknown".into(),
            },
        );
        let wl_mode = WlMode {
            size: (mode.size().0 as i32, mode.size().1 as i32).into(),
            refresh: (mode.vrefresh() * 1000) as i32,
        };
        wl_output.change_current_state(Some(wl_mode), Some(Transform::Normal), None, None);
        wl_output.set_preferred(wl_mode);
        wl_output.create_global::<Sola>(&sola.display_handle);

        // Register the output with the Space so it knows where to place windows
        // and which render elements belong to which display.
        sola.space.map_output(&wl_output, (0, 0));

        let mut render_elements =
            DrmOutputRenderElements::<SolaRenderer, Element>::new();
        render_elements.add_output(&crtc, CLEAR_COLOR, std::iter::empty());

        let mut renderer =
            sola.gpu_manager
                .single_renderer(&render_node)
                .map_err(|e| DeviceError::OutputInit {
                    node,
                    reason: format!("renderer: {e:?}"),
                })?;

        let mut drm_output = output_manager
            .initialize_output(
                crtc,
                mode,
                &[connector.handle()],
                OutputModeSource::Auto(wl_output),
                None,
                &mut renderer,
                &render_elements,
            )
            .map_err(|e| DeviceError::OutputInit {
                node,
                reason: format!("{e:?}"),
            })?;

        tracing::info!(?crtc, "DRM output initialized, starting render loop");

        // Kick off the render loop with an explicit page-flip.
        let mut renderer =
            sola.gpu_manager
                .single_renderer(&render_node)
                .map_err(|e| DeviceError::OutputInit {
                    node,
                    reason: format!("renderer: {e:?}"),
                })?;
        let elements: Vec<Element> = vec![];
        match drm_output.render_frame::<_, Element>(
            &mut renderer, &elements, CLEAR_COLOR, FrameFlags::empty(),
        ) {
            Ok(result) => {
                if !result.is_empty {
                    if let Err(err) = drm_output.queue_frame(()) {
                        tracing::error!(?err, ?crtc, "initial queue_frame failed");
                    } else {
                        tracing::info!(?crtc, "first page-flip queued");
                    }
                } else {
                    tracing::info!(?crtc, "render_frame empty after init, display should be showing");
                }
            }
            Err(err) => {
                tracing::error!(?err, ?crtc, "initial render_frame failed");
            }
        }

        outputs.insert(crtc, drm_output);
    }

    sola.devices.insert(
        node,
        backend::device::Device {
            output_manager,
            outputs,
            gbm,
            render_node,
            frame_pending: false,
            scanner,
            token,
        },
    );

    tracing::info!(?node, "GPU device fully initialized");
    Ok(())
}
