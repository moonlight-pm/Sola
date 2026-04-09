/// Sola compositor — a Wayland compositor built on Smithay.
///
/// This crate contains the compositor core: backend initialization,
/// Wayland protocol handling, output management, and rendering.

pub mod backend;
pub mod output;
pub mod state;
mod wayland;

use std::collections::HashMap;

use drm_fourcc::DrmFourcc;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags};
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::output::DrmOutputRenderElements;
use smithay::backend::drm::{DrmDeviceFd, DrmEvent, DrmNode, NodeType};
use smithay::backend::renderer::ImportDma;
use smithay::backend::session::Session;
use smithay::backend::udev::{UdevBackend, UdevEvent};
use smithay::output::{Mode as WlMode, Output, OutputModeSource, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::utils::Transform;

use output::render::{self, Element, CLEAR_COLOR};

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
pub fn run() -> anyhow::Result<()> {
    tracing::info!("sola compositor starting");

    let mut event_loop: EventLoop<Sola> = EventLoop::try_new()?;

    // The Wayland display server — manages client connections and protocol
    // dispatch. Kept separate from `Sola` because `dispatch_clients` needs
    // `&mut Display` and `&mut Sola` simultaneously.
    let mut display: Display<Sola> = Display::new()?;
    let dh = display.handle();

    // -- Session --
    let (session, session_notifier) = backend::session::create()?;
    let seat_name = session.seat();

    // -- GPU discovery --
    let primary_gpu = backend::gpu::find_primary(&seat_name)?;
    let gpu_manager = backend::gpu::create_manager()?;

    let mut sola = Sola::new(dh, event_loop.handle(), session, gpu_manager, primary_gpu);

    // Register the session notifier so we get VT switch events.
    event_loop
        .handle()
        .insert_source(session_notifier, |_, _, _| {})
        .map_err(|e| anyhow::anyhow!("failed to insert session source: {e}"))?;

    // -- Device initialization --
    // Enumerate GPUs and initialize only those with connected displays.
    // We check sysfs first to avoid opening devices we don't need — opening
    // a DRM device acquires master and Smithay's drop logs errors if we
    // never used it.
    let udev_backend = UdevBackend::new(&seat_name)?;
    for (device_id, path) in udev_backend.device_list() {
        if let Ok(node) = DrmNode::from_dev_id(device_id) {
            if node.node_with_type(NodeType::Primary).is_some() {
                if !backend::gpu::has_connected_display(&path) {
                    tracing::info!(?node, ?path, "GPU has no connected displays, skipping");
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
        .map_err(|e| anyhow::anyhow!("failed to insert udev source: {e}"))?;

    tracing::info!("entering event loop");
    while sola.running {
        display.dispatch_clients(&mut sola)?;
        display.flush_clients()?;
        event_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut sola)?;
    }

    tracing::info!("sola compositor shutting down");

    // Drop DRM devices while we still hold the session and event loop.
    // Order matters: DrmDevice's drop impl tries to restore the previous
    // display state (mode, connectors), which requires DRM master. If the
    // libseat session drops first, we lose master and the restore fails
    // with "Permission denied".
    //
    // Note: on modern kernels with libseat, Smithay's DrmDeviceFd may
    // report `privileged: false` even though master WAS granted (because
    // libseat grants master via the fd, and Smithay's redundant
    // SET_MASTER ioctl fails). This means Smithay skips DROP_MASTER on
    // cleanup, but still tries to restore state — which may fail if the
    // kernel has already revoked master. This is a known Smithay quirk
    // on libseat systems and the error is cosmetic.
    sola.devices.clear();

    Ok(())
}

/// Initialize a single DRM GPU device end-to-end.
///
/// This is the full pipeline: open device → register GPU renderer →
/// scan connectors → create output manager → initialize outputs →
/// kick off render loop → store device.
fn init_device(sola: &mut Sola, node: DrmNode, path: &std::path::Path) -> anyhow::Result<()> {
    // Step 1: Open the DRM + GBM devices.
    // The caller already verified this GPU has connected displays via sysfs.
    let (drm, drm_notifier, gbm, render_node) =
        backend::device::open(&mut sola.session, path, node)?;

    // Step 2: Scan connectors to get the details (modes, crtcs).
    let mut scanner = smithay_drm_extras::drm_scanner::DrmScanner::new();
    let connected_outputs = output::scan::find_connected_outputs(&mut scanner, &drm);

    // Step 3: Register the GPU with the renderer manager.
    sola.gpu_manager
        .as_mut()
        .add_node(render_node, gbm.clone())?;

    // Step 4: Register DRM event source (VBlank, errors).
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
        .map_err(|e| anyhow::anyhow!("failed to insert DRM source: {e}"))?;

    // Step 5: Get renderer formats for the output manager.
    let renderer_formats = {
        let renderer = sola.gpu_manager.single_renderer(&render_node)?;
        renderer.dmabuf_formats().into_iter().collect::<Vec<_>>()
    };

    // Step 6: Create the DRM output manager (takes ownership of DRM device).
    // RENDERING | SCANOUT — buffers must be renderable (for GLES composition)
    // AND scanout-capable (for DRM page flip).
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
        // Xrgb8888 first — most GPUs prefer opaque scanout (no alpha channel).
        [DrmFourcc::Xrgb8888, DrmFourcc::Argb8888],
        renderer_formats,
    );

    // Step 7: Initialize each connected display and kick off the render loop.
    let mut outputs = HashMap::new();
    for (connector, crtc, mode, name) in connected_outputs {
        // Create Wayland output.
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

        // Prepare render elements for DrmOutputManager initialization.
        // These are passed so the output manager can force a composited frame
        // during setup to validate the pipeline.
        let mut render_elements = DrmOutputRenderElements::<
            smithay::backend::renderer::multigpu::MultiRenderer<
                '_,
                '_,
                smithay::backend::renderer::multigpu::gbm::GbmGlesBackend<
                    smithay::backend::renderer::gles::GlesRenderer,
                    DrmDeviceFd,
                >,
                smithay::backend::renderer::multigpu::gbm::GbmGlesBackend<
                    smithay::backend::renderer::gles::GlesRenderer,
                    DrmDeviceFd,
                >,
            >,
            Element,
        >::new();
        render_elements.add_output(&crtc, CLEAR_COLOR, std::iter::empty());

        // Initialize the DRM output — creates the DRM compositor internally,
        // does a synchronous mode-set commit to validate the pipeline.
        let mut renderer = sola.gpu_manager.single_renderer(&render_node)?;
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
            .map_err(|e| anyhow::anyhow!("failed to initialize DRM output: {e:?}"))?;

        tracing::info!(?crtc, "DRM output initialized, starting render loop");

        // Kick off the render loop with an explicit page-flip.
        // initialize_output does a synchronous commit_frame (mode-set) but we
        // need a page-flip (queue_frame) to start receiving VBlank events.
        // VBlank events drive the continuous render loop in on_vblank().
        let mut renderer = sola.gpu_manager.single_renderer(&render_node)?;
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
                    // initialize_output already consumed the damage. Force a
                    // page-flip by using commit_frame (synchronous).
                    tracing::info!(?crtc, "render_frame empty after init, display should be showing");
                }
            }
            Err(err) => {
                tracing::error!(?err, ?crtc, "initial render_frame failed");
            }
        }

        outputs.insert(crtc, drm_output);
    }

    // Step 8: Store the device.
    sola.devices.insert(
        node,
        backend::device::Device {
            output_manager,
            outputs,
            gbm,
            render_node,
            scanner,
            token,
        },
    );

    tracing::info!(?node, "GPU device fully initialized");
    Ok(())
}
