//! Rounded corners for window frames.
//!
//! iced 0.14 `container.clip(true)` scissors to the **axis-aligned** layout
//! rectangle. Full-bleed children (lists, fills, terminal grid, CEF) therefore
//! paint the AABB "ears" outside a rounded face. [`draw_ears`] runs after the
//! frame's content, in a **later layer**, so quads, text, images, and shader
//! primitives (CEF) are all covered: a floating window's ears are cleared to
//! transparent (dest-out); a tiled window's are painted the ground colour so
//! the window stays opaque (opaque-region scanout, `PERFORMANCE.md`).

use std::sync::atomic::{AtomicUsize, Ordering};

use iced::advanced::Renderer as _;
use iced::widget::shader;
use iced::{Color, Rectangle};
#[cfg(test)]
use iced::Point;

/// What the corners outside the radius become.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Ears {
    /// Transparent: the desktop shows (a floating window's ARGB swapchain).
    Clear,
    /// Opaque in this colour: the ground behind a tile.
    Paint(Color),
}

/// Cover `bounds`' corners outside a rounded rect of `radius` (logical px).
/// Call after the content is drawn; opens its own layer.
pub(crate) fn draw_ears(renderer: &mut iced::Renderer, bounds: Rectangle, radius: f32, ears: Ears) {
    use iced_wgpu::primitive::Renderer as _;
    let fill = match ears {
        Ears::Clear => None,
        Ears::Paint(c) => Some([c.r, c.g, c.b, c.a]),
    };
    renderer.with_layer(bounds, |renderer| {
        renderer.draw_primitive(bounds, FacePunch { radius, fill });
    });
}

/// Signed distance to a rounded rectangle (iced quad convention).
///
/// Negative is inside. The AA band is `clamp(0.5 - dist, 0, 1)`, so a pixel
/// with `dist >= 0.5` is fully outside the fill. Used by unit tests (and the
/// titlebar resize-zone test) — not on the draw path.
#[cfg(test)]
pub(crate) fn rounded_rect_dist(p: Point, bounds: Rectangle, radius: f32) -> f32 {
    let radius = radius.min(bounds.width.min(bounds.height) * 0.5);
    let local_x = p.x - bounds.x;
    let local_y = p.y - bounds.y;
    let px = -(local_x - bounds.width * 0.5) * 2.0;
    let py = -(local_y - bounds.height * 0.5) * 2.0;
    rounded_box_sdf(px, py, bounds.width, bounds.height, radius * 2.0) / 2.0
}

#[cfg(test)]
fn rounded_box_sdf(px: f32, py: f32, size_x: f32, size_y: f32, corner: f32) -> f32 {
    let qx = px.abs() - size_x + corner;
    let qy = py.abs() - size_y + corner;
    qx.max(qy).min(0.0) + length(qx.max(0.0), qy.max(0.0)) - corner
}

#[cfg(test)]
fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

#[derive(Debug)]
struct FacePunch {
    radius: f32,
    /// `None` clears the ears; `Some(rgba)` paints them.
    fill: Option<[f32; 4]>,
}

struct FaceClipPipeline {
    /// dest-out: the ears become transparent.
    clear: iced::wgpu::RenderPipeline,
    /// premultiplied over: the ears take the fill colour.
    paint: iced::wgpu::RenderPipeline,
    layout: iced::wgpu::BindGroupLayout,
    slots: Vec<ClipSlot>,
    prepared: usize,
    drawn: AtomicUsize,
}

struct ClipSlot {
    buffer: iced::wgpu::Buffer,
    bind_group: iced::wgpu::BindGroup,
    paint: bool,
}

impl std::fmt::Debug for FaceClipPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceClipPipeline")
            .field("slots", &self.slots.len())
            .finish_non_exhaustive()
    }
}

const UNIFORM_SIZE: u64 = 48;

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
        // dest * (1 - src.a): src.a is "how much to punch".
        let clear_blend = iced::wgpu::BlendComponent {
            src_factor: iced::wgpu::BlendFactor::Zero,
            dst_factor: iced::wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: iced::wgpu::BlendOperation::Add,
        };
        // src + dest * (1 - src.a), src premultiplied by the shader.
        let paint_blend = iced::wgpu::BlendComponent {
            src_factor: iced::wgpu::BlendFactor::One,
            dst_factor: iced::wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: iced::wgpu::BlendOperation::Add,
        };
        let clear = build_pipeline(device, &pipeline_layout, &shader, format, clear_blend);
        let paint = build_pipeline(device, &pipeline_layout, &shader, format, paint_blend);
        Self {
            clear,
            paint,
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

fn build_pipeline(
    device: &iced::wgpu::Device,
    layout: &iced::wgpu::PipelineLayout,
    shader: &iced::wgpu::ShaderModule,
    format: iced::wgpu::TextureFormat,
    blend: iced::wgpu::BlendComponent,
) -> iced::wgpu::RenderPipeline {
    device.create_render_pipeline(&iced::wgpu::RenderPipelineDescriptor {
        label: Some("sola-kit face clip rp"),
        layout: Some(layout),
        vertex: iced::wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(iced::wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(iced::wgpu::ColorTargetState {
                format,
                blend: Some(iced::wgpu::BlendState {
                    color: blend,
                    alpha: blend,
                }),
                write_mask: iced::wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: iced::wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: iced::wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
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
            pipeline.slots.push(ClipSlot {
                buffer,
                bind_group,
                paint: false,
            });
        }
        let slot = &mut pipeline.slots[pipeline.prepared];
        slot.paint = self.fill.is_some();
        let scale = viewport.scale_factor() as f32;
        let origin = [bounds.x * scale, bounds.y * scale];
        let size = [bounds.width * scale, bounds.height * scale];
        let radius = self.radius * scale;
        // Clear mode reads only alpha: punch fully.
        let fill = self.fill.unwrap_or([0.0, 0.0, 0.0, 1.0]);
        write_uniforms(queue, &slot.buffer, origin, size, radius, fill);
        pipeline.prepared += 1;
    }

    fn draw(&self, pipeline: &Self::Pipeline, pass: &mut iced::wgpu::RenderPass<'_>) -> bool {
        let i = pipeline.drawn.fetch_add(1, Ordering::Relaxed);
        let Some(slot) = pipeline.slots.get(i) else {
            return true;
        };
        pass.set_pipeline(if slot.paint {
            &pipeline.paint
        } else {
            &pipeline.clear
        });
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
    fill: [f32; 4],
) {
    let mut bytes = [0u8; UNIFORM_SIZE as usize];
    bytes[0..4].copy_from_slice(&origin[0].to_le_bytes());
    bytes[4..8].copy_from_slice(&origin[1].to_le_bytes());
    bytes[8..12].copy_from_slice(&size[0].to_le_bytes());
    bytes[12..16].copy_from_slice(&size[1].to_le_bytes());
    bytes[16..20].copy_from_slice(&radius.to_le_bytes());
    for (i, c) in fill.iter().enumerate() {
        bytes[32 + 4 * i..36 + 4 * i].copy_from_slice(&c.to_le_bytes());
    }
    queue.write_buffer(buffer, 0, &bytes);
}

const SHADER: &str = r#"
struct Uniforms {
    origin_size: vec4<f32>,
    radius_pad: vec4<f32>,
    fill: vec4<f32>,
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
    let ear = 1.0 - inside;
    // Clear reads only alpha (dest-out); paint wants premultiplied colour.
    return vec4<f32>(u.fill.rgb * u.fill.a * ear, u.fill.a * ear);
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
