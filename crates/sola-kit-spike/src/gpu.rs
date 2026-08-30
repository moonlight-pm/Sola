//! wgpu: CSS-sized hole (offscreen readback for dumps) + window present.

use std::sync::mpsc;

#[allow(dead_code)]
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
}

impl Gpu {
    pub fn new() -> Option<Self> {
        activate_gpu_env();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });
        let adapter = match pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            },
        )) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(%e, "wgpu request_adapter");
                return None;
            }
        };
        tracing::info!(
            name = %adapter.get_info().name,
            backend = ?adapter.get_info().backend,
            "wgpu adapter"
        );
        let (device, queue) = request_device(&adapter)?;
        let layout = hole_bgl(&device);
        let pipeline = hole_pipeline(&device, &layout, wgpu::TextureFormat::Rgba8Unorm);
        let uniform = uniform_buf(&device);
        Some(Self {
            device,
            queue,
            pipeline,
            layout,
            uniform,
        })
    }

    /// CSS-box sized GPU frame as 0x00RRGGBB pixels (dump / software blit).
    pub fn render(&self, width: u32, height: u32, time: f32) -> Option<Vec<u32>> {
        let width = width.max(1);
        let height = height.max(1);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hole-tex"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        write_time(&self.queue, &self.uniform, time);
        let bind = hole_bind(&self.device, &self.layout, &self.uniform);

        let padded = (width * 4 + 255) & !255;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hole-rb"),
            size: padded as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("hole-enc"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hole-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range();
        let mut out = vec![0u32; (width * height) as usize];
        for y in 0..height as usize {
            let row = &data[y * padded as usize..][..(width as usize * 4)];
            for x in 0..width as usize {
                let r = row[x * 4] as u32;
                let g = row[x * 4 + 1] as u32;
                let b = row[x * 4 + 2] as u32;
                out[y * width as usize + x] = (r << 16) | (g << 8) | b;
            }
        }
        drop(data);
        readback.unmap();
        Some(out)
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Quad {
    pub xywh: [f32; 4],
    pub color: [f32; 4],
    pub clip: [f32; 4],
    pub extra: [f32; 4], // radius, border_width, grad_mode, _
    pub color2: [f32; 4],
}

/// Window swapchain: GPU CSS boxes + glyph texture + optional parent-hole scissor.
pub struct Present {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    hole_pipeline: wgpu::RenderPipeline,
    chrome_pipeline: wgpu::RenderPipeline,
    quad_pipeline: wgpu::RenderPipeline,
    hole_layout: wgpu::BindGroupLayout,
    chrome_layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    screen: wgpu::Buffer,
    quad_bind: wgpu::BindGroup,
    instance: Option<wgpu::Buffer>,
    instance_cap: u64,
    chrome: Option<ChromeTex>,
    rgba: Vec<u8>,
    logged_quads: bool,
}

struct ChromeTex {
    w: u32,
    h: u32,
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
}

impl Present {
    pub fn new(
        display: *mut std::ffi::c_void,
        surface: *mut std::ffi::c_void,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        activate_gpu_env();
        let display = std::ptr::NonNull::new(display)?;
        let surface_ptr = std::ptr::NonNull::new(surface)?;
        let raw_display = raw_window_handle::RawDisplayHandle::Wayland(
            raw_window_handle::WaylandDisplayHandle::new(display),
        );
        let raw_window = raw_window_handle::RawWindowHandle::Wayland(
            raw_window_handle::WaylandWindowHandle::new(surface_ptr),
        );
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });
        let surface = match unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: raw_display,
                raw_window_handle: raw_window,
            })
        } {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%e, "wgpu create_surface");
                return None;
            }
        };
        let adapter = match pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            },
        )) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(%e, "wgpu request_adapter");
                return None;
            }
        };
        tracing::info!(
            name = %adapter.get_info().name,
            backend = ?adapter.get_info().backend,
            "wgpu adapter"
        );
        let (device, queue) = request_device(&adapter)?;
        let caps = surface.get_capabilities(&adapter);
        let format = pick_format(&caps.formats)?;
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
            wgpu::PresentMode::Fifo
        } else {
            caps.present_modes.first().copied()?
        };
        let alpha_mode = if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            caps.alpha_modes.first().copied()?
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let hole_layout = hole_bgl(&device);
        let hole_pipeline = hole_pipeline(&device, &hole_layout, format);
        let chrome_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chrome-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let chrome_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chrome-pl"),
            bind_group_layouts: &[&chrome_layout],
            push_constant_ranges: &[],
        });
        let chrome_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chrome"),
            source: wgpu::ShaderSource::Wgsl(CHROME_WGSL.into()),
        });
        let chrome_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("chrome-pipe"),
            layout: Some(&chrome_pl),
            vertex: wgpu::VertexState {
                module: &chrome_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &chrome_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let quad_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let quad_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad-pl"),
            bind_group_layouts: &[&quad_layout],
            push_constant_ranges: &[],
        });
        let quad_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad"),
            source: wgpu::ShaderSource::Wgsl(QUAD_WGSL.into()),
        });
        let quad_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad-pipe"),
            layout: Some(&quad_pl),
            vertex: wgpu::VertexState {
                module: &quad_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Quad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 32,
                            shader_location: 2,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 48,
                            shader_location: 3,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 64,
                            shader_location: 4,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &quad_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        tracing::info!(
            ?format,
            ?present_mode,
            ?alpha_mode,
            w = config.width,
            h = config.height,
            "wgpu present (GPU CSS boxes + glyph overlay)"
        );
        let uniform = uniform_buf(&device);
        let screen = uniform_buf(&device);
        let quad_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("quad-bg"),
            layout: &quad_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen.as_entire_binding(),
            }],
        });
        Some(Self {
            surface,
            device,
            queue,
            config,
            hole_pipeline,
            chrome_pipeline,
            quad_pipeline,
            hole_layout,
            chrome_layout,
            uniform,
            screen,
            quad_bind,
            instance: None,
            instance_cap: 0,
            chrome: None,
            rgba: Vec::new(),
            logged_quads: false,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.config.width == width && self.config.height == height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn frame(
        &mut self,
        quads: &[Quad],
        glyphs: &[u32],
        width: u32,
        height: u32,
        hole: Option<(u32, u32, u32, u32)>,
        time: f32,
        window_radius: f32,
    ) {
        self.resize(width, height);
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(?e, "wgpu get_current_texture");
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        self.queue.write_buffer(
            &self.screen,
            0,
            bytemuck::bytes_of(&[width as f32, height as f32, window_radius, 0.0f32]),
        );
        self.upload_instances(quads);
        self.upload_chrome(glyphs, width, height, window_radius);
        let Some(chrome_tex) = self.chrome.as_ref() else {
            return;
        };
        write_time(&self.queue, &self.uniform, time);
        let hole_bind = hole_bind(&self.device, &self.hole_layout, &self.uniform);
        let view = frame.texture.create_view(&Default::default());
        let nq = quads.len() as u32;
        if !self.logged_quads {
            tracing::info!(quads = nq, "gpu CSS boxes");
            self.logged_quads = true;
        }
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present-enc"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if window_radius > 0.5 {
                            wgpu::Color::TRANSPARENT
                        } else {
                            wgpu::Color {
                                r: 0.047,
                                g: 0.055,
                                b: 0.071,
                                a: 1.0,
                            }
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if nq > 0 {
                if let Some(inst) = self.instance.as_ref() {
                    pass.set_pipeline(&self.quad_pipeline);
                    pass.set_bind_group(0, &self.quad_bind, &[]);
                    pass.set_vertex_buffer(0, inst.slice(..));
                    pass.draw(0..6, 0..nq);
                }
            }
            pass.set_pipeline(&self.chrome_pipeline);
            pass.set_bind_group(0, &chrome_tex.bind, &[]);
            pass.draw(0..3, 0..1);
            if let Some((x, y, w, h)) = hole.and_then(|r| clamp_hole(r, width, height)) {
                pass.set_pipeline(&self.hole_pipeline);
                pass.set_bind_group(0, &hole_bind, &[]);
                pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, w, h);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit(Some(enc.finish()));
        frame.present();
    }

    fn upload_instances(&mut self, quads: &[Quad]) {
        let bytes = (quads.len().max(1) * std::mem::size_of::<Quad>()) as u64;
        let need_new = self.instance.as_ref().is_none_or(|_| self.instance_cap < bytes);
        if need_new {
            self.instance = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad-inst"),
                size: bytes,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.instance_cap = bytes;
        }
        if !quads.is_empty() {
            self.queue
                .write_buffer(self.instance.as_ref().unwrap(), 0, bytemuck::cast_slice(quads));
        }
    }

    fn upload_chrome(&mut self, chrome: &[u32], width: u32, height: u32, radius: f32) {
        let width = width.max(1);
        let height = height.max(1);
        let n = (width as usize) * (height as usize);
        self.rgba.resize(n * 4, 0);
        for (i, p) in chrome.iter().take(n).enumerate() {
            self.rgba[i * 4] = ((p >> 16) & 0xff) as u8;
            self.rgba[i * 4 + 1] = ((p >> 8) & 0xff) as u8;
            self.rgba[i * 4 + 2] = (p & 0xff) as u8;
            self.rgba[i * 4 + 3] = ((p >> 24) & 0xff) as u8;
        }
        let need_new = self
            .chrome
            .as_ref()
            .is_none_or(|c| c.w != width || c.h != height);
        if need_new {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("chrome-tex"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("chrome-bg"),
                layout: &self.chrome_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
            });
            self.chrome = Some(ChromeTex {
                w: width,
                h: height,
                texture,
                bind,
            });
        }
        if radius > 0.5 {
            round_mask_rgba(&mut self.rgba, width, height, radius);
        }
        let tex = &self.chrome.as_ref().unwrap().texture;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn round_mask_rgba(rgba: &mut [u8], width: u32, height: u32, radius: f32) {
    let w = width as f32;
    let h = height as f32;
    let hx = w * 0.5;
    let hy = h * 0.5;
    let r = radius.min(hx.min(hy));
    for y in 0..height {
        for x in 0..width {
            let px = x as f32 + 0.5 - hx;
            let py = y as f32 + 0.5 - hy;
            let qx = px.abs() - hx + r;
            let qy = py.abs() - hy + r;
            let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover >= 0.999 {
                continue;
            }
            let i = ((y * width + x) * 4) as usize;
            if cover < 0.004 {
                rgba[i] = 0;
                rgba[i + 1] = 0;
                rgba[i + 2] = 0;
                rgba[i + 3] = 0;
                continue;
            }
            rgba[i] = (rgba[i] as f32 * cover).round() as u8;
            rgba[i + 1] = (rgba[i + 1] as f32 * cover).round() as u8;
            rgba[i + 2] = (rgba[i + 2] as f32 * cover).round() as u8;
            rgba[i + 3] = (rgba[i + 3] as f32 * cover).round() as u8;
        }
    }
}

/// wgpu swapchain on a raw `wl_surface` (the CSS-hole subsurface).
#[allow(dead_code)]
pub struct ChildSwap {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
}

impl ChildSwap {
    pub fn new(display: *mut std::ffi::c_void, surface: *mut std::ffi::c_void) -> Option<Self> {
        activate_gpu_env();
        let display = std::ptr::NonNull::new(display)?;
        let surface_ptr = std::ptr::NonNull::new(surface)?;
        let raw_display = raw_window_handle::RawDisplayHandle::Wayland(
            raw_window_handle::WaylandDisplayHandle::new(display),
        );
        let raw_window = raw_window_handle::RawWindowHandle::Wayland(
            raw_window_handle::WaylandWindowHandle::new(surface_ptr),
        );
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });
        let surface = match unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: raw_display,
                raw_window_handle: raw_window,
            })
        } {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%e, "wgpu child create_surface");
                return None;
            }
        };
        let adapter = match pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            },
        )) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(%e, "wgpu child request_adapter");
                return None;
            }
        };
        tracing::info!(
            name = %adapter.get_info().name,
            backend = ?adapter.get_info().backend,
            "wgpu child adapter"
        );
        let (device, queue) = request_device(&adapter)?;
        let caps = surface.get_capabilities(&adapter);
        let format = pick_format(&caps.formats)?;
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
            wgpu::PresentMode::Fifo
        } else {
            caps.present_modes.first().copied()?
        };
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes.first().copied()?
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let layout = hole_bgl(&device);
        let pipeline = color_pipeline(&device, &layout, format, STRIPE_WGSL);
        tracing::info!(
            ?format,
            ?present_mode,
            ?alpha_mode,
            "wgpu present on wl_subsurface"
        );
        let uniform = uniform_buf(&device);
        Some(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            layout,
            uniform,
        })
    }

    pub fn frame(&mut self, width: u32, height: u32, time: f32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(?e, "wgpu child get_current_texture");
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        write_time(&self.queue, &self.uniform, time);
        let bind = hole_bind(&self.device, &self.layout, &self.uniform);
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("child-enc"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("child-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(enc.finish()));
        frame.present();
    }
}

fn clamp_hole(
    (x, y, w, h): (u32, u32, u32, u32),
    sw: u32,
    sh: u32,
) -> Option<(u32, u32, u32, u32)> {
    if x >= sw || y >= sh {
        return None;
    }
    let w = w.min(sw - x).max(1);
    let h = h.min(sh - y).max(1);
    Some((x, y, w, h))
}

fn pick_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    const PREFER: &[wgpu::TextureFormat] = &[
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ];
    for f in PREFER {
        if formats.contains(f) {
            return Some(*f);
        }
    }
    formats.first().copied()
}

