/// Frame rendering and DRM output management types.
///
/// Provides type aliases for the concrete DRM output types, the VBlank
/// handler, and the render function.
///
/// ## The render loop
///
/// ```text
/// render_output() → queue_frame() → [hardware scans out] → VBlank fires
///     ↑                                                          |
///     └──── frame_submitted() ← on_vblank() ←───────────────────┘
/// ```
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/drm/output/index.html
use smithay::backend::allocator::gbm::GbmAllocator;
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::output::{DrmOutput, DrmOutputManager};
use smithay::backend::drm::{DrmDeviceFd, DrmNode};
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::renderer::multigpu::{MultiRenderer, MultiTexture};
use smithay::backend::renderer::Color32F;
use smithay::reexports::drm::control::crtc;
use smithay::utils::IsAlive;

use crate::Sola;

/// Background color — dark blue-gray.
pub const CLEAR_COLOR: Color32F = Color32F::new(0.1, 0.1, 0.2, 1.0);

// -- Type aliases for concrete Smithay types --

type GlesBackend = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;

/// The multi-GPU renderer type returned by `GpuManager::single_renderer()`.
pub type SolaRenderer<'a> = MultiRenderer<'a, 'a, GlesBackend, GlesBackend>;

/// Simple texture element type — used for initialization and non-window elements.
pub type Element = TextureRenderElement<MultiTexture>;

/// DRM output manager — owns the DRM device and manages compositors.
pub type SolaOutputManager =
    DrmOutputManager<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// A single DRM output handle (one per connected display).
pub type SolaOutput =
    DrmOutput<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// Handle a VBlank event (page flip complete) for a CRTC.
///
/// This is the heartbeat of the render loop: release the old buffer,
/// send frame callbacks to clients, then render and submit the next frame.
pub fn on_vblank(sola: &mut Sola, node: DrmNode, crtc: crtc::Handle) {
    let Some(device) = sola.devices.get_mut(&node) else {
        return;
    };
    let Some(output) = device.outputs.get_mut(&crtc) else {
        return;
    };

    if let Err(err) = output.frame_submitted() {
        tracing::error!(?err, ?crtc, "frame_submitted failed");
        return;
    }

    // Send frame callbacks to clients so they know they can submit their
    // next frame. Without this, clients stall waiting for acknowledgement.
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    if let Some(output) = sola.space.outputs().next().cloned() {
        for window in sola.space.elements() {
            if window.alive() {
                window.send_frame(&output, time, Some(std::time::Duration::ZERO), |_, _| None);
            }
        }
    }

    render_output(sola, node, crtc);
}

/// Render a frame for a specific output and submit it for scanout.
///
/// Collects window surfaces from the Space, composites them with the
/// background color, and submits the result to the DRM output.
pub fn render_output(sola: &mut Sola, node: DrmNode, crtc: crtc::Handle) {
    let device = sola.devices.get_mut(&node).unwrap();
    let render_node = device.render_node;

    let mut renderer = match sola.gpu_manager.single_renderer(&render_node) {
        Ok(r) => r,
        Err(err) => {
            tracing::error!(?err, "failed to get renderer");
            return;
        }
    };

    // Get render elements from the Space. The elements have the renderer's
    // lifetime, so we collect them into a Vec before passing to render_frame.
    let output = sola.space.outputs().next().cloned();
    let elements = if let Some(ref output) = output {
        sola.space
            .render_elements_for_output(&mut renderer, output, 1.0)
            .unwrap_or_default()
    } else {
        vec![]
    };

    let drm_output = device.outputs.get_mut(&crtc).unwrap();

    match drm_output.render_frame(&mut renderer, &elements, CLEAR_COLOR, FrameFlags::empty()) {
        Ok(result) => {
            if !result.is_empty {
                if let Err(err) = drm_output.queue_frame(()) {
                    tracing::error!(?err, ?crtc, "queue_frame failed");
                }
            }
        }
        Err(err) => {
            tracing::error!(?err, ?crtc, "render_frame failed");
        }
    }
}
