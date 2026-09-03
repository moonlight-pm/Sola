//! Rounded-rect clip for floating CSD.
//!
//! iced 0.14 `container.clip(true)` scissors to the **axis-aligned** layout
//! rectangle. Full-bleed children (lists, fills, terminal grid, CEF) therefore
//! paint the AABB "ears" outside a rounded face. This widget draws the child
//! normally, then dest-out punches those ears in a **later layer** so quads,
//! text, images, and shader primitives (CEF) are all cleared.
//!
//! Resize grips stay on a still-later layer so the nearly-invisible AABB
//! corner pads keep owning pointer hits.

use std::sync::atomic::{AtomicUsize, Ordering};

use iced::advanced::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, renderer};
use iced::widget::shader;
use iced::{
    Background, Border, Color, Element, Event, Length, Point, Rectangle, Size, Theme, Vector,
};

use crate::components::style::{HAIRLINE_A, RADIUS_XL, mix_white};

/// Matches [`super::titlebar`] frame pad — kept local so this module
/// does not cycle with the titlebar crate path.
const FRAME_BORDER: f32 = 1.0;

/// Wrap `content` so it cannot paint outside a rounded rect of `radius`
/// (logical px; typically the floating face radius).
pub fn wrap<'a, Message>(
    content: impl Into<Element<'a, Message, Theme>>,
    radius: f32,
) -> Element<'a, Message, Theme>
where
    Message: 'a,
{
    RoundedClip {
        content: content.into(),
        radius,
    }
    .into()
}

struct RoundedClip<'a, Message> {
    content: Element<'a, Message, Theme>,
    radius: f32,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for RoundedClip<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let child =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits);
        let size = limits.resolve(Length::Fill, Length::Fill, child.size());
        layout::Node::with_children(size, vec![child])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let child = layout.children().next().expect("float-clip content");
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], child, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let child = layout.children().next().expect("float-clip content");
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let child = layout.children().next().expect("float-clip content");
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let child = layout.children().next().expect("float-clip content");
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            viewport,
            translation,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };
        let child = layout.children().next().expect("float-clip content");
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            child,
            cursor,
            &visible,
        );

        // Later layer than the child so the punch lands after this layer's
        // quads, meshes, shaders (CEF), images, and text.
        use iced_wgpu::primitive::Renderer as _;
        renderer.with_layer(bounds, |renderer| {
            renderer.draw_primitive(
                bounds,
                FacePunch {
                    radius: self.radius,
                },
            );
        });

        // Inner-radius punch can clear the 1px outer hairline in the corner
        // ring (same circle centre, r vs r+1). Redraw the frame stroke so
        // the chrome edge stays continuous.
        let frame = Rectangle {
            x: bounds.x - FRAME_BORDER,
            y: bounds.y - FRAME_BORDER,
            width: bounds.width + 2.0 * FRAME_BORDER,
            height: bounds.height + 2.0 * FRAME_BORDER,
        };
        let p = theme.extended_palette();
        let fill = p.background.base.color;
        let fill = if fill.a < 0.01 {
            p.background.weaker.color
        } else {
            fill
        };
        renderer.with_layer(frame, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: frame,
                    border: Border {
                        color: mix_white(fill, HAIRLINE_A),
                        width: FRAME_BORDER,
                        radius: RADIUS_XL.into(),
                    },
                    ..renderer::Quad::default()
                },
                Background::Color(Color::TRANSPARENT),
            );
        });
    }
}

impl<'a, Message: 'a> From<RoundedClip<'a, Message>> for Element<'a, Message, Theme> {
    fn from(value: RoundedClip<'a, Message>) -> Self {
        Element::new(value)
    }
}

/// Signed distance to a rounded rectangle (iced quad convention).
///
/// Negative is inside. The AA band is `clamp(0.5 - dist, 0, 1)`, so a pixel
/// with `dist >= 0.5` is fully outside the fill. Used by unit tests (and the
/// titlebar resize-zone test) — not on the draw path.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn rounded_rect_dist(p: Point, bounds: Rectangle, radius: f32) -> f32 {
    let radius = radius.min(bounds.width.min(bounds.height) * 0.5);
    let local_x = p.x - bounds.x;
    let local_y = p.y - bounds.y;
    let px = -(local_x - bounds.width * 0.5) * 2.0;
    let py = -(local_y - bounds.height * 0.5) * 2.0;
    rounded_box_sdf(px, py, bounds.width, bounds.height, radius * 2.0) / 2.0
}