fn request_device(adapter: &wgpu::Adapter) -> Option<(wgpu::Device, wgpu::Queue)> {
    match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("sola-kit-spike"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        memory_hints: Default::default(),
        trace: Default::default(),
        experimental_features: Default::default(),
    })) {
        Ok((device, queue)) => {
            device.on_uncaptured_error(std::sync::Arc::new(|e| {
                tracing::error!(?e, "wgpu uncaptured");
            }));
            Some((device, queue))
        }
        Err(e) => {
            tracing::warn!(%e, "wgpu request_device");
            None
        }
    }
}

fn hole_bgl(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("hole-bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn hole_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    color_pipeline(device, layout, format, HOLE_WGSL)
}

fn color_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    src: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("color"),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("hole-pl"),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("hole-pipe"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn uniform_buf(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hole-u"),
        size: 256,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn write_time(queue: &wgpu::Queue, uniform: &wgpu::Buffer, time: f32) {
    queue.write_buffer(uniform, 0, bytemuck::bytes_of(&[time, 0.0f32, 0.0, 0.0]));
}

fn hole_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hole-bg"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    })
}

fn activate_gpu_env() {
    unsafe {
        if std::env::var_os("__EGL_VENDOR_LIBRARY_DIRS").is_none() {
            std::env::set_var(
                "__EGL_VENDOR_LIBRARY_DIRS",
                "/run/opengl-driver/share/glvnd/egl_vendor.d",
            );
        }
        if std::env::var_os("VK_ICD_FILENAMES").is_none() {
            std::env::set_var(
                "VK_ICD_FILENAMES",
                "/run/opengl-driver/share/vulkan/icd.d/nvidia_icd.x86_64.json",
            );
        }
    }
}

const QUAD_WGSL: &str = r#"
struct Screen { size: vec2<f32>, radius: f32, _p: f32 }
@group(0) @binding(0) var<uniform> screen: Screen;

struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) px: vec2<f32>,
  @location(1) color: vec4<f32>,
  @location(2) color2: vec4<f32>,
  @location(3) clip: vec4<f32>,
  @location(4) local: vec2<f32>,
  @location(5) size: vec2<f32>,
  @location(6) radius: f32,
  @location(7) border: f32,
  @location(8) mode: f32,
  @location(9) hue: f32,
}

