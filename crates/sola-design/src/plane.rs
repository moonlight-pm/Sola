//! Artboard textures on the plane.
//!
//! One wgpu texture per board tab, with a full mip chain so a zoomed-out
//! canvas stays legible. CEF dirty rects upload into mip 0 (the helper
//! always sends the whole composite; the rects say what changed), then the
//! chain is rebuilt. Boards draw at the camera transform, clipped to the
//! widget. Pan and zoom only rebuild six vertices per board — CEF never
//! repaints for them.
//!
//! Past a board's raster scale a detail render (see [`crate::detail`]) is
//! drawn over it: one more texture holding the visible part at screen
//! scale. A frame's `view` says which picture it is; the board's own
//! picture sits in the top-left of a frame (the surface can be larger).

use std::collections::HashMap;
use std::sync::Arc;

use iced::widget::shader::{self, Viewport};
use iced::{Rectangle, mouse};
use sola_browser::cef::cpu_import;
use sola_browser::cef::engine::CefFrame;
use sola_browser::engine::BoardDetail;

use crate::camera::Camera;
use crate::detail::View;
use crate::frames::BoardFrames;

/// One board as the plane draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Item {
    pub tab: u64,
    /// Canvas units.
    pub rect: Rectangle,
    /// The board's own pixels (top-left of each whole-board frame).
    pub pixels: (u32, u32),
    /// Frame size; frames of any other size are stale.
    pub surface: (u32, u32),
    /// Requests whose frames can be drawn (see [`crate::detail::View`]).
    pub views: [Option<View>; 2],
    /// Draw the board's detail texture, if it has one.
    pub detail: bool,
    /// On the canvas on screen. Off-screen boards keep their textures
    /// fresh but do not draw.
    pub shown: bool,
}

impl Item {
    /// What a frame tagged `view` shows: `Some(None)` the whole board,
    /// `Some(Some(d))` a detail, `None` a request no longer known.
    fn picture(&self, view: u32) -> Option<Option<BoardDetail>> {
        if let Some(v) = self.views.iter().flatten().find(|v| v.seq == view) {
            return Some(v.detail);
        }
        // Untagged frames are the whole board until the first request.
        (view == 0 && self.views.iter().all(Option::is_none)).then_some(None)
    }
}

pub struct Plane {
    pub frames: Arc<BoardFrames>,
    pub items: Vec<Item>,
    pub camera: Camera,
}

impl<M> shader::Program<M> for Plane {
    type State = ();
    type Primitive = Primitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        Primitive {
            frames: self.frames.clone(),
            items: self.items.clone(),
            camera: self.camera,
        }
    }
}

pub struct Primitive {
    frames: Arc<BoardFrames>,
    items: Vec<Item>,
    camera: Camera,
}

impl std::fmt::Debug for Primitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("plane::Primitive")
            .field("items", &self.items.len())
            .field("camera", &self.camera)
            .finish()
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipe: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let items: HashMap<u64, &Item> = self.items.iter().map(|i| (i.tab, i)).collect();
        pipe.textures.retain(|tab, _| items.contains_key(tab));
        pipe.details
            .retain(|tab, _| items.get(tab).is_some_and(|i| i.detail));
        pipe.last_view.retain(|tab, _| items.contains_key(tab));
        let mut changed = Vec::new();
        for (tab, frame) in self.frames.take_all() {
            let Some(item) = items.get(&tab) else {
                continue;
            };
            if pipe.upload(device, queue, item, &frame) {
                changed.push(tab);
            }
        }
        if !changed.is_empty() {
            pipe.rebuild_mips(device, queue, &changed);
        }
        pipe.build_draws(device, queue, *bounds, viewport, &self.camera, &self.items);
    }

    fn render(
        &self,
        pipe: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: &Rectangle<u32>,
    ) {
        if pipe.draws.is_empty() || clip.width == 0 || clip.height == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("design plane"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_pipeline(&pipe.draw_pipeline);
        pass.set_vertex_buffer(0, pipe.vertices.slice(..));
        for d in &pipe.draws {
            let bind_group = if d.detail {
                pipe.details.get(&d.tab).map(|t| &t.bind_group)
            } else {
                pipe.textures.get(&d.tab).map(|t| &t.bind_group)
            };
            let Some(bind_group) = bind_group else {
                continue;
            };
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(d.first..d.first + 6, 0..1);
        }
    }
}

