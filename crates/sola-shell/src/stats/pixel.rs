//! btop-style LED graph for menubar stats.
//!
//! A fixed LED matrix (dot + gutter) replaces variable-width numbers so
//! the cluster cannot reflow. Each column is one recent sample.
//!
//! Two grids: the islands' meters are 2 px dots on a 3 px pitch
//! ([`ISLAND`]); the panels that hang from them draw the same LEDs at 4 on
//! 5 ([`PANEL`]) — history columns and bars — so the meter you clicked and
//! its panel read as one instrument. Dots are whole on both: lit or not, a
//! cell lighting once its column reaches the cell's middle ([`lit`]). The
//! cells were once a 2×2 Bayer dither (btop's five levels per braille
//! cell), but the part-lit cells read as broken dots (Joshua: "I don't
//! like how it looks").
//!
//! Painted as a nearest-neighbor RGBA image, not iced `canvas` 1×1
//! rectangles. Tiny fill_rectangle tessellation drops on GLES2 / software
//! GL (Oath metal, virtio llvmpipe); a 35×14 texture does not.

use iced::widget::image::{self, FilterMethod};
use iced::{Color, ContentFit, Element, Length, Theme};

pub const COLS: usize = 12;
pub const ROWS: usize = 5;

pub const GRAPH_W: f32 = (COLS as u32 * ISLAND.pitch - (ISLAND.pitch - ISLAND.dot)) as f32;
pub const GRAPH_H: f32 = (ROWS as u32 * ISLAND.pitch - (ISLAND.pitch - ISLAND.dot)) as f32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// 0–100: per-column warn / crit.
    Level,
    Rx,
    Tx,
}

/// An LED grid: a cell's side and the distance from one cell to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub dot: u32,
    pub pitch: u32,
}

/// The islands' meters.
pub const ISLAND: Grid = Grid { dot: 2, pitch: 3 };
/// The panels under them.
pub const PANEL: Grid = Grid { dot: 4, pitch: 5 };

impl Grid {
    /// Pixels `n` cells take, edge to edge.
    pub fn span(&self, n: usize) -> u32 {
        (n as u32 * self.pitch).saturating_sub(self.pitch - self.dot)
    }
}

/// Whether cell `from_bottom` (0 = the bottom row) of a `rows`-tall column
/// filled `fill` (0..1) is lit: once the fill reaches the cell's middle.
pub fn lit(fill: f32, rows: usize, from_bottom: usize) -> bool {
    fill.clamp(0.0, 1.0) * rows as f32 >= from_bottom as f32 + 0.5
}

/// One whole LED of `grid` at (`x0`, `y0`).
pub fn paint_dot(buf: &mut [u8], w: u32, h: u32, x0: u32, y0: u32, grid: Grid, c: Color) {
    for py in 0..grid.dot {
        for px in 0..grid.dot {
            put_rgba(buf, w, h, (x0 + px) as i32, (y0 + py) as i32, c);
        }
    }
}

/// An RGBA buffer as a pixel-exact image (nearest-neighbor, its own size).
pub fn image<'a, Message: 'a>(w: u32, h: u32, pixels: Vec<u8>) -> Element<'a, Message> {
    image::Image::new(image::Handle::from_rgba(w, h, pixels))
        .filter_method(FilterMethod::Nearest)
        .content_fit(ContentFit::Fill)
        .width(Length::Fixed(w as f32))
        .height(Length::Fixed(h as f32))
        .into()
}

/// Columns of LEDs on `grid`, one per value (0..1 of the height, lit from
/// the bottom), each in `colors[i]`. A NaN value is an empty column: no
/// cells, a gap.
pub fn columns<'a, Message: 'a>(
    fills: &[f32],
    colors: &[Color],
    rows: usize,
    grid: Grid,
    unlit: Color,
) -> Element<'a, Message> {
    let (w, h, buf) = raster_columns(fills, colors, rows, grid, unlit);
    image(w, h, buf)
}

fn raster_columns(
    fills: &[f32],
    colors: &[Color],
    rows: usize,
    grid: Grid,
    unlit: Color,
) -> (u32, u32, Vec<u8>) {
    let (w, h) = (grid.span(fills.len()), grid.span(rows));
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for (col, (&fill, &lit)) in fills.iter().zip(colors).enumerate() {
        if fill.is_nan() {
            continue;
        }
        for row in 0..rows {
            let c = if self::lit(fill, rows, rows - 1 - row) { lit } else { unlit };
            paint_dot(&mut buf, w, h, col as u32 * grid.pitch, row as u32 * grid.pitch, grid, c);
        }
    }
    (w, h, buf)
}

/// A row of `cells` LEDs on `grid`, lit from the left in runs: each
/// `(fraction of the row, color)` in turn, the rest unlit. A cell takes the
/// run its middle falls in.
pub fn bar<'a, Message: 'a>(
    runs: &[(f32, Color)],
    cells: usize,
    grid: Grid,
    unlit: Color,
) -> Element<'a, Message> {
    let (w, h, buf) = raster_bar(runs, cells, grid, unlit);
    image(w, h, buf)
}

fn raster_bar(
    runs: &[(f32, Color)],
    cells: usize,
    grid: Grid,
    unlit: Color,
) -> (u32, u32, Vec<u8>) {
    let (w, h) = (grid.span(cells), grid.dot);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let mut spans = Vec::with_capacity(runs.len());
    let mut at = 0.0;
    for &(frac, color) in runs {
        let end = at + frac.clamp(0.0, 1.0) * cells as f32;
        spans.push((at, end, color));
        at = end;
    }
    for i in 0..cells {
        let mid = i as f32 + 0.5;
        let c = spans.iter().find(|&&(a, b, _)| mid >= a && mid < b).map_or(unlit, |s| s.2);
        paint_dot(&mut buf, w, h, i as u32 * grid.pitch, 0, grid, c);
    }
    (w, h, buf)
}

