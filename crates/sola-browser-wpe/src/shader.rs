//! iced `shader::Program` that samples the currently-imported WPE
//! frame as a fullscreen quad.
//!
//! Ownership flow per frame:
//! 1. WPE worker thread emits a `WpeFrame` (FD + metadata + token).
//! 2. App's subscription receives it, stashes in `slot.pending`,
//!    requests an iced redraw.
//! 3. Next render cycle: `Primitive::prepare` runs on iced's render
//!    thread. It takes the pending frame, imports as a wgpu texture
//!    (via `wgpu_import::import`), swaps in the bind group, and
//!    sends a `Cmd::Release` back to the WPE worker for the
//!    previously-displayed frame's token so WPE can recycle that
//!    buffer.
//! 4. `Primitive::render` issues the fullscreen-triangle draw call.

use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;

use iced::widget::shader;
use iced::{Rectangle, mouse};

use crate::wgpu_import::{self, DmabufMetadata, ImportedFrame};
use crate::wpe::{Cmd, ResourceToken, WpeFrame};

/// Shared between the App (which fills `pending`) and the shader
/// Pipeline (which drains it on next prepare). The `releaser`
/// channel goes back to the WPE worker thread so we can hand
/// recycled buffer-resource tokens back when a new frame replaces
/// an old one — and so the shader Program can request resizes when
/// the iced widget bounds change.
pub struct FrameSlot {
    pub pending: Mutex<Option<WpeFrame>>,
    pub releaser: Sender<Cmd>,
    /// Last size we asked WPE to render at (physical pixels). Used
    /// to debounce resize commands so we only fire on actual change.
    pub last_size: Mutex<(u32, u32)>,
}

#[derive(Debug)]
pub struct WpeProgram {
    pub slot: Arc<FrameSlot>,
}

impl std::fmt::Debug for FrameSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameSlot").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct WpePrimitive {
    pub slot: Arc<FrameSlot>,
}

impl<Msg> shader::Program<Msg> for WpeProgram {
    type State = ();
    type Primitive = WpePrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        WpePrimitive {
            slot: self.slot.clone(),
        }
    }
}

impl shader::Primitive for WpePrimitive {
    type Pipeline = WpePipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &iced::widget::shader::Viewport,
    ) {
        // Mirror the iced widget's physical size to WPE so the
        // WebProcess re-lays out at the actual viewport size
        // instead of the headless default (1024x768). Runs on
        // every prepare but only sends a Cmd when the size
        // actually changes.
        let scale = viewport.scale_factor() as f32;
        let req_w = (bounds.width * scale).round().max(1.0) as u32;
        let req_h = (bounds.height * scale).round().max(1.0) as u32;
        let mut last = self.slot.last_size.lock().unwrap();
        if *last != (req_w, req_h) {
            *last = (req_w, req_h);
            drop(last);
            let _ = self
                .slot
                .releaser
                .send(Cmd::Resize {
                    width: req_w,
                    height: req_h,
                });
        }

        let mut guard = self.slot.pending.lock().unwrap();
        let Some(frame) = guard.take() else {
            return;
        };
        drop(guard);

        tracing::trace!(
            w = frame.width,
            h = frame.height,
            stride = frame.stride,
            "shader::prepare: importing new WPE frame",
        );

        let new_token = frame.token;
        let meta = DmabufMetadata {
            width: frame.width,
            height: frame.height,
            format: frame.format,
            modifier: frame.modifier,
            stride: frame.stride,
            offset: frame.offset,
        };
        let imported = match unsafe { wgpu_import::import(device, frame.fd, &meta) } {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("wgpu_import::import failed: {e}");
                // Don't send Release for `new_token` either — WPE
                // already considers the buffer in flight; releasing
                // a buffer we never "consumed" would confuse it.
                return;
            }
        };

        let view = imported
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        pipeline.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wpe-shader bg"),
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

        // Release the previous frame's buffer back to WPE so it can
        // recycle it. Order matters — `pipeline.current` Drop runs
        // *after* this swap, so the previous wgpu::Texture stays
        // alive until we've already told WPE the underlying buffer
        // is free. That's safe because wgpu's texture wraps the
        // *imported* memory, not the producer's memory; the producer
        // (WPE) reuses its own buffer pool independently.
        if let Some(prev) = pipeline.current.take() {
            let _ = self.slot.releaser.send(Cmd::Release { token: prev.token });
        }
        pipeline.current = Some(CurrentFrame {
            _imported: imported,
            token: new_token,
        });
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
            label: Some("wpe sample pass"),
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
    _imported: ImportedFrame,
    token: ResourceToken,
}

#[derive(Debug)]
pub struct WpePipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,
    current: Option<CurrentFrame>,
}

impl shader::Pipeline for WpePipeline {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("wpe sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wpe bgl"),
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
            label: Some("wpe shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wpe pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wpe rp"),
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
