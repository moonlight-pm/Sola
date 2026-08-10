//! CPU conversion for multi-plane YUV dma-bufs (YouTube / media).
//!
//! Full multi-plane GPU import (NV12 sampling) is larger work. For dogfood we
//! mmap the planes, convert to BGRA, upload as a normal texture, then release
//! the WPE buffer immediately so the pool does not stall.

/// DRM fourcc `NV12` (`'NV12'`).
pub const DRM_FORMAT_NV12: u32 = 0x3231_564e;
/// DRM fourcc `NV21` (`'NV21'`).
pub const DRM_FORMAT_NV21: u32 = 0x3132_564e;

/// Convert NV12 or NV21 (2 planes: Y full, UV half) to tightly packed BGRA8.
///
/// `y` / `uv` are mmap'd plane bases; strides may exceed width.
pub fn nv12_family_to_bgra(
    y: &[u8],
    y_stride: u32,
    uv: &[u8],
    uv_stride: u32,
    width: u32,
    height: u32,
    nv21: bool,
) -> Option<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    let y_stride = y_stride as usize;
    let uv_stride = uv_stride as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let y_need = y_stride.checked_mul(h)?;
    let uv_need = uv_stride.checked_mul((h + 1) / 2)?;
    if y.len() < y_need || uv.len() < uv_need {
        return None;
    }

    let mut out = vec![0u8; w * h * 4];
    for row in 0..h {
        let y_row = &y[row * y_stride..row * y_stride + w];
        let uv_row = &uv[(row / 2) * uv_stride..(row / 2) * uv_stride + (w & !1).max(2).min(uv_stride)];
        for col in 0..w {
            let yi = y_row[col] as i32;
            let uv_i = (col / 2) * 2;
            if uv_i + 1 >= uv_row.len() {
                continue;
            }
            let (u, v) = if nv21 {
                (uv_row[uv_i + 1] as i32, uv_row[uv_i] as i32)
            } else {
                (uv_row[uv_i] as i32, uv_row[uv_i + 1] as i32)
            };
            // BT.601 limited range → RGB
            let c = yi - 16;
            let d = u - 128;
            let e = v - 128;
            let r = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8;
            let g = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
            let b = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8;
            let o = (row * w + col) * 4;
            out[o] = b;
            out[o + 1] = g;
            out[o + 2] = r;
            out[o + 3] = 255;
        }
    }
    Some(out)
}

/// mmap a dmabuf plane for reading. Returns (ptr, length) mapped region.
///
/// Caller must `munmap`. Length is a lower bound from stride×height (or
/// stride×height/2 for chroma); we map at least that many bytes + offset.
pub unsafe fn mmap_plane(
    fd: i32,
    offset: u32,
    length: usize,
) -> Option<(*mut libc::c_void, usize)> {
    let map_len = offset as usize + length;
    if map_len == 0 || fd < 0 {
        return None;
    }
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return None;
    }
    Some((ptr, map_len))
}

pub unsafe fn munmap_plane(ptr: *mut libc::c_void, len: usize) {
    if !ptr.is_null() && ptr != libc::MAP_FAILED {
        unsafe {
            libc::munmap(ptr, len);
        }
    }
}
