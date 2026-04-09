/// GPU device enumeration and initialization via udev.
///
/// Scans for DRM devices on the system, registers them with the GPU
/// manager, and creates DRM outputs for GPUs with connected displays.
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
use smithay::utils::Transform;

use crate::error::{CompositorError, DeviceError};
use crate::output::render::{self, CLEAR_COLOR};
use crate::state::Sola;
use crate::types::{Element, SolaOutputManager, SolaRenderer};

/// Enumerate GPUs and initialize devices.
///
/// Registers ALL GPUs with the GpuManager (for cross-GPU buffer import),
/// but only creates DRM outputs on GPUs with connected displays. Also
/// registers a udev hotplug listener for runtime device changes.
pub fn setup(
    sola: &mut Sola,
    event_loop: &EventLoop<'static, Sola>,
) -> Result<(), CompositorError> {
    let seat_name = sola.session.seat();
    let udev_backend = UdevBackend::new(&seat_name)?;

    for (device_id, path) in udev_backend.device_list() {
        if let Ok(node) = DrmNode::from_dev_id(device_id) {
            if node.node_with_type(NodeType::Primary).is_some() {
                if !super::gpu::has_connected_display(&path) {
                    if let Err(err) = register_gpu(sola, &path, node) {
                        tracing::warn!(?err, ?node, "failed to register non-display GPU");
                    } else {
                        tracing::info!(?node, ?path, "registered non-display GPU for buffer import");
                    }
                    continue;
                }
                if let Err(err) = init_device(sola, node, &path) {
                    tracing::error!(?err, ?node, "failed to initialize GPU");
                }
            }
        }
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

    Ok(())
}

/// Register a GPU with the GpuManager without creating DRM outputs.
fn register_gpu(
    sola: &mut Sola,
    path: &std::path::Path,
    node: DrmNode,
) -> Result<(), DeviceError> {
    let (_drm, _notifier, gbm, render_node) =
        super::device::open(&mut sola.session, path, node)?;

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
pub fn init_device(
    sola: &mut Sola,
    node: DrmNode,
    path: &std::path::Path,
) -> Result<(), DeviceError> {
    let (drm, drm_notifier, gbm, render_node) =
        super::device::open(&mut sola.session, path, node)?;

    let mut scanner = smithay_drm_extras::drm_scanner::DrmScanner::new();
    let connected_outputs = crate::output::scan::find_connected_outputs(&mut scanner, &drm);

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
        let renderer = sola
            .gpu_manager
            .single_renderer(&render_node)
            .map_err(|e| DeviceError::OutputInit {
                node,
                reason: format!("renderer: {e:?}"),
            })?;
        renderer.dmabuf_formats().into_iter().collect::<Vec<_>>()
    };

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), None);
    let mut output_manager = SolaOutputManager::new(
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

        sola.space.map_output(&wl_output, (0, 0));

        let mut render_elements = DrmOutputRenderElements::<SolaRenderer, Element>::new();
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

        let mut renderer =
            sola.gpu_manager
                .single_renderer(&render_node)
                .map_err(|e| DeviceError::OutputInit {
                    node,
                    reason: format!("renderer: {e:?}"),
                })?;
        let elements: Vec<Element> = vec![];
        match drm_output.render_frame::<_, Element>(
            &mut renderer,
            &elements,
            CLEAR_COLOR,
            FrameFlags::empty(),
        ) {
            Ok(result) => {
                if !result.is_empty {
                    if let Err(err) = drm_output.queue_frame(()) {
                        tracing::error!(?err, ?crtc, "initial queue_frame failed");
                    } else {
                        tracing::info!(?crtc, "first page-flip queued");
                    }
                } else {
                    tracing::info!(
                        ?crtc,
                        "render_frame empty after init, display should be showing"
                    );
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
        super::device::Device {
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
