//! btop-style dithered pixel graph for menubar stats.
//!
//! A fixed LED matrix (dot + gutter) replaces variable-width numbers so
//! the cluster cannot reflow. Each column is one recent sample. A cell is
//! a 2×2 Bayer dither — the same five fill levels btop packs into braille.
//!
//! Painted as a nearest-neighbor RGBA image, not iced `canvas` 1×1
//! rectangles. Tiny fill_rectangle tessellation drops on GLES2 / software
//! GL (Oath metal, virtio llvmpipe); a 35×14 texture does not.

use iced::widget::image::{self, FilterMethod};
use iced::{Color, ContentFit, Element, Length, Theme};

pub const COLS: usize = 12;
pub const ROWS: usize = 5;
pub const DOT: f32 = 2.0;
pub const GAP: f32 = 1.0;

pub const GRAPH_W: f32 = COLS as f32 * DOT + (COLS - 1) as f32 * GAP;
pub const GRAPH_H: f32 = ROWS as f32 * DOT + (ROWS - 1) as f32 * GAP;

/// Ordered 2×2 Bayer thresholds in (0, 1).
const BAYER: [[f32; 2]; 2] = [[0.125, 0.625], [0.875, 0.375]];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// 0–100: per-column warn / crit.
    Level,
    Rx,
    Tx,
}

pub fn graph<'a, Message: 'a>(
    samples: Vec<f32>,
    max: f32,
    tint: Tint,
    theme: &Theme,
) -> Element<'a, Message> {
    let samples = last_n(&samples, COLS);
    let (w, h) = graph_px();
    let pixels = raster_stats(&samples, max.max(1.0), tint, theme);
    image::Image::new(image::Handle::from_rgba(w, h, pixels))
        .filter_method(FilterMethod::Nearest)
        .content_fit(ContentFit::Fill)
        .width(Length::Fixed(GRAPH_W))
        .height(Length::Fixed(GRAPH_H))
        .into()
}

pub fn graph_px() -> (u32, u32) {
    (GRAPH_W.round() as u32, GRAPH_H.round() as u32)
}

/// Oldest→newest, length `n`. Short windows pad on the left (btop scroll-on).
pub fn last_n(samples: &[f32], n: usize) -> Vec<f32> {
    if samples.len() >= n {
        samples[samples.len() - n..].to_vec()
    } else {
        let mut out = vec![0.0; n - samples.len()];
        out.extend_from_slice(samples);
        out
    }
}

pub fn bayer2(x: u32, y: u32) -> f32 {
    BAYER[(y % 2) as usize][(x % 2) as usize]
}

/// How much of cell `from_bottom` (0 = bottom row) is filled for a 0..1 column.
pub fn cell_fill(column: f32, from_bottom: usize) -> f32 {
    let height_cells = column.clamp(0.0, 1.0) * ROWS as f32;
    (height_cells - from_bottom as f32).clamp(0.0, 1.0)
}

pub fn cell_pixel_on(cell_fill: f32, dx: u32, dy: u32) -> bool {
    cell_fill >= 1.0 || (cell_fill > 0.0 && cell_fill > bayer2(dx, dy))
}

pub fn unlit_color(theme: &Theme) -> Color {
    let p = theme.extended_palette();
    Color {
        a: 0.16,
        ..p.background.base.text
    }
}