struct BoardTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
    mips: u32,
}

/// A board's detail render: the surface-sized frame and what it shows.
struct DetailTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
    seq: u32,
    shows: BoardDetail,
}

struct Draw {
    tab: u64,
    detail: bool,
    first: u32,
}

pub struct Pipeline {
    draw_pipeline: wgpu::RenderPipeline,
    mip_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// Trilinear: boards are usually minified.
    sampler: wgpu::Sampler,
    /// Bilinear box-down for one mip level from the one above.
    mip_sampler: wgpu::Sampler,
    textures: HashMap<u64, BoardTexture>,
    details: HashMap<u64, DetailTexture>,
    /// View of the last frame taken per tab: dirty rects only hold
    /// between frames of one view.
    last_view: HashMap<u64, u32>,
    staging: Vec<u8>,
    vertices: wgpu::Buffer,
    draws: Vec<Draw>,
    max_side: u32,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("plane::Pipeline")
            .field("textures", &self.textures.len())
            .finish_non_exhaustive()
    }
}

/// Board textures hold CEF's BGRA bytes; sRGB so sampling and the mip
/// downsample work in linear light (same as sola-browser's import).
const BOARD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
const VERTEX_BYTES: u64 = 16;

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("design plane shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("design plane bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("design plane pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let quad_layout = wgpu::VertexBufferLayout {
            array_stride: VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
        };
        let make = |label: &str, vs: &str, buffers: &[wgpu::VertexBufferLayout], fmt| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: fmt,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let draw_pipeline = make("design plane draw", "vs_quad", &[quad_layout], format);
        let mip_pipeline = make("design plane mip", "vs_full", &[], BOARD_FORMAT);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("design plane sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mip_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("design plane mip sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let vertices = vertex_buffer(device, 64 * 6 * VERTEX_BYTES);
        Self {
            draw_pipeline,
            mip_pipeline,
            layout,
            sampler,
            mip_sampler,
            textures: HashMap::new(),
            details: HashMap::new(),
            last_view: HashMap::new(),
            staging: Vec::new(),
            vertices,
            draws: Vec::new(),
            max_side: device.limits().max_texture_dimension_2d,
        }
    }
}

