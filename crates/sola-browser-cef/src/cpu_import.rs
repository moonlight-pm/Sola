//! CPU-buffer → `wgpu::Texture` upload for CEF's `on_paint` frames.
//!
//! Different shape from the WPE crate's `wgpu_import.rs`: there is
//! no DMA-BUF FD here, no Vulkan modifier dance — just
//! `queue.write_texture` with the BGRA bytes CEF gave us.
//!
//! The texture is owned by the shader Pipeline and recreated only
//! when the frame dimensions change. Steady-state path is a single
//! write_texture per frame.

use crate::engine::CefFrame;

/// Public handle returned by `upload`. The texture lives at the
/// caller-managed size; we never recreate inside `upload` — the
/// caller checks dimensions and rebuilds before calling.
pub struct UploadedFrame {
    pub texture: wgpu::Texture,
}

impl std::fmt::Debug for UploadedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UploadedFrame").finish_non_exhaustive()
    }
}

/// Allocate the destination texture. `format` is `Bgra8UnormSrgb`
/// so the GPU sRGB-decodes on sample and re-encodes on the swap-
/// chain write — same reasoning as the WPE crate's sRGB import.
pub fn create_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cef-frame"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Copy `frame.pixels` into `texture` via the queue. Texture must
/// already be sized to `(frame.width, frame.height)`.
pub fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture, frame: &CefFrame) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        frame.pixels.as_slice(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(frame.width * 4),
            rows_per_image: Some(frame.height),
        },
        wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        },
    );
}
