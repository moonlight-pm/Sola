/// Frame rendering and DRM output management.
///
/// ## Render loop design
///
/// Two paths trigger rendering:
///
/// 1. **VBlank-driven** (`on_vblank`): After a page flip completes, render
///    and queue the next frame. Steady-state path at display refresh rate.
///
/// 2. **Poll-driven** (`render_all`): Called from the main event loop every
///    tick. Only renders if no page flip is pending. Catches new windows
///    and late-arriving content.
///
/// Frame callbacks are sent every tick from the main loop (not just on
/// VBlank) to avoid a deadlock: XWayland clients wait for a frame callback
/// before committing their first buffer, but VBlanks only fire after a
/// successful queue_frame, which requires damage from a committed buffer.
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::DrmNode;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::Color32F;
use smithay::desktop::space::SpaceRenderElements;
use smithay::reexports::drm::control::crtc;
use smithay::utils::IsAlive;

use crate::Sola;
use crate::types::SolaRenderer;

/// Background color — dark blue-gray.
pub const CLEAR_COLOR: Color32F = Color32F::new(0.1, 0.1, 0.2, 1.0);

// Combined render element enum.
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

    device.frame_pending = false;

    do_render(sola, node, crtc);
}

/// Send frame callbacks to all windows and render all outputs.
pub fn render_all(sola: &mut Sola) {
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

    let targets: Vec<(DrmNode, crtc::Handle)> = sola
        .devices
        .iter()
        .filter(|(_, device)| !device.frame_pending)
        .flat_map(|(node, device)| {
            device.outputs.keys().map(move |crtc| (*node, *crtc))
        })
        .collect();

    for (node, crtc) in targets {
        do_render(sola, node, crtc);
    }
}

/// Render a frame and submit for scanout.
fn do_render(sola: &mut Sola, node: DrmNode, crtc: crtc::Handle) {
    let device = sola.devices.get_mut(&node).unwrap();
    let render_node = device.render_node;

    let mut renderer = match sola.gpu_manager.single_renderer(&render_node) {
        Ok(r) => r,
        Err(err) => {
            tracing::error!(?err, "failed to get renderer");
            return;
        }
    };

    let output = sola.space.outputs().next().cloned();
    let space_elements = if let Some(ref output) = output {
        sola.space
            .render_elements_for_output(&mut renderer, output, 1.0)
            .unwrap_or_default()
    } else {
        vec![]
    };

    let mut elements: Vec<OutputElement> = space_elements
        .into_iter()
        .map(OutputElement::Space)
        .collect();

    if let Some(ref cursor_buffer) = sola.cursor_buffer {
        let (hx, hy) = sola.cursor_hotspot;
        let (px, py) = sola.pointer_location;
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
                elements.insert(0, OutputElement::Cursor(cursor_element));
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
                match drm_output.queue_frame(()) {
                    Ok(()) => {
                        sola.devices.get_mut(&node).unwrap().frame_pending = true;
                    }
                    Err(err) => {
                        tracing::error!(?err, ?crtc, "queue_frame failed");
                    }
                }
            }
        }
        Err(err) => {
            tracing::error!(?err, ?crtc, "render_frame failed");
        }
    }
}
