//! CPU OSR paint helpers: dirty-rect copies, buffer reuse, BGRA detect cache.
//!
//! CEF's `on_paint` always hands us a full view buffer, but `dirty_rects`
//! still tells us what changed. Copying / swizzling / uploading only those
//! rects keeps the CEF UI thread and wgpu queue off the full 8–12 MiB
//! path for menus, carets, and other partial updates.

use serde::{Deserialize, Serialize};

/// One damage rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl DirtyRect {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            w: width,
            h: height,
        }
    }

    pub fn is_full(self, width: u32, height: u32) -> bool {
        self.x == 0 && self.y == 0 && self.w >= width && self.h >= height
    }

    pub fn pixel_bytes(self) -> usize {
        self.w as usize * self.h as usize * 4
    }
}

/// True when `rects` is empty or covers the whole view (treat as a full blit).
pub fn is_full_damage(rects: &[DirtyRect], width: u32, height: u32) -> bool {
    rects.is_empty() || rects.iter().any(|r| r.is_full(width, height))
}

/// Copy `src` (full `src_w × src_h` BGRA) into `dst` (same geometry).
/// `dst` is resized to `src_w * src_h * 4` when the size changes or on
/// a full-damage blit. Partial damage is applied in place.
pub fn apply_paint(dst: &mut Vec<u8>, src: &[u8], src_w: u32, src_h: u32, dirty: &[DirtyRect]) {
    let need = (src_w as usize)
        .saturating_mul(src_h as usize)
        .saturating_mul(4);
    let size_changed = dst.len() != need;
    if size_changed || is_full_damage(dirty, src_w, src_h) {
        dst.clear();
        if src.len() >= need {
            dst.extend_from_slice(&src[..need]);
        } else {
            dst.extend_from_slice(src);
            dst.resize(need, 0);
        }
        return;
    }
    for r in dirty {
        copy_rect(dst, src, src_w, src_h, *r);
    }
}

fn copy_rect(dst: &mut [u8], src: &[u8], src_w: u32, src_h: u32, r: DirtyRect) {
    if r.w == 0 || r.h == 0 {
        return;
    }
    let x = r.x.min(src_w);
    let y = r.y.min(src_h);
    let w = r.w.min(src_w.saturating_sub(x));
    let h = r.h.min(src_h.saturating_sub(y));
    if w == 0 || h == 0 {
        return;
    }
    let row_bytes = src_w as usize * 4;
    let copy_bytes = w as usize * 4;
    for row in 0..h as usize {
        let yy = y as usize + row;
        let off = yy * row_bytes + x as usize * 4;
        let end = off + copy_bytes;
        if end <= src.len() && end <= dst.len() {
            dst[off..end].copy_from_slice(&src[off..end]);
        }
    }
}