impl Pipeline {
    /// Upload one frame to the board's texture or its detail texture. A
    /// new or resized texture gets a full upload; otherwise only the dirty
    /// rects. True when the board's own texture changed (mips to rebuild).
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        item: &Item,
        frame: &CefFrame,
    ) -> bool {
        let tab = item.tab;
        let size = (frame.width, frame.height);
        if size != item.surface {
            return false;
        }
        let Some(picture) = item.picture(frame.view) else {
            return false;
        };
        let same_view = self.last_view.insert(tab, frame.view) == Some(frame.view);
        match picture {
            None => {
                let px = item.pixels;
                if px.0 == 0 || px.1 == 0 || px.0 > self.max_side || px.1 > self.max_side {
                    tracing::warn!(tab, w = px.0, h = px.1, max = self.max_side, "board frame skipped");
                    return false;
                }
                let fresh = self.textures.get(&tab).is_none_or(|t| t.size != px);
                if fresh {
                    let tex = board_texture(device, &self.layout, &self.sampler, px, true);
                    self.textures.insert(tab, tex);
                }
                let tex = &self.textures[&tab];
                cpu_import::upload_cropped(
                    queue,
                    &tex.texture,
                    frame,
                    &mut self.staging,
                    fresh || !same_view,
                    px,
                );
                true
            }
            Some(shows) => {
                if !item.detail || size.0 > self.max_side || size.1 > self.max_side {
                    return false;
                }
                let fresh = self.details.get(&tab).is_none_or(|t| t.size != size);
                if fresh {
                    let t = board_texture(device, &self.layout, &self.sampler, size, false);
                    self.details.insert(
                        tab,
                        DetailTexture {
                            texture: t.texture,
                            bind_group: t.bind_group,
                            size,
                            seq: frame.view,
                            shows,
                        },
                    );
                }
                let tex = self.details.get_mut(&tab).expect("detail texture");
                let full = fresh || !same_view || tex.seq != frame.view;
                tex.seq = frame.view;
                tex.shows = shows;
                cpu_import::upload(queue, &tex.texture, frame, &mut self.staging, full);
                false
            }
        }
    }

    fn rebuild_mips(&self, device: &wgpu::Device, queue: &wgpu::Queue, tabs: &[u64]) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("design plane mips"),
        });
        for tab in tabs {
            let Some(t) = self.textures.get(tab) else {
                continue;
            };
            for level in 1..t.mips {
                let src = mip_view(&t.texture, level - 1);
                let dst = mip_view(&t.texture, level);
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("design plane mip bg"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&src),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.mip_sampler),
                        },
                    ],
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("design plane mip pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &dst,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.mip_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        // write_texture uploads land before this submit's passes.
        queue.submit([encoder.finish()]);
    }

    fn build_draws(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: Rectangle,
        viewport: &Viewport,
        camera: &Camera,
        items: &[Item],
    ) {
        self.draws.clear();
        let scale = viewport.scale_factor();
        let target = viewport.physical_size();
        let mut verts: Vec<f32> = Vec::with_capacity(items.len() * 48);
        let mut quad = |draws: &mut Vec<Draw>, tab, detail, [l, t, r, b]: [f32; 4], [u0, v0, u1, v1]: [f32; 4]| {
            let (l, t) = ndc(l, t, target);
            let (r, b) = ndc(r, b, target);
            draws.push(Draw {
                tab,
                detail,
                first: (verts.len() / 4) as u32,
            });
            for [x, y, u, v] in [
                [l, t, u0, v0],
                [r, t, u1, v0],
                [l, b, u0, v1],
                [r, t, u1, v0],
                [r, b, u1, v1],
                [l, b, u0, v1],
            ] {
                verts.extend([x, y, u, v]);
            }
        };
        let to_screen = |r: Rectangle| {
            let r = camera.rect_to_screen(r);
            Rectangle::new(iced::Point::new(bounds.x + r.x, bounds.y + r.y), r.size())
        };
        for item in items.iter().filter(|i| i.shown) {
            let Some(tex) = self.textures.get(&item.tab) else {
                continue;
            };
            let screen = to_screen(item.rect);
            if !screen.intersects(&bounds) {
                continue;
            }
            // At 1:1 (one texel per pixel) snap to whole pixels so the page
            // is sharp. Otherwise keep sub-pixel positions: snapping a scaled
            // board makes it jump a pixel as the zoom changes.
            let one_to_one = (screen.width * scale - tex.size.0 as f32).abs() < 0.5;
            let snap = |v: f32| if one_to_one { v.round() } else { v };
            let x0 = snap(screen.x * scale);
            let y0 = snap(screen.y * scale);
            let x1 = x0 + snap(screen.width * scale);
            let y1 = y0 + snap(screen.height * scale);
            quad(&mut self.draws, item.tab, false, [x0, y0, x1, y1], [0.0, 0.0, 1.0, 1.0]);

            if !item.detail {
                continue;
            }
            let Some(d) = self.details.get(&item.tab) else {
                continue;
            };
            // The part of the board the detail covers, in board CSS px.
            let k = d.shows.scale;
            let (dw, dh) = (f64::from(d.size.0), f64::from(d.size.1));
            let (bw, bh) = (f64::from(item.rect.width), f64::from(item.rect.height));
            let (cx0, cy0) = (d.shows.x.max(0.0), d.shows.y.max(0.0));
            let (cx1, cy1) = ((d.shows.x + dw / k).min(bw), (d.shows.y + dh / k).min(bh));
            if cx1 <= cx0 || cy1 <= cy0 {
                continue;
            }
            let uv = [
                ((cx0 - d.shows.x) * k / dw) as f32,
                ((cy0 - d.shows.y) * k / dh) as f32,
                ((cx1 - d.shows.x) * k / dw) as f32,
                ((cy1 - d.shows.y) * k / dh) as f32,
            ];
            let part = to_screen(Rectangle::new(
                iced::Point::new(item.rect.x + cx0 as f32, item.rect.y + cy0 as f32),
                iced::Size::new((cx1 - cx0) as f32, (cy1 - cy0) as f32),
            ));
            // Drawn at its own scale (the camera rested): put its texels on
            // pixels. Only the origin moves, so the scale stays exactly 1:1.
            let at_scale = (f64::from(camera.zoom * scale) - k).abs() < k * 1e-4;
            let (px, py) = (part.x * scale, part.y * scale);
            let (sx, sy) = if at_scale { (px.round() - px, py.round() - py) } else { (0.0, 0.0) };
            quad(
                &mut self.draws,
                item.tab,
                true,
                [
                    px + sx,
                    py + sy,
                    px + sx + part.width * scale,
                    py + sy + part.height * scale,
                ],
                uv,
            );
        }
        if verts.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&verts);
        if bytes.len() as u64 > self.vertices.size() {
            let cap = (bytes.len() as u64).next_power_of_two();
            self.vertices = vertex_buffer(device, cap);
        }
        queue.write_buffer(&self.vertices, 0, bytes);
    }
}

