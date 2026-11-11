//! The other machine's pointer, drawn over the picture where it was last
//! sent. It moves with this window's own redraw, not the trip there and
//! back; the host sends its image only when it leaves it out of the picture.

use std::sync::Arc;

use iced::Rectangle;
use sola_kvm::view_wire::Cursor;

use crate::picture::{Place, Uniforms};

pub struct Pointer {
    render: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    image: Option<Image>,
    /// The picture in physical pixels, where the pointer shows; `None`
    /// draws none.
    area: Option<Rectangle>,
}

struct Image {
    cursor: Arc<Cursor>,
    bind: wgpu::BindGroup,
}

impl Pointer {
    pub fn new(device: &wgpu::Device, module: &wgpu::ShaderModule, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("remote pointer"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("remote pointer"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let render = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("remote pointer"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_pointer"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("remote pointer"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("remote pointer"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            render,
            layout,
            sampler,
            uniforms,
            image: None,
            area: None,
        }
    }

    /// Upload `cursor` when it is not the one already here.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, cursor: Option<Arc<Cursor>>) {
        match cursor {
            Some(cursor) if self.image.as_ref().is_some_and(|i| Arc::ptr_eq(&i.cursor, &cursor)) => {}
            Some(cursor) if cursor.width > 0 && cursor.height > 0 => {
                self.image = Some(self.make_image(device, queue, cursor));
            }
            _ => self.image = None,
        }
    }

    fn make_image(&self, device: &wgpu::Device, queue: &wgpu::Queue, cursor: Arc<Cursor>) -> Image {
        let size = wgpu::Extent3d {
            width: u32::from(cursor.width),
            height: u32::from(cursor.height),
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("remote pointer"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Stored as sent; the shader decodes it as it does the picture.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &cursor.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 4),
                rows_per_image: Some(size.height),
            },
            size,
        );
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("remote pointer"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        Image { cursor, bind }
    }

    /// Put the pointer's hot spot at `at` (0 to 1 across the picture), scaled
    /// as the picture is: `picture` in the widget, `pixels` wide.
    pub fn place(
        &mut self,
        queue: &wgpu::Queue,
        place: &Place,
        picture: Rectangle,
        pixels: u32,
        at: Option<(f32, f32)>,
        srgb_target: bool,
    ) {
        self.area = None;
        let (Some(image), Some((x, y))) = (&self.image, at) else {
            return;
        };
        let cursor = &image.cursor;
        let per_pixel = picture.width / pixels.max(1) as f32;
        let rect = Rectangle {
            x: picture.x + x * picture.width - f32::from(cursor.hot_x) * per_pixel,
            y: picture.y + y * picture.height - f32::from(cursor.hot_y) * per_pixel,
            width: f32::from(cursor.width) * per_pixel,
            height: f32::from(cursor.height) * per_pixel,
        };
        let uniforms = Uniforms::new(place.clip(rect), srgb_target);
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        self.area = Some(place.physical(picture));
    }

    /// Draw it in the picture's pass, cut to the picture as the other
    /// machine's screen cuts it.
    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>, clip: &Rectangle<u32>) {
        let (Some(image), Some(area)) = (&self.image, self.area) else {
            return;
        };
        let clip = Rectangle {
            x: clip.x as f32,
            y: clip.y as f32,
            width: clip.width as f32,
            height: clip.height as f32,
        };
        let Some(shown) = clip.intersection(&area) else {
            return;
        };
        let (left, top) = (shown.x.round() as u32, shown.y.round() as u32);
        let (right, bottom) = ((shown.x + shown.width).round() as u32, (shown.y + shown.height).round() as u32);
        if right <= left || bottom <= top {
            return;
        }
        pass.set_scissor_rect(left, top, right - left, bottom - top);
        pass.set_pipeline(&self.render);
        pass.set_bind_group(0, &image.bind, &[]);
        pass.draw(0..6, 0..1);
    }
}