/// Swizzle only the damaged pixels. Format is detected once (ARGB vs BGRA)
/// so steady-state paints do not scan 512 samples every frame.
pub fn ensure_bgra_dirty(pixels: &mut [u8], width: u32, height: u32, dirty: &[DirtyRect]) {
    match cached_format() {
        PixelFormat::Bgra => return,
        PixelFormat::Unknown => {
            if looks_like_argb(pixels) {
                set_cached_format(PixelFormat::Argb);
            } else {
                set_cached_format(PixelFormat::Bgra);
                return;
            }
        }
        PixelFormat::Argb => {}
    }
    if is_full_damage(dirty, width, height) {
        swizzle_argb(pixels);
        return;
    }
    let row_bytes = width as usize * 4;
    for r in dirty {
        let x = r.x.min(width);
        let y = r.y.min(height);
        let w = r.w.min(width.saturating_sub(x));
        let h = r.h.min(height.saturating_sub(y));
        for row in 0..h as usize {
            let off = (y as usize + row) * row_bytes + x as usize * 4;
            let end = off + w as usize * 4;
            if end <= pixels.len() {
                swizzle_argb(&mut pixels[off..end]);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PixelFormat {
    Unknown = 0,
    Bgra = 1,
    Argb = 2,
}

static FORMAT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn cached_format() -> PixelFormat {
    match FORMAT.load(std::sync::atomic::Ordering::Relaxed) {
        1 => PixelFormat::Bgra,
        2 => PixelFormat::Argb,
        _ => PixelFormat::Unknown,
    }
}

fn set_cached_format(fmt: PixelFormat) {
    FORMAT.store(fmt as u8, std::sync::atomic::Ordering::Relaxed);
    if fmt == PixelFormat::Argb {
        static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("CEF on_paint is ARGB — swizzling to BGRA (avoids red wash)");
        }
    }
}

fn swizzle_argb(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        px.reverse();
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
    a_first > a_last.saturating_mul(2) && a_first > (n as u32 / 2)
}

/// Blit a `sw × sh` BGRA overlay onto `dst` (`dw × dh`) at `(dx, dy)`.
/// Clips to the destination. Used for CEF `PET_POPUP` (`<select>`).
pub fn blit_overlay(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    src: &[u8],
    sw: u32,
    sh: u32,
    dx: i32,
    dy: i32,
) {
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return;
    }
    let dst_x0 = dx.max(0) as u32;
    let dst_y0 = dy.max(0) as u32;
    if dst_x0 >= dw || dst_y0 >= dh {
        return;
    }
    let src_x0 = if dx < 0 { (-dx) as u32 } else { 0 };
    let src_y0 = if dy < 0 { (-dy) as u32 } else { 0 };
    let copy_w = sw.saturating_sub(src_x0).min(dw.saturating_sub(dst_x0));
    let copy_h = sh.saturating_sub(src_y0).min(dh.saturating_sub(dst_y0));
    if copy_w == 0 || copy_h == 0 {
        return;
    }
    let src_row = sw as usize * 4;
    let dst_row = dw as usize * 4;
    let copy_bytes = copy_w as usize * 4;
    for row in 0..copy_h as usize {
        let s_off = (src_y0 as usize + row) * src_row + src_x0 as usize * 4;
        let d_off = (dst_y0 as usize + row) * dst_row + dst_x0 as usize * 4;
        let s_end = s_off + copy_bytes;
        let d_end = d_off + copy_bytes;
        if s_end <= src.len() && d_end <= dst.len() {
            dst[d_off..d_end].copy_from_slice(&src[s_off..s_end]);
        }
    }
}

/// View-pixel box of a `PET_POPUP` after clipping to the view.
pub fn overlay_dirty(dx: i32, dy: i32, sw: u32, sh: u32, dw: u32, dh: u32) -> Option<DirtyRect> {
    let x = dx.max(0) as u32;
    let y = dy.max(0) as u32;
    if x >= dw || y >= dh || sw == 0 || sh == 0 {
        return None;
    }
    let src_x0 = if dx < 0 { (-dx) as u32 } else { 0 };
    let src_y0 = if dy < 0 { (-dy) as u32 } else { 0 };
    let w = sw.saturating_sub(src_x0).min(dw.saturating_sub(x));
    let h = sh.saturating_sub(src_y0).min(dh.saturating_sub(y));
    if w == 0 || h == 0 {
        None
    } else {
        Some(DirtyRect { x, y, w, h })
    }
}

/// Recycle an `Arc<Vec<u8>>` when we are the unique owner; otherwise allocate.
pub fn take_unique_pixels(prev: Option<std::sync::Arc<Vec<u8>>>, need: usize) -> Vec<u8> {
    if let Some(arc) = prev {
        if let Ok(mut v) = std::sync::Arc::try_unwrap(arc) {
            if v.len() != need {
                v.resize(need, 0);
            }
            return v;
        }
    }
    vec![0u8; need]
}

/// Triple-buffer of pixel `Arc`s. Latest-wins mailbox + last_frame hold at
/// most two refs; the third slot is free to unwrap and refill without
/// allocating an 8 MiB `Vec` on every paint.
pub struct PixelRing {
    slots: [Option<std::sync::Arc<Vec<u8>>>; 3],
    next: usize,
}

impl Default for PixelRing {
    fn default() -> Self {
        Self {
            slots: [None, None, None],
            next: 0,
        }
    }
}

impl PixelRing {
    pub fn take(&mut self, need: usize) -> Vec<u8> {
        for slot in &mut self.slots {
            if let Some(arc) = slot.take() {
                match std::sync::Arc::try_unwrap(arc) {
                    Ok(mut v) => {
                        if v.len() != need {
                            v.resize(need, 0);
                        }
                        return v;
                    }
                    Err(arc) => {
                        *slot = Some(arc);
                    }
                }
            }
        }
        vec![0u8; need]
    }

    pub fn publish(&mut self, pixels: Vec<u8>) -> std::sync::Arc<Vec<u8>> {
        let arc = std::sync::Arc::new(pixels);
        for slot in &mut self.slots {
            if slot.is_none() {
                *slot = Some(arc.clone());
                return arc;
            }
        }
        self.slots[self.next] = Some(arc.clone());
        self.next = (self.next + 1) % 3;
        arc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_damage_replaces() {
        let src = vec![1u8; 4 * 2 * 2];
        let mut dst = vec![9u8; 4];
        apply_paint(&mut dst, &src, 2, 2, &[]);
        assert_eq!(dst, src);
    }

    #[test]
    fn partial_damage_copies_one_pixel() {
        let mut src = vec![0u8; 4 * 2 * 2];
        src[0..4].copy_from_slice(&[1, 2, 3, 4]);
        src[4..8].copy_from_slice(&[5, 6, 7, 8]);
        let mut dst = vec![9u8; 4 * 2 * 2];
        apply_paint(
            &mut dst,
            &src,
            2,
            2,
            &[DirtyRect {
                x: 1,
                y: 0,
                w: 1,
                h: 1,
            }],
        );
        assert_eq!(&dst[4..8], &[5, 6, 7, 8]);
        assert_eq!(&dst[0..4], &[9, 9, 9, 9]);
    }

    #[test]
    fn take_unique_recycles() {
        let a = std::sync::Arc::new(vec![1u8; 8]);
        let v = take_unique_pixels(Some(a), 8);
        assert_eq!(v.len(), 8);
    }

    #[test]
    fn take_unique_allocates_when_shared() {
        let a = std::sync::Arc::new(vec![1u8; 8]);
        let _hold = a.clone();
        let v = take_unique_pixels(Some(a), 8);
        assert_eq!(v.len(), 8);
        assert_eq!(v[0], 0);
    }

    #[test]
    fn pixel_ring_recycles_unique_slot() {
        let mut ring = PixelRing::default();
        let a = ring.publish(vec![1u8; 16]);
        drop(a);
        let v = ring.take(16);
        assert_eq!(v.len(), 16);
        // Recycled buffer keeps previous bytes (then we overwrite in apply_paint).
        assert_eq!(v[0], 1);
    }

    #[test]
    fn overlay_blits_into_view() {
        // 2×2 dest, 1×1 src at (1, 0)
        let mut dst = vec![0u8; 16];
        let src = vec![9u8, 8, 7, 6];
        blit_overlay(&mut dst, 2, 2, &src, 1, 1, 1, 0);
        assert_eq!(&dst[4..8], &[9, 8, 7, 6]);
        assert_eq!(&dst[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn overlay_clips_negative_origin() {
        let mut dst = vec![0u8; 16];
        let src = vec![1, 1, 1, 1, 2, 2, 2, 2];
        blit_overlay(&mut dst, 2, 2, &src, 2, 1, -1, 0);
        // src col 1 lands at dest (0, 0)
        assert_eq!(&dst[0..4], &[2, 2, 2, 2]);
    }

    #[test]
    fn overlay_dirty_clips() {
        let d = overlay_dirty(-4, 2, 10, 8, 20, 20).unwrap();
        assert_eq!(
            d,
            DirtyRect {
                x: 0,
                y: 2,
                w: 6,
                h: 8
            }
        );
    }
}