#[cfg_attr(not(test), allow(dead_code))]
fn rounded_box_sdf(px: f32, py: f32, size_x: f32, size_y: f32, corner: f32) -> f32 {
    let qx = px.abs() - size_x + corner;
    let qy = py.abs() - size_y + corner;
    qx.max(qy).min(0.0) + length(qx.max(0.0), qy.max(0.0)) - corner
}

#[cfg_attr(not(test), allow(dead_code))]
fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

#[derive(Debug)]
struct FacePunch {
    radius: f32,
}

struct FaceClipPipeline {
    pipeline: iced::wgpu::RenderPipeline,
    layout: iced::wgpu::BindGroupLayout,
    slots: Vec<ClipSlot>,
    prepared: usize,
    drawn: AtomicUsize,
}

struct ClipSlot {
    buffer: iced::wgpu::Buffer,
    bind_group: iced::wgpu::BindGroup,
}

impl std::fmt::Debug for FaceClipPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceClipPipeline")
            .field("slots", &self.slots.len())
            .finish_non_exhaustive()
    }
}

const UNIFORM_SIZE: u64 = 32;

impl shader::Pipeline for FaceClipPipeline {
    fn new(
        device: &iced::wgpu::Device,
        _queue: &iced::wgpu::Queue,
        format: iced::wgpu::TextureFormat,
    ) -> Self {
        let layout = device.create_bind_group_layout(&iced::wgpu::BindGroupLayoutDescriptor {
            label: Some("sola-kit face clip bgl"),
            entries: &[iced::wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: iced::wgpu::ShaderStages::FRAGMENT,
                ty: iced::wgpu::BindingType::Buffer {
                    ty: iced::wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: iced::wgpu::BufferSize::new(UNIFORM_SIZE),
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(iced::wgpu::ShaderModuleDescriptor {
            label: Some("sola-kit face clip shader"),
            source: iced::wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout =
            device.create_pipeline_layout(&iced::wgpu::PipelineLayoutDescriptor {
                label: Some("sola-kit face clip pl"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let pipeline = device.create_render_pipeline(&iced::wgpu::RenderPipelineDescriptor {
            label: Some("sola-kit face clip rp"),
            layout: Some(&pipeline_layout),
            vertex: iced::wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(iced::wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(iced::wgpu::ColorTargetState {
                    format,
                    // dest * (1 - src.a): src.a is "how much to punch".
                    blend: Some(iced::wgpu::BlendState {
                        color: iced::wgpu::BlendComponent {
                            src_factor: iced::wgpu::BlendFactor::Zero,
                            dst_factor: iced::wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: iced::wgpu::BlendOperation::Add,
                        },
                        alpha: iced::wgpu::BlendComponent {
                            src_factor: iced::wgpu::BlendFactor::Zero,
                            dst_factor: iced::wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: iced::wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: iced::wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: iced::wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: iced::wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            layout,
            slots: Vec::new(),
            prepared: 0,
            drawn: AtomicUsize::new(0),
        }
    }

    fn trim(&mut self) {
        self.prepared = 0;
        self.drawn.store(0, Ordering::Relaxed);
    }
}

impl shader::Primitive for FacePunch {
    type Pipeline = FaceClipPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &iced::wgpu::Device,
        queue: &iced::wgpu::Queue,
        bounds: &Rectangle,
        viewport: &shader::Viewport,
    ) {
        if pipeline.prepared == pipeline.slots.len() {
            let buffer = device.create_buffer(&iced::wgpu::BufferDescriptor {
                label: Some("sola-kit face clip uniforms"),
                size: UNIFORM_SIZE,
                usage: iced::wgpu::BufferUsages::UNIFORM | iced::wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&iced::wgpu::BindGroupDescriptor {
                label: Some("sola-kit face clip bg"),
                layout: &pipeline.layout,
                entries: &[iced::wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            pipeline.slots.push(ClipSlot { buffer, bind_group });
        }
        let slot = &pipeline.slots[pipeline.prepared];
        let scale = viewport.scale_factor() as f32;
        let origin = [bounds.x * scale, bounds.y * scale];
        let size = [bounds.width * scale, bounds.height * scale];
        let radius = self.radius * scale;
        write_uniforms(queue, &slot.buffer, origin, size, radius);
        pipeline.prepared += 1;
    }

    fn draw(&self, pipeline: &Self::Pipeline, pass: &mut iced::wgpu::RenderPass<'_>) -> bool {
        let i = pipeline.drawn.fetch_add(1, Ordering::Relaxed);
        let Some(slot) = pipeline.slots.get(i) else {
            return true;
        };
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &slot.bind_group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

fn write_uniforms(
    queue: &iced::wgpu::Queue,
    buffer: &iced::wgpu::Buffer,
    origin: [f32; 2],
    size: [f32; 2],
    radius: f32,
) {
    let mut bytes = [0u8; UNIFORM_SIZE as usize];
    bytes[0..4].copy_from_slice(&origin[0].to_le_bytes());
    bytes[4..8].copy_from_slice(&origin[1].to_le_bytes());
    bytes[8..12].copy_from_slice(&size[0].to_le_bytes());
    bytes[12..16].copy_from_slice(&size[1].to_le_bytes());
    bytes[16..20].copy_from_slice(&radius.to_le_bytes());
    queue.write_buffer(buffer, 0, &bytes);
}

const SHADER: &str = r#"
struct Uniforms {
    origin_size: vec4<f32>,
    radius_pad: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(pos[i], 0.0, 1.0);
}

fn rounded_box_sdf(p: vec2<f32>, size: vec2<f32>, corner: f32) -> f32 {
    let q = abs(p) - size + vec2<f32>(corner);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - corner;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let origin = u.origin_size.xy;
    let size = u.origin_size.zw;
    let radius = min(u.radius_pad.x, min(size.x, size.y) * 0.5);
    let local = pos.xy - origin;
    let dist = rounded_box_sdf(
        -(local - size * 0.5) * 2.0,
        size,
        radius * 2.0,
    ) / 2.0;
    let inside = clamp(0.5 - dist, 0.0, 1.0);
    return vec4<f32>(0.0, 0.0, 0.0, 1.0 - inside);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn box100() -> Rectangle {
        Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }
    }

    #[test]
    fn centre_is_inside_aabb_corner_is_outside() {
        let b = box100();
        let r = 13.0;
        assert!(
            rounded_rect_dist(Point::new(50.0, 50.0), b, r) < 0.0,
            "centre must be inside the rounded face"
        );
        let corner = rounded_rect_dist(Point::new(99.5, 99.5), b, r);
        assert!(
            corner >= 0.5,
            "AABB corner is the ear iced clip would leak; dist={corner}"
        );
    }

    #[test]
    fn mid_edge_stays_inside() {
        let b = box100();
        let r = 13.0;
        // Straight bottom, away from the radius — must remain filled.
        assert!(rounded_rect_dist(Point::new(50.0, 99.0), b, r) < 0.0);
        assert!(rounded_rect_dist(Point::new(1.0, 50.0), b, r) < 0.0);
    }

    #[test]
    fn diagonal_past_the_arc_is_outside() {
        let b = box100();
        let r = 13.0;
        // Circle centre of the BR corner is (87, 87); 13px along the
        // diagonal lands on the arc, further is the ear.
        let cx = 100.0 - r;
        let cy = 100.0 - r;
        let on_arc = Point::new(cx + r / 2.0_f32.sqrt(), cy + r / 2.0_f32.sqrt());
        let past = Point::new(
            cx + (r + 4.0) / 2.0_f32.sqrt(),
            cy + (r + 4.0) / 2.0_f32.sqrt(),
        );
        assert!(
            rounded_rect_dist(on_arc, b, r).abs() < 1.0,
            "point on the quarter-circle should sit in the AA band"
        );
        assert!(
            rounded_rect_dist(past, b, r) >= 0.5,
            "past the arc is the square ear"
        );
    }
}
