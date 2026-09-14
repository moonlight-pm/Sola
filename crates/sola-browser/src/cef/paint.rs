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

const MAX_DIRTY_RECTS: usize = 8;

/// Bounding box of `rects`. None if empty.
pub fn union_aabb(rects: &[DirtyRect]) -> Option<DirtyRect> {
    let mut iter = rects.iter();
    let first = *iter.next()?;
    let mut x0 = first.x;
    let mut y0 = first.y;
    let mut x1 = first.x.saturating_add(first.w);
    let mut y1 = first.y.saturating_add(first.h);
    for r in iter {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.x.saturating_add(r.w));
        y1 = y1.max(r.y.saturating_add(r.h));
    }
    Some(DirtyRect {
        x: x0,
        y: y0,
        w: x1.saturating_sub(x0),
        h: y1.saturating_sub(y0),
    })
}

/// Merge `extra` into `into` for an accumulator.
///
/// Empty `extra` is full damage (wire protocol) and absorbs. Empty `into`
/// is **no damage yet** — callers that mean "already full" must use
/// [`merge_wire_damage`]. Too many rects collapse to an AABB; a huge AABB
/// becomes full (empty vec).
pub fn merge_damage(into: &mut Vec<DirtyRect>, extra: &[DirtyRect], w: u32, h: u32) {
    if is_full_damage(extra, w, h) {
        into.clear();
        return;
    }
    if into.iter().any(|r| r.is_full(w, h)) {
        into.clear();
        return;
    }
    into.extend_from_slice(extra);
    if into.len() <= MAX_DIRTY_RECTS {
        return;
    }
    let Some(u) = union_aabb(into) else {
        return;
    };
    into.clear();
    let area = u64::from(u.w).saturating_mul(u64::from(u.h));
    let view = u64::from(w).saturating_mul(u64::from(h));
    if u.is_full(w, h) || (view > 0 && area > view / 2) {
        return;
    }
    into.push(u);
}

/// Merge using the frame wire protocol: empty `into` **or** `extra` is full.
pub fn merge_wire_damage(into: &mut Vec<DirtyRect>, extra: &[DirtyRect], w: u32, h: u32) {
    if is_full_damage(into, w, h) {
        into.clear();
        return;
    }
    merge_damage(into, extra, w, h);
}

/// Pending damage that is not yet on a published frame.
/// `None` = nothing. `Some([])` = full. `Some(rects)` = partial.
pub fn absorb_pending(pending: &mut Option<Vec<DirtyRect>>, extra: &[DirtyRect], w: u32, h: u32) {
    match pending {
        None => {
            *pending = Some(if is_full_damage(extra, w, h) {
                Vec::new()
            } else {
                extra.to_vec()
            });
        }
        Some(into) => merge_wire_damage(into, extra, w, h),
    }
}

/// Copy `src` (full `src_w × src_h` BGRA) into `dst` (same geometry).
///
/// Partial damage is applied in place **only** when `dst_complete` is set
/// (the buffer is the previous full frame at this size). A fresh or
/// zeroed `dst` always takes a full blit — otherwise holes stay until a
/// later hover damage happens to cover them.
pub fn apply_paint(
    dst: &mut Vec<u8>,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dirty: &[DirtyRect],
    dst_complete: bool,
) {
    let need = (src_w as usize)
        .saturating_mul(src_h as usize)
        .saturating_mul(4);
    let size_changed = dst.len() != need;
    if size_changed || !dst_complete || is_full_damage(dirty, src_w, src_h) {
        dst.clear();
        if src.len() >= need {
            dst.extend_from_slice(&src[..need]);
        } else {
            dst.extend_from_slice(src);
            dst.resize(need, 0);
        }
        return;
    }
    copy_rects(dst, src, src_w, src_h, dirty);
}

/// Copy each rect from a full `src` buffer into `dst` (same stride).
pub fn copy_rects(dst: &mut [u8], src: &[u8], src_w: u32, src_h: u32, rects: &[DirtyRect]) {
    for r in rects {
        copy_rect(dst, src, src_w, src_h, *r);
    }
}

