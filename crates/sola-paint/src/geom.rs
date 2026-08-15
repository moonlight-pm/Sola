//! Contain-fit geometry and crop mapping (pure; unit-tested).

use iced::{Point, Rectangle, Size};

/// Destination rectangle for `ContentFit::Contain` of an image in `view`.
pub fn contain_rect(img: Size, view: Size) -> Rectangle {
    if img.width <= 0.0 || img.height <= 0.0 || view.width <= 0.0 || view.height <= 0.0 {
        return Rectangle::new(Point::ORIGIN, Size::ZERO);
    }
    let scale = (view.width / img.width).min(view.height / img.height);
    let w = img.width * scale;
    let h = img.height * scale;
    Rectangle::new(
        Point::new((view.width - w) * 0.5, (view.height - h) * 0.5),
        Size::new(w, h),
    )
}

/// Axis-aligned rect from two corners, snapped inside `bounds`.
pub fn norm_rect(a: Point, b: Point, bounds: Rectangle) -> Rectangle {
    let min_x = a.x.min(b.x).clamp(bounds.x, bounds.x + bounds.width);
    let max_x = a.x.max(b.x).clamp(bounds.x, bounds.x + bounds.width);
    let min_y = a.y.min(b.y).clamp(bounds.y, bounds.y + bounds.height);
    let max_y = a.y.max(b.y).clamp(bounds.y, bounds.y + bounds.height);
    Rectangle::new(
        Point::new(min_x, min_y),
        Size::new((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)),
    )
}

/// Map a view-space point onto image pixels. Out-of-dest points clamp to the edge.
#[allow(dead_code)] // used by tests; handy if later tools need a hit-test
pub fn view_to_pixel(pt: Point, dest: Rectangle, img_w: u32, img_h: u32) -> (u32, u32) {
    if dest.width <= 0.0 || dest.height <= 0.0 || img_w == 0 || img_h == 0 {
        return (0, 0);
    }
    let nx = ((pt.x - dest.x) / dest.width).clamp(0.0, 1.0);
    let ny = ((pt.y - dest.y) / dest.height).clamp(0.0, 1.0);
    let x = (nx * img_w as f32).floor() as u32;
    let y = (ny * img_h as f32).floor() as u32;
    (x.min(img_w.saturating_sub(1)), y.min(img_h.saturating_sub(1)))
}

/// Inclusive-origin / exclusive-end pixel crop from a view-space selection.
/// Returns `(x, y, width, height)` in image pixels, or `None` if degenerate.
pub fn crop_pixels(
    selection: Rectangle,
    dest: Rectangle,
    img_w: u32,
    img_h: u32,
) -> Option<(u32, u32, u32, u32)> {
    if selection.width < 2.0 || selection.height < 2.0 || dest.width <= 0.0 || dest.height <= 0.0 {
        return None;
    }
    let nx0 = ((selection.x - dest.x) / dest.width).clamp(0.0, 1.0);
    let ny0 = ((selection.y - dest.y) / dest.height).clamp(0.0, 1.0);
    let nx1 = ((selection.x + selection.width - dest.x) / dest.width).clamp(0.0, 1.0);
    let ny1 = ((selection.y + selection.height - dest.y) / dest.height).clamp(0.0, 1.0);
    let x0 = (nx0.min(nx1) * img_w as f32).floor() as u32;
    let y0 = (ny0.min(ny1) * img_h as f32).floor() as u32;
    let x1 = (nx0.max(nx1) * img_w as f32).ceil() as u32;
    let y1 = (ny0.max(ny1) * img_h as f32).ceil() as u32;
    let x = x0.min(img_w);
    let y = y0.min(img_h);
    let w = x1.saturating_sub(x).min(img_w.saturating_sub(x));
    let h = y1.saturating_sub(y).min(img_h.saturating_sub(y));
    (w >= 1 && h >= 1).then_some((x, y, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contain_centers_letterbox() {
        let dest = contain_rect(Size::new(100.0, 50.0), Size::new(400.0, 400.0));
        assert!((dest.width - 400.0).abs() < 0.01);
        assert!((dest.height - 200.0).abs() < 0.01);
        assert!((dest.x - 0.0).abs() < 0.01);
        assert!((dest.y - 100.0).abs() < 0.01);
    }

    #[test]
    fn contain_pillarbox() {
        let dest = contain_rect(Size::new(50.0, 100.0), Size::new(400.0, 200.0));
        assert!((dest.height - 200.0).abs() < 0.01);
        assert!((dest.width - 100.0).abs() < 0.01);
        assert!((dest.x - 150.0).abs() < 0.01);
    }

    #[test]
    fn view_to_pixel_corners() {
        let dest = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        assert_eq!(view_to_pixel(Point::new(10.0, 20.0), dest, 200, 100), (0, 0));
        assert_eq!(
            view_to_pixel(Point::new(110.0, 70.0), dest, 200, 100),
            (199, 99)
        );
    }

    #[test]
    fn crop_pixels_full_dest() {
        let dest = Rectangle::new(Point::ORIGIN, Size::new(200.0, 100.0));
        let sel = dest;
        assert_eq!(crop_pixels(sel, dest, 200, 100), Some((0, 0, 200, 100)));
    }

    #[test]
    fn crop_pixels_rejects_tiny() {
        let dest = Rectangle::new(Point::ORIGIN, Size::new(200.0, 100.0));
        let sel = Rectangle::new(Point::new(10.0, 10.0), Size::new(1.0, 1.0));
        assert_eq!(crop_pixels(sel, dest, 200, 100), None);
    }
}
