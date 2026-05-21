//! iced `shader::Program` that samples the most recent CEF frame
//! as a fullscreen quad.
//!
//! Flow per frame:
//! 1. CEF worker thread copies `on_paint` BGRA bytes into a
//!    `CefFrame` and pushes through the channel.
//! 2. iced subscription stashes the frame in `slot.pending` and
//!    triggers a redraw.
//! 3. `Primitive::prepare` on the render thread takes the frame,
//!    (re-)creates the destination wgpu::Texture if dimensions
//!    changed, uploads via `queue.write_texture`, and rebuilds the
//!    bind group.
//! 4. `Primitive::render` issues a fullscreen-triangle draw call.
//!
//! No DMA-BUF, no FD lifetime, no resource Release back to CEF —
//! the buffer CEF handed us was already memcpy'd out in `on_paint`.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use iced::widget::shader;
use iced::{Rectangle, mouse};

use crate::cef::{CefFrame, Cmd};
use crate::cpu_import::{self, UploadedFrame};

/// Shared between the App (which fills `pending`) and the shader
/// Pipeline (which drains it on next prepare). `releaser` lets the
/// shader Program send `Cmd::Resize` back to the CEF worker when
/// the iced widget bounds change.
pub struct FrameSlot {
    pub pending: Mutex<Option<CefFrame>>,
    pub releaser: Sender<Cmd>,
    /// Last size we asked CEF to render at (physical pixels). Used
    /// to debounce resize commands so we only fire on actual change.
    pub last_size: Mutex<(u32, u32)>,
}

impl std::fmt::Debug for FrameSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameSlot").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct CefProgram {
    pub slot: Arc<FrameSlot>,
}

#[derive(Debug)]
pub struct CefPrimitive {
    pub slot: Arc<FrameSlot>,
}

impl<Msg> shader::Program<Msg> for CefProgram {
    type State = ();
    type Primitive = CefPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        CefPrimitive {
            slot: self.slot.clone(),
        }
    }
}

impl shader::Primitive for CefPrimitive {
    type Pipeline = CefPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &iced::widget::shader::Viewport,
    ) {
        let scale = viewport.scale_factor() as f32;
        let req_w = (bounds.width * scale).round().max(1.0) as u32;
        let req_h = (bounds.height * scale).round().max(1.0) as u32;
        let mut last = self.slot.last_size.lock().unwrap();
        if *last != (req_w, req_h) {
            *last = (req_w, req_h);
            drop(last);
            let _ = self.slot.releaser.send(Cmd::Resize {
                width: req_w,
                height: req_h,
            });
        }

        let mut guard = self.slot.pending.lock().unwrap();
        let Some(frame) = guard.take() else {
            return;
        };
        drop(guard);

        let need_new_texture = match &pipeline.current {
            Some(cur) => cur.size != (frame.width, frame.height),
            None => true,
        };
        if need_new_texture {
            let texture = cpu_import::create_texture(device, frame.width, frame.height);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            pipeline.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cef-shader bg"),
                layout: &pipeline.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                    },
                ],
            }));
            pipeline.current = Some(CurrentFrame {
                _uploaded: UploadedFrame { texture },
                size: (frame.width, frame.height),
            });
        }

        if let Some(cur) = pipeline.current.as_ref() {
            cpu_import::upload(queue, &cur._uploaded.texture, &frame);
        }

        // FPS counter — log every ~1s. Bench harness scrapes this
        // out of the log file.
        pipeline.fps_count += 1;
        let elapsed = pipeline.fps_window_start.elapsed();
        if elapsed >= std::time::Duration::from_secs(1) {
            let fps = pipeline.fps_count as f64 / elapsed.as_secs_f64();
            tracing::info!(fps = format!("{:.1}", fps), "shader fps");
            pipeline.fps_count = 0;
            pipeline.fps_window_start = std::time::Instant::now();
        }
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let Some(bg) = &pipeline.bind_group else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cef sample pass"),
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
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bg, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[derive(Debug)]
struct CurrentFrame {
    _uploaded: UploadedFrame,
    size: (u32, u32),
}

#[derive(Debug)]
pub struct CefPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,
    current: Option<CurrentFrame>,
    /// FPS counter — same instrumentation as sola-browser-wpe.
    fps_count: u64,
    fps_window_start: std::time::Instant,
}

impl shader::Pipeline for CefPipeline {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cef sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cef bgl"),
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
            label: Some("cef shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cef pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cef rp"),
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
            current: None,
            fps_count: 0,
            fps_window_start: std::time::Instant::now(),
        }
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
