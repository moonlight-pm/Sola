/// Frame rendering and DRM output management types.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/backend/drm/output/index.html
use smithay::backend::allocator::gbm::GbmAllocator;
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::output::{DrmOutput, DrmOutputManager};
use smithay::backend::drm::{DrmDeviceFd, DrmNode};
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::renderer::multigpu::{MultiRenderer, MultiTexture};
use smithay::backend::renderer::Color32F;
use smithay::desktop::space::SpaceRenderElements;
use smithay::reexports::drm::control::crtc;
use smithay::utils::IsAlive;

use crate::Sola;

/// Background color — dark blue-gray.
pub const CLEAR_COLOR: Color32F = Color32F::new(0.1, 0.1, 0.2, 1.0);

// -- Type aliases --

type GlesBackend = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;

/// The multi-GPU renderer type.
pub type SolaRenderer<'a> = MultiRenderer<'a, 'a, GlesBackend, GlesBackend>;

/// Simple texture element type — used for initialization.
pub type Element = TextureRenderElement<MultiTexture>;

pub type SolaOutputManager =
    DrmOutputManager<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

pub type SolaOutput =
    DrmOutput<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

// Combined render element enum — holds both window surfaces and cursor.
// Pinned to our concrete SolaRenderer rather than being generic, because
// SpaceRenderElements requires ImportAll (ImportMemWl + ImportDmaWl) which
// is hard to express in the render_elements! macro's `where` clause.
smithay::backend::renderer::element::render_elements! {
    pub OutputElement<='a, SolaRenderer<'a>>;
    Space=SpaceRenderElements<SolaRenderer<'a>, WaylandSurfaceRenderElement<SolaRenderer<'a>>>,
    Cursor=MemoryRenderBufferRenderElement<SolaRenderer<'a>>,
}

/// Handle a VBlank event (page flip complete) for a CRTC.
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

    // Send frame callbacks to clients.
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

/// Render all outputs across all devices.
pub fn render_all(sola: &mut Sola) {
    let targets: Vec<(DrmNode, crtc::Handle)> = sola
        .devices
        .iter()
        .flat_map(|(node, device)| {
            device.outputs.keys().map(move |crtc| (*node, *crtc))
        })
        .collect();

    for (node, crtc) in targets {
        render_output(sola, node, crtc);
    }
}

/// Render a frame for a specific output and submit it for scanout.
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

    // Collect space elements (window surfaces).
    let output = sola.space.outputs().next().cloned();
    let mut elements: Vec<OutputElement> = if let Some(ref output) = output {
        sola.space
            .render_elements_for_output(&mut renderer, output, 1.0)
            .unwrap_or_default()
            .into_iter()
            .map(OutputElement::Space)
            .collect()
    } else {
        vec![]
    };

    // Add cursor element at the pointer position.
    if let Some(ref cursor_buffer) = sola.cursor_buffer {
        let (hx, hy) = sola.cursor_hotspot;
        let (px, py) = sola.pointer_location;
        // Position the cursor image so the hotspot aligns with the pointer.
        let cursor_pos = (px as i32 - hx, py as i32 - hy);

        match MemoryRenderBufferRenderElement::from_buffer(
            &mut renderer,
            (cursor_pos.0 as f64, cursor_pos.1 as f64),
            cursor_buffer,
            None,
            None,
            None,
            Kind::Cursor,
        ) {
            Ok(cursor_element) => {
                elements.push(OutputElement::Cursor(cursor_element));
            }
            Err(err) => {
                tracing::warn!(?err, "failed to create cursor render element");
            }
        }
    }

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