fn ndc(x: f32, y: f32, target: iced::Size<u32>) -> (f32, f32) {
    let w = target.width.max(1) as f32;
    let h = target.height.max(1) as f32;
    (x / w * 2.0 - 1.0, 1.0 - y / h * 2.0)
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("design plane vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// floor(log2(longest side)) + 1.
fn mip_count(w: u32, h: u32) -> u32 {
    32 - w.max(h).max(1).leading_zeros()
}

/// A board-sized texture; `mipped` for a board's own picture (minified),
/// one level for a detail (drawn near 1:1).
fn board_texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    (width, height): (u32, u32),
    mipped: bool,
) -> BoardTexture {
    let mips = if mipped { mip_count(width, height) } else { 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("design board"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: BOARD_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("design board bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    BoardTexture {
        texture,
        bind_group,
        size: (width, height),
        mips,
    }
}

fn mip_view(texture: &wgpu::Texture, level: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("design board mip"),
        base_mip_level: level,
        mip_level_count: Some(1),
        ..Default::default()
    })
}

const SHADER: &str = r#"
struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_quad(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> Out {
    var o: Out;
    o.pos = vec4<f32>(pos, 0.0, 1.0);
    o.uv = uv;
    return o;
}

// Fullscreen triangle for one mip level (uv 0,0 top-left).
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.pos = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    o.uv = uv;
    return o;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv);
    return vec4<f32>(c.rgb, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_counts() {
        assert_eq!(mip_count(1, 1), 1);
        assert_eq!(mip_count(2, 1), 2);
        assert_eq!(mip_count(2880, 1800), 12);
        assert_eq!(mip_count(4096, 10), 13);
    }

    #[test]
    fn ndc_corners() {
        let t = iced::Size::new(200, 100);
        assert_eq!(ndc(0.0, 0.0, t), (-1.0, 1.0));
        assert_eq!(ndc(200.0, 100.0, t), (1.0, -1.0));
        assert_eq!(ndc(100.0, 50.0, t), (0.0, 0.0));
    }
}
