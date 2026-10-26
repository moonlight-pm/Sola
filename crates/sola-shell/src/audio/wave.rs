//! Menubar spectrum analyzer — 12 treble-biased frequency bars in the same
//! whole-dot LED language as the stats graphs. Height is band amplitude
//! (graphic EQ / old-stereo analyzer), not a scrolling waveform.
//!
//! Rasterized to a nearest-neighbor RGBA image (same reason as
//! [`crate::stats::pixel`]: iced canvas 1×1 rectangles drop on GLES2).
//! The matrix's ends are round — a stadium as tall as the matrix — so in
//! its island, inset the same all round, they run with the pill's ends.

use iced::widget::image::{self, FilterMethod};
use iced::{Color, ContentFit, Element, Length, Theme};

use super::meter::{self, BANDS};
use crate::stats::pixel::{GRAPH_H, ISLAND, ROWS, lit, paint_dot, unlit_color};

/// LED columns that move together as one frequency bar.
const BAR_COLS: usize = 3;
/// Extra gutter between bars so they read as 12 meters, not one matrix.
const BAND_GAP: u32 = 2;

const BAR_W: u32 = BAR_COLS as u32 * ISLAND.pitch - (ISLAND.pitch - ISLAND.dot);

/// ~3× the stats graph: 12 bars × 3 LED columns + band gutters.
pub const SPECTRUM_W: f32 = (BANDS as u32 * BAR_W + (BANDS as u32 - 1) * BAND_GAP) as f32;
pub const SPECTRUM_H: f32 = GRAPH_H;

/// Phosphor stack, bottom → top. Same vertical language on every bar
/// (green base, cyan, violet, amber peak) — the old-stereo LED analyzer,
/// not a hue-per-band rainbow.
const ROW_RGB: [[f32; 3]; ROWS] = [
    [0.22, 0.98, 0.42], // green
    [0.12, 0.92, 0.88], // teal
    [0.38, 0.52, 1.00], // azure
    [0.78, 0.32, 1.00], // violet
    [1.00, 0.58, 0.18], // amber peak
];

pub fn visualizer<'a, Message: 'a>(muted: bool, theme: &Theme) -> Element<'a, Message> {
    let (w, h) = spectrum_px();
    let pixels = raster(muted, theme);
    image::Image::new(image::Handle::from_rgba(w, h, pixels))
        .filter_method(FilterMethod::Nearest)
        .content_fit(ContentFit::Fill)
        .width(Length::Fixed(SPECTRUM_W))
        .height(Length::Fixed(SPECTRUM_H))
        .into()
}

fn spectrum_px() -> (u32, u32) {
    (SPECTRUM_W.round() as u32, SPECTRUM_H.round() as u32)
}

fn phosphor(from_bottom: usize, muted: bool) -> Color {
    let [r, g, b] = ROW_RGB[from_bottom.min(ROWS - 1)];
    let heat = from_bottom as f32 / (ROWS - 1) as f32;
    let t = heat * heat * 0.28;
    Color {
        r: r + (1.0 - r) * t,
        g: g + (1.0 - g) * t,
        b: b + (1.0 - b) * t,
        a: if muted { 0.42 } else { 1.0 },
    }
}

fn raster(muted: bool, theme: &Theme) -> Vec<u8> {
    let (w, h) = spectrum_px();
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let unlit = unlit_color(theme);
    let bands = meter::samples();
    for band in 0..BANDS {
        let x_bar = band as u32 * (BAR_W + BAND_GAP);
        for col in 0..BAR_COLS as u32 {
            let x0 = x_bar + col * ISLAND.pitch;
            for row in 0..ROWS {
                let from_bottom = ROWS - 1 - row;
                let c = if lit(bands[band], ROWS, from_bottom) {
                    phosphor(from_bottom, muted)
                } else {
                    unlit
                };
                paint_dot(&mut buf, w, h, x0, row as u32 * ISLAND.pitch, ISLAND, c);
            }
        }
    }
    round_ends(&mut buf, w, h);
    buf
}

/// Fade every pixel by how much of it lies inside a stadium of the
/// matrix's size (ends of radius h/2), so the end columns curve.
fn round_ends(buf: &mut [u8], w: u32, h: u32) {
    const SAMPLES: u32 = 4;
    let (wf, r) = (w as f32, h as f32 / 2.0);
    let inside = |x: f32, y: f32| {
        let cx = x.clamp(r, wf - r);
        (x - cx).powi(2) + (y - r).powi(2) <= r * r
    };
    for py in 0..h {
        for px in 0..w {
            let mut hits = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = px as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    hits += u32::from(inside(x, y));
                }
            }
            let i = ((py * w + px) * 4 + 3) as usize;
            buf[i] = (f32::from(buf[i]) * hits as f32 / (SAMPLES * SAMPLES) as f32).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::pixel::GRAPH_W;

    #[test]
    fn spectrum_is_about_three_times_the_stat_graph() {
        assert!((SPECTRUM_W - GRAPH_W * 3.0).abs() < 20.0, "{SPECTRUM_W}");
        assert!(SPECTRUM_W > GRAPH_W * 2.5);
        assert_eq!(SPECTRUM_H, GRAPH_H);
        assert_eq!(SPECTRUM_W, 12.0 * 8.0 + 11.0 * 2.0);
        assert_eq!(spectrum_px(), (118, 14));
    }

    #[test]
    fn the_ends_curve_and_the_middle_does_not() {
        let (w, h) = spectrum_px();
        let mut buf = vec![255u8; (w * h * 4) as usize];
        round_ends(&mut buf, w, h);
        let alpha = |x: u32, y: u32| buf[((y * w + x) * 4 + 3) as usize];
        // Corners go, the middle row of an end stays, the middle is whole.
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(w - 1, h - 1), 0);
        assert_eq!(alpha(0, h / 2), 255);
        assert_eq!(alpha(w / 2, 0), 255);
        // Partly inside: a soft edge.
        assert!((1..255).contains(&alpha(1, 2)), "{}", alpha(1, 2));
    }
}
