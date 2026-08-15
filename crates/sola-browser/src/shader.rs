//! Shared fullscreen-sample shader pipeline for browser frames.
//!
//! Imports a texture (dma-buf) and samples it with WGSL. The browser
//! window is transparent (float CSD); content must always write **opaque**
//! pixels into the webview rect or the desktop shows through.

use std::time::Instant;

use iced::Rectangle;

/// Result of importing one frame into a GPU texture the pipeline can sample.
pub struct ImportedTexture {
    pub bind_group: wgpu::BindGroup,
    /// Pixel size of the imported content.
    pub size: (u32, u32),
}

/// Engine-specific frame import (WPE: dma-buf → wgpu).
pub trait FrameImport {
    type Frame: Send + 'static;
    type Hold;

    fn import(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        frame: Self::Frame,
    ) -> Option<(ImportedTexture, Self::Hold)>;
}

/// Sola dark chrome (#0a0a0b) as BGRA bytes for a 1×1 fallback texel.
const FALLBACK_BGRA: [u8; 4] = [0x0b, 0x0a, 0x0a, 0xff];

/// Shared sample pipeline: WGSL fullscreen triangle + bind-group slots.
pub struct SamplePipeline {
    pipeline: wgpu::RenderPipeline,
    /// Blit import → owned texture (BGRA sRGB target).
    blit_pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    /// Live web content. `None` → use [`Self::fallback_bind_group`].
    pub bind_group: Option<wgpu::BindGroup>,
    pub frame_size: Option<(u32, u32)>,
    fallback_bind_group: wgpu::BindGroup,
    _fallback_texture: wgpu::Texture,
    fps_count: u64,
    fps_window_start: Instant,
}

impl std::fmt::Debug for SamplePipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SamplePipeline")
            .field("has_content", &self.bind_group.is_some())
            .field("frame_size", &self.frame_size)
            .finish_non_exhaustive()
    }
}

impl SamplePipeline {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> Self {
        // Prefer nearest for crisp UI text when content ≈ scissor (1:1 HiDPI).
        // Linear min softens whole pages into "blurry browser" on any scale.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(&format!("{label} sampler")),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

        let make_rp = |fmt: wgpu::TextureFormat, name: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
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
        let pipeline = make_rp(format, &format!("{label} rp"));
        let blit_pipeline = make_rp(
            wgpu::TextureFormat::Bgra8UnormSrgb,
            &format!("{label} blit-rp"),
        );

        let fallback_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("{label} fallback")),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &fallback_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &FALLBACK_BGRA,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let fallback_view = fallback_texture.create_view(&Default::default());
        let fallback_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{label} fallback bg")),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&fallback_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            pipeline,
            blit_pipeline,
            bind_group_layout,
            sampler,
            bind_group: None,
            frame_size: None,
            fallback_bind_group,
            _fallback_texture: fallback_texture,
            fps_count: 0,
            fps_window_start: Instant::now(),
        }
    }

    /// Copy dma-buf import into a GPU-owned texture and **wait** for the GPU.
    ///
    /// After this returns, the caller may free the import + release the WPE
    /// buffer immediately. Without `Maintain::Wait`, iced can sample a
    /// clear-black destination mid-blit (black flashes under scroll).
    pub fn blit_to_owned(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        src: &wgpu::Texture,
        size: (u32, u32),
    ) -> wgpu::Texture {
        let (w, h) = size;
        let dst = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("wpe-owned-frame"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let src_view = src.create_view(&wgpu::TextureViewDescriptor::default());
        let dst_view = dst.create_view(&wgpu::TextureViewDescriptor::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wpe blit bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("wpe blit enc"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wpe blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Fullscreen triangle overwrites every pixel.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.039,
                            g: 0.039,
                            b: 0.043,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.draw(0..3, 0..1);
        }
        let _idx = queue.submit(Some(encoder.finish()));
        // Block until blit is done so we never sample a half-cleared texture
        // and can release the WPE dma-buf without pool pin / ignore deadlock.
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        dst
    }

    pub fn install(&mut self, imported: ImportedTexture) {
        self.bind_group = Some(imported.bind_group);
        self.frame_size = Some(imported.size);
    }

    pub fn install_bind_group(&mut self, bind_group: wgpu::BindGroup, size: (u32, u32)) {
        self.bind_group = Some(bind_group);
        self.frame_size = Some(size);
    }

    /// Drop live content; next render uses the opaque dark fallback.
    pub fn clear(&mut self) {
        self.bind_group = None;
        self.frame_size = None;
    }

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

    /// Always draw into the content scissor: live frame or dark fallback.
    /// Never leave the rect empty on a transparent window.
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
        last_requested_size: (u32, u32),
        pass_label: &str,
    ) {
        // A parked last-frame at spawn size (1280×800) stretched into the
        // live widget looks like a half-width, overly-tall page. Keep the
        // last good texture only when it matches the widget.
        let size_ok = self
            .frame_size
            .is_some_and(|sz| size_matches(sz, last_requested_size));
        let bg = if size_ok {
            self.bind_group
                .as_ref()
                .unwrap_or(&self.fallback_bind_group)
        } else {
            &self.fallback_bind_group
        };

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(pass_label),
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

/// Widget vs frame size, with 1px slack for HiDPI rounding.
pub fn size_matches(a: (u32, u32), b: (u32, u32)) -> bool {
    a.0.abs_diff(b.0) <= 1 && a.1.abs_diff(b.1) <= 1
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
    // Force opaque: transparent window + REPLACE with α=0 from WebKit
    // (blank/loading frames) punches a hole through to the desktop.
    let c = textureSample(tex, samp, in.uv);
    return vec4(c.rgb, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::size_matches;

    #[test]
    fn size_matches_exact_and_one_px() {
        assert!(size_matches((1920, 1080), (1920, 1080)));
        assert!(size_matches((1920, 1080), (1921, 1079)));
        assert!(!size_matches((1280, 800), (2560, 800)));
        assert!(!size_matches((1280, 800), (2560, 1600)));
    }
}
