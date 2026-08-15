//! Contain-fit geometry, zoom/pan, and crop mapping (pure; unit-tested).

use iced::{Point, Rectangle, Size, Vector};

pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 16.0;
pub const ZOOM_STEP: f32 = 0.10;

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

/// Image dest in view space at `zoom` (1.0 = contain-fit) + `pan`.
pub fn dest_rect(img: Size, view: Size, zoom: f32, pan: Vector) -> Rectangle {
    let fit = contain_rect(img, view);
    if fit.width <= 0.0 || fit.height <= 0.0 {
        return fit;
    }
    let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let w = fit.width * zoom;
    let h = fit.height * zoom;
    Rectangle::new(
        Point::new((view.width - w) * 0.5 + pan.x, (view.height - h) * 0.5 + pan.y),
        Size::new(w, h),
    )
}

/// Clamp `pan` so a zoomed-in image cannot be dragged fully off-stage.
/// When the dest is smaller than the view, pan is zeroed.
pub fn clamp_pan(img: Size, view: Size, zoom: f32, pan: Vector) -> Vector {
    let dest = dest_rect(img, view, zoom, Vector::ZERO);
    let max_x = ((dest.width - view.width) * 0.5).max(0.0);
    let max_y = ((dest.height - view.height) * 0.5).max(0.0);
    Vector::new(pan.x.clamp(-max_x, max_x), pan.y.clamp(-max_y, max_y))
}

/// Multiply zoom by `factor`, keeping the image point under `cursor` fixed.
pub fn zoom_at(
    img: Size,
    view: Size,
    zoom: f32,
    pan: Vector,
    cursor: Point,
    factor: f32,
) -> (f32, Vector) {
    let old = dest_rect(img, view, zoom, pan);
    if old.width <= 0.0 || old.height <= 0.0 {
        return (zoom, pan);
    }
    let next = (zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    if (next - zoom).abs() < f32::EPSILON {
        return (zoom, clamp_pan(img, view, zoom, pan));
    }
    let nx = (cursor.x - old.x) / old.width;
    let ny = (cursor.y - old.y) / old.height;
    let new_w = old.width * (next / zoom);
    let new_h = old.height * (next / zoom);
    let pan_x = cursor.x - (view.width - new_w) * 0.5 - nx * new_w;
    let pan_y = cursor.y - (view.height - new_h) * 0.5 - ny * new_h;
    let pan = clamp_pan(img, view, next, Vector::new(pan_x, pan_y));
    (next, pan)
}

/// Wheel / key zoom factor. `steps` > 0 zooms in.
pub fn zoom_factor(steps: f32) -> f32 {
    if steps >= 0.0 {
        (1.0 + ZOOM_STEP).powf(steps)
    } else {
        1.0 / (1.0 + ZOOM_STEP).powf(-steps)
    }
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
    fn dest_at_1x_matches_contain() {
        let img = Size::new(100.0, 50.0);
        let view = Size::new(400.0, 400.0);
        let a = contain_rect(img, view);
        let b = dest_rect(img, view, 1.0, Vector::ZERO);
        assert!((a.x - b.x).abs() < 0.01);
        assert!((a.width - b.width).abs() < 0.01);
    }

    #[test]
    fn dest_2x_doubles_and_stays_centered() {
        let img = Size::new(100.0, 50.0);
        let view = Size::new(400.0, 400.0);
        let dest = dest_rect(img, view, 2.0, Vector::ZERO);
        assert!((dest.width - 800.0).abs() < 0.01);
        assert!((dest.height - 400.0).abs() < 0.01);
        assert!((dest.x + 200.0).abs() < 0.01);
        assert!((dest.y - 0.0).abs() < 0.01);
    }

    #[test]
    fn clamp_pan_zero_when_fitted() {
        let img = Size::new(100.0, 50.0);
        let view = Size::new(400.0, 400.0);
        let pan = clamp_pan(img, view, 1.0, Vector::new(80.0, 40.0));
        assert!((pan.x).abs() < 0.01);
        assert!((pan.y).abs() < 0.01);
    }

    #[test]
    fn zoom_at_keeps_focal_point() {
        let img = Size::new(100.0, 100.0);
        let view = Size::new(200.0, 200.0);
        let cursor = Point::new(100.0, 100.0);
        let (z, pan) = zoom_at(img, view, 1.0, Vector::ZERO, cursor, 2.0);
        assert!((z - 2.0).abs() < 0.01);
        let dest = dest_rect(img, view, z, pan);
        // Center of the 1× dest is the image center; still under the cursor.
        let cx = dest.x + dest.width * 0.5;
        let cy = dest.y + dest.height * 0.5;
        assert!((cx - cursor.x).abs() < 0.5);
        assert!((cy - cursor.y).abs() < 0.5);
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

    #[test]
    fn crop_pixels_tracks_zoomed_dest() {
        let dest = dest_rect(
            Size::new(100.0, 100.0),
            Size::new(100.0, 100.0),
            2.0,
            Vector::ZERO,
        );
        // Dest is 200×200 centered → (-50, -50). Select the visible center
        // 50×50 which maps to the middle 25% of the image.
        let sel = Rectangle::new(Point::new(25.0, 25.0), Size::new(50.0, 50.0));
        let (x, y, w, h) = crop_pixels(sel, dest, 100, 100).unwrap();
        assert_eq!((x, y), (37, 37));
        assert_eq!((w, h), (26, 26));
    }
}
