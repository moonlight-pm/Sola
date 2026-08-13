//! CPU-buffer → `wgpu::Texture` upload for CEF's `on_paint` frames.
//!
//! Different shape from the WPE crate's `wgpu_import.rs`: there is
//! no DMA-BUF FD here, no Vulkan modifier dance — just
//! `queue.write_texture` with the BGRA bytes CEF gave us.
//!
//! The texture is owned by the shader Pipeline and recreated only
//! when the frame dimensions change. Steady-state path is a single
//! write_texture per frame.

use crate::cef::engine::CefFrame;

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

/// wgpu `write_texture` row pitch must be a multiple of 256.
const COPY_ALIGN: u32 = 256;

/// Copy `frame.pixels` into `texture` via the queue. Texture must
/// already be sized to `(frame.width, frame.height)`.
///
/// `staging` is reused for 256-byte row padding so we do not allocate
/// a second full frame every paint. Dirty rects upload only the damage
/// when the texture already holds the previous composite (`full` = false).
pub fn upload(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    frame: &CefFrame,
    staging: &mut Vec<u8>,
    force_full: bool,
) {
    let dirty = if force_full || crate::cef::paint::is_full_damage(&frame.dirty, frame.width, frame.height)
    {
        None
    } else {
        Some(frame.dirty.as_slice())
    };
    match dirty {
        None => upload_rect(
            queue,
            texture,
            frame.pixels.as_slice(),
            frame.width,
            0,
            0,
            frame.width,
            frame.height,
            staging,
        ),
        Some(rects) => {
            for r in rects {
                upload_rect(
                    queue,
                    texture,
                    frame.pixels.as_slice(),
                    frame.width,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    staging,
                );
            }
        }
    }
}

fn upload_rect(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    src: &[u8],
    src_w: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    staging: &mut Vec<u8>,
) {
    if w == 0 || h == 0 {
        return;
    }
    let unpadded = w.saturating_mul(4);
    let padded = unpadded.div_ceil(COPY_ALIGN).max(1) * COPY_ALIGN;
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(padded),
        rows_per_image: Some(h),
    };
    let extent = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let dest = wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d { x, y, z: 0 },
        aspect: wgpu::TextureAspect::All,
    };
    let src_row = src_w as usize * 4;
    let copy_bytes = unpadded as usize;
    if padded == unpadded && x == 0 && w == src_w {
        let start = y as usize * src_row;
        let end = start + h as usize * src_row;
        if end <= src.len() {
            queue.write_texture(dest, &src[start..end], layout, extent);
            return;
        }
    }
    let need = padded as usize * h as usize;
    if staging.len() < need {
        staging.resize(need, 0);
    }
    for row in 0..h as usize {
        let s = (y as usize + row) * src_row + x as usize * 4;
        let d = row * padded as usize;
        if s + copy_bytes <= src.len() && d + copy_bytes <= staging.len() {
            staging[d..d + copy_bytes].copy_from_slice(&src[s..s + copy_bytes]);
        }
    }
    queue.write_texture(dest, &staging[..need], layout, extent);
}

/// Headless / software CEF sometimes delivers ARGB (A first) instead of
/// the documented BGRA. Uploading that as BGRA makes every opaque pixel
/// show up red (A=255 lands in R). Detect and swizzle in place.
pub fn ensure_bgra(pixels: &mut [u8]) {
    if pixels.len() < 16 {
        return;
    }
    if !looks_like_argb(pixels) {
        return;
    }
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::info!(
            sample = ?&pixels[..4],
            "CEF on_paint is ARGB — swizzling to BGRA (avoids red wash)"
        );
    }
    for px in pixels.chunks_exact_mut(4) {
        px.reverse(); // ARGB → BGRA
    }
}

fn looks_like_argb(pixels: &[u8]) -> bool {
    let mut a_first = 0u32;
    let mut a_last = 0u32;
    let n = (pixels.len() / 4).min(512);
    if n < 16 {
        return false;
    }
    for i in 0..n {
        let p = &pixels[i * 4..];
        if p[0] == 255 && p[3] != 255 {
            a_first += 1;
        }
        if p[3] == 255 && p[0] != 255 {
            a_last += 1;
        }
    }
    // Strong majority of "alpha-only-in-byte-0" → ARGB.
    a_first > a_last.saturating_mul(2) && a_first > (n as u32 / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_argb_dark_page() {
        // Opaque dark gray stored as ARGB — the red-wash case.
        let mut buf = Vec::new();
        for _ in 0..64 {
            buf.extend_from_slice(&[255, 15, 15, 15]);
        }
        assert!(looks_like_argb(&buf));
        ensure_bgra(&mut buf);
        assert_eq!(&buf[0..4], &[15, 15, 15, 255]);
    }

    #[test]
    fn leaves_bgra_alone() {
        let mut buf = Vec::new();
        for _ in 0..64 {
            buf.extend_from_slice(&[15, 15, 15, 255]);
        }
        assert!(!looks_like_argb(&buf));
        ensure_bgra(&mut buf);
        assert_eq!(&buf[0..4], &[15, 15, 15, 255]);
    }
}
