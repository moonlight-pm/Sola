//! Where the picture sits in the window, and where a point in the window
//! lands on the other machine.

use iced::{Point, Rectangle, Size};

/// The largest rectangle of the picture's shape inside `bounds`, centered,
/// relative to `bounds`' origin.
pub fn fit(frame: (u32, u32), bounds: Size) -> Rectangle {
    if frame.0 == 0 || frame.1 == 0 {
        return Rectangle::with_size(bounds);
    }
    let scale = (bounds.width / frame.0 as f32).min(bounds.height / frame.1 as f32);
    let (w, h) = (frame.0 as f32 * scale, frame.1 as f32 * scale);
    Rectangle {
        x: (bounds.width - w) / 2.0,
        y: (bounds.height - h) / 2.0,
        width: w,
        height: h,
    }
}

/// `at` (relative to the widget) in the other machine's coordinates; `None`
/// when it is outside the picture.
pub fn to_points(at: Point, picture: Rectangle, points: (u32, u32)) -> Option<(i32, i32)> {
    if !picture.contains(at) || picture.width <= 0.0 || picture.height <= 0.0 {
        return None;
    }
    let x = (at.x - picture.x) / picture.width * points.0 as f32;
    let y = (at.y - picture.y) / picture.height * points.1 as f32;
    Some((x as i32, y as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_window_bars_the_sides() {
        let r = fit((1000, 500), Size::new(2000.0, 500.0));
        assert_eq!((r.x, r.y, r.width, r.height), (500.0, 0.0, 1000.0, 500.0));
    }

    #[test]
    fn a_tall_window_bars_top_and_bottom() {
        let r = fit((1000, 500), Size::new(1000.0, 1000.0));
        assert_eq!((r.x, r.y, r.width, r.height), (0.0, 250.0, 1000.0, 500.0));
    }

    #[test]
    fn an_unknown_picture_fills() {
        let r = fit((0, 0), Size::new(30.0, 20.0));
        assert_eq!((r.width, r.height), (30.0, 20.0));
    }

    #[test]
    fn points_map_through_the_picture_not_the_window() {
        let picture = Rectangle {
            x: 500.0,
            y: 0.0,
            width: 1000.0,
            height: 500.0,
        };
        assert_eq!(to_points(Point::new(1000.0, 250.0), picture, (1512, 982)), Some((756, 491)));
        assert_eq!(to_points(Point::new(500.0, 0.0), picture, (1512, 982)), Some((0, 0)));
    }

    #[test]
    fn the_bars_are_not_the_picture() {
        let picture = Rectangle {
            x: 500.0,
            y: 0.0,
            width: 1000.0,
            height: 500.0,
        };
        assert_eq!(to_points(Point::new(100.0, 250.0), picture, (1512, 982)), None);
    }
}