@vertex
fn vs(
  @builtin(vertex_index) vi: u32,
  @location(0) xywh: vec4<f32>,
  @location(1) color: vec4<f32>,
  @location(2) clip: vec4<f32>,
  @location(3) extra: vec4<f32>,
  @location(4) color2: vec4<f32>,
) -> VOut {
  var c = array<vec2<f32>, 6>(
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
    vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
  );
  let uv = c[vi];
  let p = xywh.xy + uv * xywh.zw;
  var o: VOut;
  o.pos = vec4(p.x / screen.size.x * 2.0 - 1.0, 1.0 - p.y / screen.size.y * 2.0, 0.0, 1.0);
  o.px = p;
  o.color = color;
  o.color2 = color2;
  o.clip = clip;
  o.local = uv * xywh.zw;
  o.size = xywh.zw;
  o.radius = extra.x;
  o.border = extra.y;
  o.mode = extra.z;
  o.hue = extra.w;
  return o;
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3<f32> {
  let c = v * s;
  let hp = h * 6.0;
  let hp2 = hp - 2.0 * floor(hp * 0.5);
  let x = c * (1.0 - abs(hp2 - 1.0));
  let m = v - c;
  var rgb = vec3(0.0);
  if (hp < 1.0) { rgb = vec3(c, x, 0.0); }
  else if (hp < 2.0) { rgb = vec3(x, c, 0.0); }
  else if (hp < 3.0) { rgb = vec3(0.0, c, x); }
  else if (hp < 4.0) { rgb = vec3(0.0, x, c); }
  else if (hp < 5.0) { rgb = vec3(x, 0.0, c); }
  else { rgb = vec3(c, 0.0, x); }
  return rgb + vec3(m);
}

fn sd_round(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
  let q = abs(p) - b + vec2(r);
  return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn srgb_to_lin(c: vec3<f32>) -> vec3<f32> {
  let lo = c / 12.92;
  let hi = pow(max((c + 0.055) / 1.055, vec3(0.0)), vec3(2.4));
  return select(hi, lo, c <= vec3(0.04045));
}

fn lin_to_srgb(c: vec3<f32>) -> vec3<f32> {
  let lo = c * 12.92;
  let hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
  return select(hi, lo, c <= vec3(0.0031308));
}

// 4×4 Bayer, −0.5..0.5 ulp — 8-bit UNORM fills have ~7 mix steps
// between close graphite stops; iced's sRGB surface hides that.
fn bayer(p: vec2<f32>) -> f32 {
  let x = u32(p.x) & 3u;
  let y = u32(p.y) & 3u;
  let m = array<f32, 16>(
    0.0, 8.0, 2.0, 10.0,
    12.0, 4.0, 14.0, 6.0,
    3.0, 11.0, 1.0, 9.0,
    15.0, 7.0, 13.0, 5.0,
  );
  return (m[y * 4u + x] + 0.5) / 16.0 - 0.5;
}

fn grad_mix(a: vec3<f32>, b: vec3<f32>, t: f32, px: vec2<f32>) -> vec3<f32> {
  let mixed = mix(srgb_to_lin(a), srgb_to_lin(b), clamp(t, 0.0, 1.0));
  return clamp(lin_to_srgb(mixed) + vec3(bayer(px) / 255.0), vec3(0.0), vec3(1.0));
}

@fragment
fn fs(v: VOut) -> @location(0) vec4<f32> {
  if (v.px.x < v.clip.x || v.px.y < v.clip.y || v.px.x >= v.clip.x + v.clip.z || v.px.y >= v.clip.y + v.clip.w) {
    discard;
  }
  let half = v.size * 0.5;
  let p = v.local - half;
  let r = min(v.radius, min(half.x, half.y));
  let d = sd_round(p, half, r);
  var a = clamp(0.5 - d, 0.0, 1.0);
  if (screen.radius > 0.5) {
    let wh = screen.size * 0.5;
    let wr = min(screen.radius, min(wh.x, wh.y));
    let wd = sd_round(v.px - wh, wh, wr);
    a *= clamp(0.5 - wd, 0.0, 1.0);
  }
  if (a < 0.004) {
    discard;
  }
  var rgb = v.color.rgb;
  if (v.mode > 0.5 && v.mode < 1.5) {
    let t = clamp(v.local.y / max(v.size.y, 1.0), 0.0, 1.0);
    rgb = grad_mix(v.color.rgb, v.color2.rgb, t, v.px);
  } else if (v.mode > 1.5 && v.mode < 2.5) {
    let t = clamp((v.local.x / max(v.size.x, 1.0) + v.local.y / max(v.size.y, 1.0)) * 0.5, 0.0, 1.0);
    rgb = grad_mix(v.color.rgb, v.color2.rgb, t, v.px);
  } else if (v.mode > 2.5 && v.mode < 3.5) {
    let sat = clamp(v.local.x / max(v.size.x, 1.0), 0.0, 1.0);
    let val = 1.0 - clamp(v.local.y / max(v.size.y, 1.0), 0.0, 1.0);
    rgb = hsv_to_rgb(v.hue, sat, val);
  } else if (v.mode > 3.5 && v.mode < 4.5) {
    let hue = clamp(v.local.x / max(v.size.x, 1.0), 0.0, 1.0);
    rgb = hsv_to_rgb(hue, 1.0, 1.0);
  } else if (v.mode > 4.5) {
    let t = clamp(v.local.x / max(v.size.x, 1.0), 0.0, 1.0);
    let cx = i32(floor(v.local.x / 8.0));
    let cy = i32(floor(v.local.y / 8.0));
    let g = select(0.22, 0.32, ((cx + cy) & 1) == 0);
    rgb = mix(vec3(g), v.color.rgb, t);
  }
  // Hairline rim on large surfaces (cards). Small radii (buttons) skip —
  // the mix reads as a 45° bevel on a 26px control.
  if (v.border > 0.4 && v.radius > 10.0) {
    let t = smoothstep(-v.border - 0.5, 0.5, d);
    rgb = mix(rgb, rgb * 0.82 + vec3(0.18), t);
  }
  let cov = v.color.a * a;
  return vec4(rgb * cov, cov);
}
"#;

const STRIPE_WGSL: &str = r#"
struct U { time: f32, _p: vec3<f32> }
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
  var p = array<vec2<f32>, 3>(
    vec2(-1.0, -1.0),
    vec2( 3.0, -1.0),
    vec2(-1.0,  3.0),
  );
  var o: VOut;
  o.pos = vec4(p[i], 0.0, 1.0);
  o.uv = p[i] * 0.5 + 0.5;
  return o;
}

@fragment
fn fs(v: VOut) -> @location(0) vec4<f32> {
  let s = sin((v.uv.x + v.uv.y) * 14.0 - u.time * 3.0);
  let g = smoothstep(-0.15, 0.15, s);
  let orange = vec3(0.878, 0.416, 0.102);
  let navy = vec3(0.07, 0.11, 0.164);
  return vec4(mix(navy, orange, g), 1.0);
}
"#;

const HOLE_WGSL: &str = r#"
struct U { time: f32, _p: vec3<f32> }
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
  var p = array<vec2<f32>, 3>(
    vec2(-1.0, -1.0),
    vec2( 3.0, -1.0),
    vec2(-1.0,  3.0),
  );
  var o: VOut;
  o.pos = vec4(p[i], 0.0, 1.0);
  o.uv = p[i] * 0.5 + 0.5;
  return o;
}

@fragment
fn fs(v: VOut) -> @location(0) vec4<f32> {
  let t = u.time;
  let g = 0.15 + 0.15 * sin(t * 2.0 + v.uv.x * 6.0);
  return vec4(v.uv.x, g, v.uv.y, 1.0);
}
"#;

const CHROME_WGSL: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  var p = array<vec2<f32>, 3>(
    vec2(-1.0, -1.0),
    vec2( 3.0, -1.0),
    vec2(-1.0,  3.0),
  );
  return vec4(p[i], 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  let p = vec2<i32>(pos.xy);
  let c = textureLoad(tex, p, 0);
  return vec4(c.rgb * c.a, c.a);
}
"#;
