/// Frame rendering and DRM output management types.
///
/// Provides type aliases for the concrete DRM output types, the VBlank
/// handler, and the render function.
///
/// ## The render loop
///
/// Wayland compositors use a double-buffered rendering model driven by
/// VBlank (vertical blank) events from the display hardware:
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

use crate::Sola;

/// Background color — dark blue-gray.
pub const CLEAR_COLOR: Color32F = Color32F::new(0.1, 0.1, 0.2, 1.0);

// -- Type aliases for the concrete Smithay types used throughout Sola --
//
// Smithay is heavily generic. These aliases pin the generic parameters to
// our specific backend choices (GBM allocator, GLES renderer, DRM fd)
// so the rest of the codebase doesn't need to spell them out.

/// The GBM+GLES backend type, parameterized for our DRM fd type.
type GlesBackend = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;

/// The multi-GPU renderer type. Even with a single GPU, Smithay's
/// GpuManager returns this wrapper type from `single_renderer()`.
pub type SolaRenderer<'a> = MultiRenderer<'a, 'a, GlesBackend, GlesBackend>;

/// DRM output manager — owns the DRM device and manages compositors.
pub type SolaOutputManager =
    DrmOutputManager<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// A single DRM output handle (one per connected display).
pub type SolaOutput =
    DrmOutput<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// The render element type used for compositing.
/// `MultiTexture` supports multi-GPU texture import.
pub type Element = TextureRenderElement<MultiTexture>;

/// Handle a VBlank event (page flip complete) for a CRTC.
///
/// Called from the DRM event source callback when the display hardware
/// signals that a page flip has completed. This is the heartbeat of the
/// render loop: release the old buffer, then render and submit the next frame.
pub fn on_vblank(sola: &mut Sola, node: DrmNode, crtc: crtc::Handle) {
    let Some(device) = sola.devices.get_mut(&node) else {
        return;
    };
    let Some(output) = device.outputs.get_mut(&crtc) else {
        return;
    };

    // Release the just-scanned-out buffer back to the swapchain.
    if let Err(err) = output.frame_submitted() {
        tracing::error!(?err, ?crtc, "frame_submitted failed");
        return;
    }

    // Render and submit the next frame.
    render_output(sola, node, crtc);
}

/// Render a frame for a specific output and submit it for scanout.
///
/// In Phase 1 this renders a solid color with no window elements.
/// Later phases will build a list of window surface elements here.
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

    let output = device.outputs.get_mut(&crtc).unwrap();
    let elements: Vec<Element> = vec![];

    match output.render_frame::<_, Element>(&mut renderer, &elements, CLEAR_COLOR, FrameFlags::empty()) {
        Ok(result) => {
            if !result.is_empty {
                if let Err(err) = output.queue_frame(()) {
                    tracing::error!(?err, ?crtc, "queue_frame failed");
                }
            }
        }
        Err(err) => {
            tracing::error!(?err, ?crtc, "render_frame failed");
        }
    }
}
