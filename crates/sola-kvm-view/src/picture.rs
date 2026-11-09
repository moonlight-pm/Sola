//! The picture on the GPU, and the pointer over it. Two textures (luma and
//! half-size chroma), replaced whole when a new picture comes, on one quad
//! letterboxed in the widget.

use std::sync::Arc;

use iced::widget::shader::{self, Action, Viewport};
use iced::{Event, Point, Rectangle, mouse};
use sola_kvm::protocol::Packet;

use crate::app::Msg;
use crate::fit::{fit, to_points};
use crate::pictures::{Frame, Pictures};

/// Pixels of a smooth scroll that make one wheel detent.
const PIXELS_PER_DETENT: f32 = 32.0;

/// The picture and what the pointer does over it.
pub struct Picture {
    pub pictures: Arc<Pictures>,
    /// The other machine's coordinates; `None` until it says.
    pub points: Option<(u32, u32)>,
    /// The pointer and keyboard are going to the other machine.
    pub captured: bool,
}

impl Picture {
    fn over(&self, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Point> {
        cursor.position_in(bounds)
    }

    fn target(&self, bounds: Rectangle, at: Point) -> Option<(i32, i32)> {
        to_points(at, fit(self.pictures.size(), bounds.size()), self.points?)
    }
}

fn button_index(button: mouse::Button) -> Option<u8> {
    match button {
        mouse::Button::Left => Some(0),
        mouse::Button::Right => Some(1),
        mouse::Button::Middle => Some(2),
        _ => None,
    }
}

impl shader::Program<Msg> for Picture {
    type State = ();
    type Primitive = Primitive;

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Msg>> {
        let at = self.over(bounds, cursor)?;
        let Event::Mouse(event) = event else {
            return None;
        };
        let target = self.target(bounds, at)?;
        if !self.captured {
            return matches!(event, mouse::Event::ButtonPressed(mouse::Button::Left))
                .then(|| Action::publish(Msg::Capture).and_capture());
        }
        let packet = match *event {
            mouse::Event::CursorMoved { .. } => Packet::Motion { x: target.0, y: target.1 },
            mouse::Event::ButtonPressed(b) => Packet::Button { button: button_index(b)?, pressed: 1 },
            mouse::Event::ButtonReleased(b) => Packet::Button { button: button_index(b)?, pressed: 0 },
            mouse::Event::WheelScrolled { delta } => {
                let (dx, dy) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (x, y),
                    mouse::ScrollDelta::Pixels { x, y } => (x / PIXELS_PER_DETENT, y / PIXELS_PER_DETENT),
                };
                Packet::Scroll { dx, dy }
            }
            _ => return None,
        };
        Some(Action::publish(Msg::Pointer(packet)).and_capture())
    }

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        Primitive {
            pictures: Arc::clone(&self.pictures),
        }
    }

    fn mouse_interaction(&self, _state: &(), bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        // The other machine draws its own pointer into the picture.
        match self.over(bounds, cursor) {
            Some(at) if self.captured && self.target(bounds, at).is_some() => mouse::Interaction::Hidden,
            _ => mouse::Interaction::default(),
        }
    }
}

pub struct Primitive {
    pictures: Arc<Pictures>,
}

impl std::fmt::Debug for Primitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Primitive")
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
        if let Some(frame) = self.pictures.take() {
            pipe.upload(device, queue, &frame);
            self.pictures.give_back(frame);
        }
        pipe.place(queue, *bounds, viewport);
    }

    fn render(
        &self,
        pipe: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: &Rectangle<u32>,
    ) {
        let Some(textures) = &pipe.textures else {
            return;
        };
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("remote view"),
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
        pass.set_pipeline(&pipe.render);
        pass.set_bind_group(0, &textures.bind, &[]);
        pass.draw(0..6, 0..1);
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect: [f32; 4],
    srgb_target: u32,
    padding: [u32; 3],
}

struct Textures {
    size: (u32, u32),
    y: wgpu::Texture,
    uv: wgpu::Texture,
    bind: wgpu::BindGroup,
}

pub struct Pipeline {
    render: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    srgb_target: bool,
    textures: Option<Textures>,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pipeline")
    }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("remote view"),
            source: wgpu::ShaderSource::Wgsl(include_str!("picture.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("remote view"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("remote view"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let render = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("remote view"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                targets: &[Some(format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("remote view"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("remote view"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            render,
            layout,
            sampler,
            uniforms,
            srgb_target: format.is_srgb(),
            textures: None,
        }
    }
}

impl Pipeline {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        let size = (frame.width, frame.height);
        if self.textures.as_ref().is_none_or(|t| t.size != size) {
            self.textures = Some(self.make_textures(device, size));
        }
        let Some(textures) = &self.textures else {
            return;
        };
        write_plane(queue, &textures.y, &frame.y, size, 1);
        write_plane(queue, &textures.uv, &frame.uv, (size.0 / 2, size.1.div_ceil(2)), 2);
    }

    fn make_textures(&self, device: &wgpu::Device, size: (u32, u32)) -> Textures {
        let plane = |label, size: (u32, u32), format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let y = plane("remote view luma", size, wgpu::TextureFormat::R8Unorm);
        let uv = plane("remote view chroma", (size.0 / 2, size.1.div_ceil(2)), wgpu::TextureFormat::Rg8Unorm);
        let y_view = y.create_view(&Default::default());
        let uv_view = uv.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("remote view"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&y_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&uv_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Textures { size, y, uv, bind }
    }

    /// Put the quad where the picture goes, in clip space.
    fn place(&self, queue: &wgpu::Queue, bounds: Rectangle, viewport: &Viewport) {
        let Some(textures) = &self.textures else {
            return;
        };
        let target = viewport.physical_size();
        let scale = viewport.scale_factor() as f32;
        let picture = fit(textures.size, bounds.size());
        let x = |logical: f32| (bounds.x + logical) * scale / target.width as f32 * 2.0 - 1.0;
        let y = |logical: f32| 1.0 - (bounds.y + logical) * scale / target.height as f32 * 2.0;
        let uniforms = Uniforms {
            rect: [x(picture.x), y(picture.y), x(picture.x + picture.width), y(picture.y + picture.height)],
            srgb_target: u32::from(self.srgb_target),
            padding: [0; 3],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }
}

fn write_plane(queue: &wgpu::Queue, texture: &wgpu::Texture, data: &[u8], size: (u32, u32), bytes_per_texel: u32) {
    queue.write_texture(
        texture.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.0 * bytes_per_texel),
            rows_per_image: Some(size.1),
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
}