/// An island's meter: the last [`COLS`] samples as columns on [`ISLAND`],
/// each tinted by its own sample.
pub fn graph<'a, Message: 'a>(
    samples: Vec<f32>,
    max: f32,
    tint: Tint,
    theme: &Theme,
) -> Element<'a, Message> {
    let (w, h, buf) = raster_graph(&samples, max, tint, theme);
    image(w, h, buf)
}

fn raster_graph(samples: &[f32], max: f32, tint: Tint, theme: &Theme) -> (u32, u32, Vec<u8>) {
    let samples = last_n(samples, COLS);
    let max = max.max(1.0);
    let p = theme.extended_palette();
    let fills: Vec<f32> = samples.iter().map(|s| (s / max).clamp(0.0, 1.0)).collect();
    let colors: Vec<Color> = samples
        .iter()
        .map(|&s| match tint {
            Tint::Level => crate::stats::level_color(s, theme),
            Tint::Rx => p.primary.base.color,
            Tint::Tx => p.success.base.color,
        })
        .collect();
    raster_columns(&fills, &colors, ROWS, ISLAND, unlit_color(theme))
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
        assert_eq!((ISLAND.span(COLS), ISLAND.span(ROWS)), (35, 14));
    }

    #[test]
    fn a_cell_lights_once_the_fill_reaches_its_middle() {
        // 0.5 × 5 rows = 2.5 cells: the bottom three lit, the third just.
        let lit_rows = |fill| (0..ROWS).filter(|&r| lit(fill, ROWS, r)).count();
        assert_eq!(lit_rows(0.5), 3);
        assert_eq!(lit_rows(0.49), 2);
        assert_eq!(lit_rows(0.0), 0);
        assert_eq!(lit_rows(0.09), 0);
        assert_eq!(lit_rows(0.11), 1);
        assert_eq!(lit_rows(1.0), ROWS);
    }

    /// The color of dot (`col`, `row`) on `grid`, checking every pixel of
    /// it agrees.
    fn dot_at(grid: Grid, w: u32, buf: &[u8], col: u32, row: u32) -> [u8; 4] {
        let px = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        };
        let (x0, y0) = (col * grid.pitch, row * grid.pitch);
        let first = px(x0, y0);
        for y in y0..y0 + grid.dot {
            for x in x0..x0 + grid.dot {
                assert_eq!(px(x, y), first, "dot ({col}, {row}) is not whole");
            }
        }
        first
    }

    #[test]
    fn island_meter_dots_are_whole() {
        let theme = Theme::Dark;
        let samples: Vec<f32> = (0..COLS).map(|i| i as f32 * 100.0 / (COLS - 1) as f32).collect();
        let (w, h, buf) = raster_graph(&samples, 100.0, Tint::Rx, &theme);
        assert_eq!((w, h), (35, 14));
        for col in 0..COLS as u32 {
            for row in 0..ROWS as u32 {
                dot_at(ISLAND, w, &buf, col, row);
            }
        }
    }

    #[test]
    fn panel_columns_light_whole_dots() {
        let (lit, unlit) = (Color::WHITE, Color::BLACK);
        // 0.3 × 4 rows = 1.2 cells: the bottom dot lit, the rest dark.
        // 0.6 × 4 = 2.4: two. 0.65 × 4 = 2.6: three.
        let (w, _, buf) = raster_columns(&[0.3, 0.6, 0.65], &[lit; 3], 4, PANEL, unlit);
        let lit_rows = |col| (0..4).filter(|&row| dot_at(PANEL, w, &buf, col, row) == [255; 4]).count();
        assert_eq!((lit_rows(0), lit_rows(1), lit_rows(2)), (1, 2, 3));
    }

    #[test]
    fn panel_bar_dots_take_one_run_each() {
        let (a, b, unlit) = (Color::WHITE, Color::from_rgb(1.0, 0.0, 0.0), Color::BLACK);
        // 4 cells: a to 1.3, b to 2.6, unlit after.
        let (w, _, buf) = raster_bar(&[(0.325, a), (0.325, b)], 4, PANEL, unlit);
        let dots: Vec<_> = (0..4).map(|i| dot_at(PANEL, w, &buf, i, 0)).collect();
        assert_eq!(dots, vec![[255; 4], [255, 0, 0, 255], [255, 0, 0, 255], [0, 0, 0, 255]]);
    }

    #[test]
    fn warn_threshold_unchanged() {
        assert!(crate::stats::WARN_PCT < crate::stats::CRIT_PCT);
    }

    #[test]
    fn raster_lights_pixels_for_a_full_column() {
        let theme = Theme::Dark;
        let (_, _, pixels) = raster_graph(&[100.0; COLS], 100.0, Tint::Level, &theme);
        assert_eq!(pixels.len(), 35 * 14 * 4);
        let lit = pixels.chunks(4).filter(|px| px[3] > 200).count();
        assert!(lit > 80, "expected a full matrix of opaque LEDs, got {lit}");
    }

    #[test]
    fn raster_empty_still_paints_unlit_dots() {
        let theme = Theme::Dark;
        let (_, _, pixels) = raster_graph(&[0.0; COLS], 100.0, Tint::Level, &theme);
        let dim = pixels.chunks(4).filter(|px| px[3] > 0 && px[3] < 80).count();
        assert!(dim > 80, "unlit LEDs should still be visible, got {dim}");
    }
}
