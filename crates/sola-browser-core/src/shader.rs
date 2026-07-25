//! Shared fullscreen-sample shader pipeline for browser frames.
//!
//! Both engines import a texture (dma-buf or CPU upload) and sample it
//! with the same WGSL. Engine crates still own `shader::Program` for
//! input translation; they use [`SamplePipeline`] for create / render /
//! FPS bookkeeping so the WGSL and three-state Clear/Load logic never
//! drift.

use std::time::Instant;

use iced::Rectangle;

/// Result of importing one frame into a GPU texture the pipeline can sample.
pub struct ImportedTexture {
    pub bind_group: wgpu::BindGroup,
    /// Pixel size of the imported content (compared to last Resize).
    pub size: (u32, u32),
}

/// Engine-specific frame import. Implemented by WPE (dma-buf) and CEF (CPU).
pub trait FrameImport {
    type Frame: Send + 'static;
    /// GPU resources that must stay alive while the bind group is used.
    type Hold;

    fn import(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        frame: Self::Frame,
    ) -> Option<(ImportedTexture, Self::Hold)>;
}

/// Shared sample pipeline: WGSL fullscreen triangle + bind-group slots.
#[derive(Debug)]
pub struct SamplePipeline {
    pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub bind_group: Option<wgpu::BindGroup>,
    /// Size of the frame currently bound, if any.
    pub frame_size: Option<(u32, u32)>,
    fps_count: u64,
    fps_window_start: Instant,
}

impl SamplePipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, label: &str) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(&format!("{label} sampler")),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(&format!("{label} bgl")),
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

        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("{label} shader")),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{label} pl")),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("{label} rp")),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            bind_group: None,
            frame_size: None,
            fps_count: 0,
            fps_window_start: Instant::now(),
        }
    }

    pub fn install(&mut self, imported: ImportedTexture) {
        self.bind_group = Some(imported.bind_group);
        self.frame_size = Some(imported.size);
    }

    /// FPS counter — logs at debug every ~1s. Bench harness can enable
    /// `SOLA_BROWSER_FPS=1` for info-level lines.
    pub fn note_frame(&mut self) {
        self.fps_count += 1;
        let elapsed = self.fps_window_start.elapsed();
        if elapsed >= std::time::Duration::from_secs(1) {
            let fps = self.fps_count as f64 / elapsed.as_secs_f64();
            if std::env::var_os("SOLA_BROWSER_FPS").is_some() {
                tracing::info!(fps = format!("{:.1}", fps), "shader fps");
            } else {
                tracing::debug!(fps = format!("{:.1}", fps), "shader fps");
            }
            self.fps_count = 0;
            self.fps_window_start = Instant::now();
        }
    }

    /// Three-state Clear/Load/draw (white-flash / black-rect / stretch fixes).
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
        last_requested_size: (u32, u32),
        pass_label: &str,
    ) {
        let (load_op, do_draw) = match self.frame_size {
            None => (wgpu::LoadOp::Clear(wgpu::Color::BLACK), false),
            Some(size) if size == last_requested_size => (wgpu::LoadOp::Load, true),
            Some(_) => (wgpu::LoadOp::Load, false),
        };

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(pass_label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: load_op,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let Some(bg) = (do_draw).then_some(self.bind_group.as_ref()).flatten() else {
            return;
        };

        pass.set_viewport(
            clip_bounds.x as f32,
            clip_bounds.y as f32,
            clip_bounds.width as f32,
            clip_bounds.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bg, &[]);
        pass.draw(0..3, 0..1);
    }
}

const SHADER_WGSL: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var out: VsOut;
    let x = f32((vid << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vid & 2u) * 2.0 - 1.0;
    out.pos = vec4<f32>(x, -y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}
"#;
