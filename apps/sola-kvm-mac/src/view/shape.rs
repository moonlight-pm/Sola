//! The screen a viewer asks for, as the agent makes it: points are the
//! viewer's logical pixels, pixels twice them for a window scaled above 1,
//! even, and inside what a display takes.

use crate::view_wire::Size;

/// The most pixels a side a made display has.
pub const MAX_PIXELS: (u32, u32) = (7680, 4320);
const LEAST_POINTS: (u32, u32) = (640, 480);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub points: (u32, u32),
    /// Pixels a point: 1, or 2 (HiDPI).
    pub scale: u32,
}

impl Shape {
    pub fn of(size: Size) -> Self {
        let scale = if size.scale_percent > 100 { 2 } else { 1 };
        let side = |want: u32, least: u32, most: u32| want.clamp(least, most / scale) & !1;
        Self {
            points: (
                side(size.width, LEAST_POINTS.0, MAX_PIXELS.0),
                side(size.height, LEAST_POINTS.1, MAX_PIXELS.1),
            ),
            scale,
        }
    }

    pub fn pixels(self) -> (usize, usize) {
        ((self.points.0 * self.scale) as usize, (self.points.1 * self.scale) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: u32, height: u32, scale_percent: u16) -> Size {
        Size {
            width,
            height,
            scale_percent,
        }
    }

    #[test]
    fn at_scale_1_a_point_is_a_pixel_and_sides_are_even() {
        let shape = Shape::of(size(3635, 2102, 100));
        assert_eq!(shape, Shape { points: (3634, 2102), scale: 1 });
        assert_eq!(shape.pixels(), (3634, 2102));
    }

    #[test]
    fn a_window_scaled_above_1_gets_twice_the_pixels() {
        let shape = Shape::of(size(1200, 800, 150));
        assert_eq!(shape.pixels(), (2400, 1600));
    }

    #[test]
    fn it_stays_inside_what_a_display_takes() {
        assert_eq!(Shape::of(size(10, 10, 100)).points, (640, 480));
        assert_eq!(Shape::of(size(9000, 9000, 100)).pixels(), (7680, 4320));
        assert_eq!(Shape::of(size(5000, 3000, 200)).pixels(), (7680, 4320));
    }
}