pub fn put_rgba(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, c: Color) {
    if x < 0 || y < 0 {
        return;
    }
    let x = x as u32;
    let y = y as u32;
    if x >= w || y >= h {
        return;
    }
    let i = ((y * w + x) * 4) as usize;
    if i + 3 >= buf.len() {
        return;
    }
    buf[i] = (c.r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    buf[i + 1] = (c.g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    buf[i + 2] = (c.b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    buf[i + 3] = (c.a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
}

/// One 2×2 LED cell at (`x0`, `y0`).
pub fn paint_cell(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x0: f32,
    y0: f32,
    cf: f32,
    lit: Color,
    unlit: Color,
) {
    let x0 = x0.round() as i32;
    let y0 = y0.round() as i32;
    for dy in 0..2i32 {
        for dx in 0..2i32 {
            let on = cell_pixel_on(cf, dx as u32, dy as u32);
            put_rgba(buf, w, h, x0 + dx, y0 + dy, if on { lit } else { unlit });
        }
    }
}

fn raster_stats(samples: &[f32], max: f32, tint: Tint, theme: &Theme) -> Vec<u8> {
    let (w, h) = graph_px();
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let p = theme.extended_palette();
    let unlit = unlit_color(theme);
    let pitch = DOT + GAP;
    let n = samples.len().min(COLS);
    for col in 0..COLS {
        let sample = if col < n { samples[col] } else { 0.0 };
        let fill = (sample / max).clamp(0.0, 1.0);
        let lit = match tint {
            Tint::Level => crate::stats::level_color(sample, p.background.base.text),
            Tint::Rx => p.primary.base.color,
            Tint::Tx => p.success.base.color,
        };
        let x0 = col as f32 * pitch;
        for row in 0..ROWS {
            let from_bottom = ROWS - 1 - row;
            let cf = cell_fill(fill, from_bottom);
            let y0 = row as f32 * pitch;
            paint_cell(&mut buf, w, h, x0, y0, cf, lit, unlit);
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_n_pads_left() {
        assert_eq!(last_n(&[1.0, 2.0], 4), vec![0.0, 0.0, 1.0, 2.0]);
        assert_eq!(last_n(&[1.0, 2.0, 3.0, 4.0, 5.0], 3), vec![3.0, 4.0, 5.0]);
    }

    #[test]
    fn graph_size_is_fixed() {
        assert_eq!(GRAPH_W, 12.0 * 2.0 + 11.0);
        assert_eq!(GRAPH_H, 5.0 * 2.0 + 4.0);
        let (w, h) = graph_px();
        assert_eq!(w, 35);
        assert_eq!(h, 14);
    }

    #[test]
    fn half_column_fills_bottom_two_and_dithers_third() {
        // 0.5 * 5 = 2.5 cells from the bottom.
        assert_eq!(cell_fill(0.5, 0), 1.0);
        assert_eq!(cell_fill(0.5, 1), 1.0);
        assert!((cell_fill(0.5, 2) - 0.5).abs() < f32::EPSILON);
        assert_eq!(cell_fill(0.5, 3), 0.0);
        assert_eq!(cell_fill(0.5, 4), 0.0);
        // 0.5 fill lights the two Bayer taps below 0.5 (0.125 and 0.375).
        let lit: usize = (0..2)
            .flat_map(|dy| (0..2).map(move |dx| cell_pixel_on(0.5, dx, dy)))
            .filter(|on| *on)
            .count();
        assert_eq!(lit, 2);
    }

    #[test]
    fn empty_column_is_dark() {
        for dy in 0..2 {
            for dx in 0..2 {
                assert!(!cell_pixel_on(0.0, dx, dy));
            }
        }
    }

    #[test]
    fn warn_threshold_unchanged() {
        assert!(crate::stats::WARN_PCT < crate::stats::CRIT_PCT);
    }

    #[test]
    fn raster_lights_pixels_for_a_full_column() {
        let theme = Theme::Dark;
        let pixels = raster_stats(&[100.0; COLS], 100.0, Tint::Level, &theme);
        assert_eq!(pixels.len(), 35 * 14 * 4);
        let lit = pixels.chunks(4).filter(|px| px[3] > 200).count();
        assert!(lit > 80, "expected a full matrix of opaque LEDs, got {lit}");
    }

    #[test]
    fn raster_empty_still_paints_unlit_dots() {
        let theme = Theme::Dark;
        let pixels = raster_stats(&[0.0; COLS], 100.0, Tint::Level, &theme);
        let dim = pixels.chunks(4).filter(|px| px[3] > 0 && px[3] < 80).count();
        assert!(dim > 80, "unlit LEDs should still be visible, got {dim}");
    }
}