pub fn copy_rect(dst: &mut [u8], src: &[u8], src_w: u32, src_h: u32, r: DirtyRect) {
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

/// Triple-buffer of pixel `Arc`s for recycle.
///
/// Overlay publish keeps the latest slot so a mailbox drop can unique-unwrap
/// an older buffer. View `last_frame` must **not** [`publish`] the current
/// Arc — that pins latest at ≥2 refs and makes `try_unwrap` fail every paint.
/// Stash only shared (mailbox-held) buffers; unique last_frame is the dest.
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
        self.take_recycled(need).0
    }

    /// Same as [`take`], plus whether `pixels` is a same-size unique recycle
    /// (safe to apply partial dirty). A resize or fresh alloc is `false`.
    pub fn take_recycled(&mut self, need: usize) -> (Vec<u8>, bool) {
        for slot in &mut self.slots {
            if let Some(arc) = slot.take() {
                match std::sync::Arc::try_unwrap(arc) {
                    Ok(mut v) => {
                        if v.len() == need {
                            return (v, true);
                        }
                        v.resize(need, 0);
                        return (v, false);
                    }
                    Err(arc) => {
                        *slot = Some(arc);
                    }
                }
            }
        }
        (vec![0u8; need], false)
    }

    /// Keep `arc` for a later unique unwrap. Does not clone.
    pub fn stash(&mut self, arc: std::sync::Arc<Vec<u8>>) {
        for slot in &mut self.slots {
            if slot.is_none() {
                *slot = Some(arc);
                return;
            }
        }
        self.slots[self.next] = Some(arc);
        self.next = (self.next + 1) % 3;
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
        apply_paint(&mut dst, &src, 2, 2, &[], true);
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
            true,
        );
        assert_eq!(&dst[4..8], &[5, 6, 7, 8]);
        assert_eq!(&dst[0..4], &[9, 9, 9, 9]);
    }

    #[test]
    fn incomplete_dst_partial_dirty_copies_full_src() {
        let mut src = vec![0u8; 16];
        src[0..4].copy_from_slice(&[1, 2, 3, 4]);
        src[4..8].copy_from_slice(&[5, 6, 7, 8]);
        let mut dst = vec![9u8; 16];
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
            false,
        );
        assert_eq!(dst, src);
    }

    #[test]
    fn ghost_union_is_not_full_damage() {
        let a = DirtyRect {
            x: 10,
            y: 20,
            w: 40,
            h: 12,
        };
        let b = DirtyRect {
            x: 80,
            y: 90,
            w: 40,
            h: 12,
        };
        assert!(!is_full_damage(&[a, b], 5120, 2000));
        assert!(!is_full_damage(&[a], 5120, 2000));
    }

    #[test]
    fn merge_damage_full_absorbs() {
        let mut d = vec![DirtyRect {
            x: 1,
            y: 1,
            w: 2,
            h: 2,
        }];
        merge_damage(&mut d, &[], 10, 10);
        assert!(d.is_empty());
    }

    #[test]
    fn merge_wire_empty_into_stays_full() {
        let mut d = Vec::new();
        merge_wire_damage(
            &mut d,
            &[DirtyRect {
                x: 1,
                y: 1,
                w: 2,
                h: 2,
            }],
            10,
            10,
        );
        assert!(d.is_empty());
    }

    #[test]
    fn absorb_pending_none_then_partial() {
        let mut p = None;
        absorb_pending(
            &mut p,
            &[DirtyRect {
                x: 2,
                y: 0,
                w: 1,
                h: 1,
            }],
            10,
            10,
        );
        let r = p.as_ref().unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].x, 2);
    }

    #[test]
    fn merge_damage_unions_many_to_aabb() {
        let mut d = Vec::new();
        for i in 0..10u32 {
            merge_damage(
                &mut d,
                &[DirtyRect {
                    x: i,
                    y: 0,
                    w: 1,
                    h: 1,
                }],
                100,
                100,
            );
        }
        assert!(d.len() <= MAX_DIRTY_RECTS);
        let u = union_aabb(&d).unwrap();
        assert_eq!(u.x, 0);
        assert_eq!(u.w, 10);
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
    fn pixel_ring_recycles_older_while_latest_shared() {
        let mut ring = PixelRing::default();
        let a = ring.publish(vec![1u8; 16]);
        drop(a);
        let b = ring.publish(vec![2u8; 16]);
        let _hold = b.clone();
        let (v, recycled) = ring.take_recycled(16);
        assert!(recycled);
        assert_eq!(v[0], 1);
    }

    #[test]
    fn stash_shared_recycles_after_drop() {
        let mut ring = PixelRing::default();
        let a = std::sync::Arc::new(vec![1u8; 16]);
        let hold = a.clone();
        ring.stash(a);
        let (v, recycled) = ring.take_recycled(16);
        assert!(!recycled);
        assert_eq!(v[0], 0);
        drop(hold);
        let (v2, recycled2) = ring.take_recycled(16);
        assert!(recycled2);
        assert_eq!(v2[0], 1);
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
