//! btop-style dithered pixel graph for menubar stats.
//!
//! A fixed LED matrix (dot + gutter) replaces variable-width numbers so
//! the cluster cannot reflow. Each column is one recent sample. A cell is
//! a 2×2 Bayer dither — the same five fill levels btop packs into braille.

use iced::widget::canvas::{self, Canvas, Frame, Geometry};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

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

#[derive(Clone, Debug)]
pub struct PixelGraph {
    pub samples: Vec<f32>,
    pub max: f32,
    pub tint: Tint,
}

pub fn graph<'a, Message: 'a>(samples: Vec<f32>, max: f32, tint: Tint) -> Element<'a, Message> {
    Canvas::new(PixelGraph {
        samples: last_n(&samples, COLS),
        max: max.max(1.0),
        tint,
    })
    .width(Length::Fixed(GRAPH_W))
    .height(Length::Fixed(GRAPH_H))
    .into()
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

impl<Message> canvas::Program<Message> for PixelGraph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let p = theme.extended_palette();
        let unlit = Color {
            a: 0.16,
            ..p.background.base.text
        };
        let pitch = DOT + GAP;
        let n = self.samples.len().min(COLS);

        for col in 0..COLS {
            let sample = if col < n { self.samples[col] } else { 0.0 };
            let fill = (sample / self.max).clamp(0.0, 1.0);
            let lit = match self.tint {
                Tint::Level => crate::stats::level_color(sample, p.background.base.text),
                Tint::Rx => p.primary.base.color,
                Tint::Tx => p.success.base.color,
            };
            let x0 = (col as f32 * pitch).round();
            for row in 0..ROWS {
                let from_bottom = ROWS - 1 - row;
                let cf = cell_fill(fill, from_bottom);
                let y0 = (row as f32 * pitch).round();
                for dy in 0..2u32 {
                    for dx in 0..2u32 {
                        let on = cell_pixel_on(cf, dx, dy);
                        frame.fill_rectangle(
                            Point::new(x0 + dx as f32, y0 + dy as f32),
                            Size::new(1.0, 1.0),
                            if on { lit } else { unlit },
                        );
                    }
                }
            }
        }
        vec![frame.into_geometry()]
    }
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
}
