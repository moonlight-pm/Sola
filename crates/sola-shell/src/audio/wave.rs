//! Menubar spectrum analyzer — 12 treble-biased frequency bars in the same
//! dithered LED language as the stats graphs. Height is band amplitude
//! (graphic EQ / old-stereo analyzer), not a scrolling waveform.

use std::time::{Duration, Instant};

use iced::widget::canvas::{self, Canvas, Frame, Geometry};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, mouse, window};

use super::meter::{self, BANDS};
use crate::stats::pixel::{DOT, GAP, GRAPH_H, ROWS, cell_fill, cell_pixel_on};

/// Same cadence as the kit working ring: live motion, not a vsync pump.
const WAVE_TICK: Duration = Duration::from_millis(50);
/// LED columns that move together as one frequency bar.
const BAR_COLS: usize = 3;
/// Extra gutter between bars so they read as 12 meters, not one matrix.
const BAND_GAP: f32 = 2.0;

const BAR_W: f32 = BAR_COLS as f32 * DOT + (BAR_COLS - 1) as f32 * GAP;

/// ~3× the stats graph: 12 bars × 3 LED columns + band gutters.
pub const SPECTRUM_W: f32 = BANDS as f32 * BAR_W + (BANDS - 1) as f32 * BAND_GAP;
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

pub fn visualizer<'a, Message: 'a>(muted: bool) -> Element<'a, Message> {
    Canvas::new(Spectrum { muted })
        .width(Length::Fixed(SPECTRUM_W))
        .height(Length::Fixed(SPECTRUM_H))
        .into()
}

struct Spectrum {
    muted: bool,
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

impl<Message> canvas::Program<Message> for Spectrum {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if !meter::is_live() {
            return None;
        }
        matches!(event, Event::Window(window::Event::RedrawRequested(_)))
            .then(|| canvas::Action::request_redraw_at(Instant::now() + WAVE_TICK))
    }

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
        let bands = meter::samples();
        let stride = BAR_W + BAND_GAP;

        for band in 0..BANDS {
            let fill = bands[band].clamp(0.0, 1.0);
            let x_bar = (band as f32 * stride).round();
            for col in 0..BAR_COLS {
                let x0 = x_bar + (col as f32 * pitch).round();
                for row in 0..ROWS {
                    let from_bottom = ROWS - 1 - row;
                    let cf = cell_fill(fill, from_bottom);
                    let y0 = (row as f32 * pitch).round();
                    let lit = phosphor(from_bottom, self.muted);
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
        }
        vec![frame.into_geometry()]
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
    }
}
