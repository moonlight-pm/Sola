//! Pixel lattice + zoom. The grid is the product.

use iced::mouse;
use iced::widget::canvas::{self, Cache, Canvas, Event, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme};

use crate::app::Msg;

pub const ZOOM_MIN: u32 = 1;
pub const ZOOM_MAX: u32 = 11;
pub const ZOOM_DEFAULT: u32 = 7;

/// Odd source-pixel count. Zoom in → fewer, larger cells.
pub fn sample_size(zoom: u32) -> u32 {
    match zoom.clamp(ZOOM_MIN, ZOOM_MAX) {
        1 => 65,
        2 => 51,
        3 => 41,
        4 => 33,
        5 => 25,
        6 => 21,
        7 => 15,
        8 => 11,
        9 => 7,
        10 => 5,
        _ => 3,
    }
}

#[derive(Debug, Clone)]
pub struct Patch {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub pixels: Vec<u8>,
}

impl Patch {
    pub fn hot_rgba(&self) -> Option<[u8; 4]> {
        rgba_at(&self.pixels, self.width, self.hot_x, self.hot_y)
    }
}

pub fn rgba_at(pixels: &[u8], width: u32, x: u32, y: u32) -> Option<[u8; 4]> {
    if x >= width {
        return None;
    }
    let i = ((y as usize).checked_mul(width as usize)?).checked_add(x as usize)? * 4;
    let slice = pixels.get(i..i + 4)?;
    Some([slice[0], slice[1], slice[2], slice[3]])
}

pub fn view<'a>(
    patch: Option<&'a Patch>,
    cache: &'a Cache,
    well: Color,
    hairline: Color,
    accent: Color,
) -> Element<'a, Msg> {
    Canvas::new(GridCanvas {
        patch,
        cache,
        well,
        hairline,
        accent,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct GridCanvas<'a> {
    patch: Option<&'a Patch>,
    cache: &'a Cache,
    well: Color,
    hairline: Color,
    accent: Color,
}

impl canvas::Program<Msg> for GridCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Msg>> {
        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.position_in(bounds).is_none() {
                    return None;
                }
                let steps = match *delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / 60.0,
                };
                if steps.abs() < f32::EPSILON {
                    return None;
                }
                let msg = if steps > 0.0 {
                    Msg::ZoomIn
                } else {
                    Msg::ZoomOut
                };
                Some(canvas::Action::publish(msg).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            paint(
                frame,
                bounds.size(),
                self.patch,
                self.well,
                self.hairline,
                self.accent,
            );
        });
        vec![geometry]
    }
}

fn paint(
    frame: &mut Frame,
    view: Size,
    patch: Option<&Patch>,
    well: Color,
    hairline: Color,
    accent: Color,
) {
    frame.fill(&Path::rectangle(Point::ORIGIN, view), well);
    let Some(patch) = patch else {
        return;
    };
    if patch.width == 0 || patch.height == 0 {
        return;
    }
    let cols = patch.width as f32;
    let rows = patch.height as f32;
    let cell = (view.width / cols).min(view.height / rows).floor().max(1.0);
    let grid_w = cell * cols;
    let grid_h = cell * rows;
    let ox = ((view.width - grid_w) / 2.0).floor();
    let oy = ((view.height - grid_h) / 2.0).floor();

    for y in 0..patch.height {
        for x in 0..patch.width {
            let Some([r, g, b, a]) = rgba_at(&patch.pixels, patch.width, x, y) else {
                continue;
            };
            let color = Color::from_rgba8(r, g, b, a as f32 / 255.0);
            let p = Point::new(ox + x as f32 * cell, oy + y as f32 * cell);
            frame.fill(&Path::rectangle(p, Size::new(cell, cell)), color);
        }
    }

    if cell >= 8.0 {
        let stroke = Stroke::default().with_color(hairline).with_width(1.0);
        for i in 0..=patch.width {
            let x = ox + i as f32 * cell;
            frame.stroke(
                &Path::line(Point::new(x, oy), Point::new(x, oy + grid_h)),
                stroke,
            );
        }
        for i in 0..=patch.height {
            let y = oy + i as f32 * cell;
            frame.stroke(
                &Path::line(Point::new(ox, y), Point::new(ox + grid_w, y)),
                stroke,
            );
        }
    }

    let hot = Path::rectangle(
        Point::new(
            ox + patch.hot_x as f32 * cell,
            oy + patch.hot_y as f32 * cell,
        ),
        Size::new(cell, cell),
    );
    frame.stroke(
        &hot,
        Stroke::default()
            .with_color(accent)
            .with_width(2.0_f32.max(cell / 8.0)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_size_is_odd_and_shrinks_with_zoom() {
        let mut prev = u32::MAX;
        for z in ZOOM_MIN..=ZOOM_MAX {
            let n = sample_size(z);
            assert_eq!(n % 2, 1, "zoom {z} size {n} must be odd");
            assert!(n < prev, "zoom in must show fewer pixels");
            prev = n;
        }
        assert_eq!(sample_size(ZOOM_DEFAULT), 15);
        assert_eq!(sample_size(ZOOM_MIN), 65);
        assert_eq!(sample_size(ZOOM_MAX), 3);
        assert_eq!(sample_size(0), sample_size(ZOOM_MIN));
        assert_eq!(sample_size(99), sample_size(ZOOM_MAX));
    }

    #[test]
    fn rgba_at_reads_row_major() {
        let px = vec![1, 0, 0, 255, 0, 2, 0, 255, 0, 0, 3, 255, 4, 5, 6, 255];
        assert_eq!(rgba_at(&px, 2, 1, 0), Some([0, 2, 0, 255]));
        assert_eq!(rgba_at(&px, 2, 0, 1), Some([0, 0, 3, 255]));
        assert_eq!(rgba_at(&px, 2, 2, 0), None);
    }
}
