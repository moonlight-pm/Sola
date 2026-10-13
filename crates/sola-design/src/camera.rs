//! Plane camera. Canvas units are CSS px at 100%; screen points are
//! logical px relative to the plane widget's top-left.
//!
//! `screen = offset + canvas · zoom`

use iced::mouse::ScrollDelta;
use iced::{Point, Rectangle, Size, Vector};

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 8.0;
/// Space kept around the content on fit.
const FIT_PAD: f32 = 48.0;
/// One button / menu zoom step.
pub const STEP: f32 = 1.25;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub offset: Vector,
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            offset: Vector::new(FIT_PAD, FIT_PAD),
            zoom: 1.0,
        }
    }
}

impl Camera {
    pub fn to_screen(&self, p: Point) -> Point {
        Point::new(
            self.offset.x + p.x * self.zoom,
            self.offset.y + p.y * self.zoom,
        )
    }

    pub fn to_canvas(&self, p: Point) -> Point {
        Point::new(
            (p.x - self.offset.x) / self.zoom,
            (p.y - self.offset.y) / self.zoom,
        )
    }

    pub fn rect_to_screen(&self, r: Rectangle) -> Rectangle {
        let tl = self.to_screen(r.position());
        Rectangle::new(tl, Size::new(r.width * self.zoom, r.height * self.zoom))
    }

    pub fn pan(&mut self, delta: Vector) {
        self.offset = self.offset + delta;
    }

    /// Multiply zoom by `factor`, keeping the canvas point under `anchor` still.
    pub fn zoom_at(&mut self, anchor: Point, factor: f32) {
        self.set_zoom_at(anchor, self.zoom * factor);
    }

    pub fn set_zoom_at(&mut self, anchor: Point, zoom: f32) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let under = self.to_canvas(anchor);
        self.zoom = zoom;
        self.offset = Vector::new(anchor.x - under.x * zoom, anchor.y - under.y * zoom);
    }

    /// Fit `content` into `view`, never past `max_zoom`, centred.
    pub fn fit(content: Rectangle, view: Size, max_zoom: f32) -> Self {
        let avail = Size::new(
            (view.width - 2.0 * FIT_PAD).max(1.0),
            (view.height - 2.0 * FIT_PAD).max(1.0),
        );
        let zoom = (avail.width / content.width.max(1.0))
            .min(avail.height / content.height.max(1.0))
            .min(max_zoom);
        Self::centered(content, view, zoom)
    }

    /// `content` centred in `view` at `zoom`.
    pub fn centered(content: Rectangle, view: Size, zoom: f32) -> Self {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let center = content.center();
        Self {
            offset: Vector::new(
                view.width / 2.0 - center.x * zoom,
                view.height / 2.0 - center.y * zoom,
            ),
            zoom,
        }
    }
}

/// Zoom factor for one wheel event. Lines are a mouse wheel notch; pixels
/// come from a touchpad and arrive in many small steps.
pub fn wheel_factor(delta: ScrollDelta) -> f32 {
    match delta {
        ScrollDelta::Lines { y, .. } => 1.15_f32.powf(y),
        ScrollDelta::Pixels { y, .. } => (y * 0.0025).exp(),
    }
}

/// Smallest rectangle holding every `rects` entry.
pub fn bounds_of(rects: impl IntoIterator<Item = Rectangle>) -> Option<Rectangle> {
    rects.into_iter().reduce(|a, b| a.union(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn round_trips_points() {
        let cam = Camera {
            offset: Vector::new(30.0, -12.0),
            zoom: 0.4,
        };
        let p = Point::new(812.5, 90.0);
        let back = cam.to_canvas(cam.to_screen(p));
        assert!(close(back.x, p.x) && close(back.y, p.y));
    }

    #[test]
    fn zoom_keeps_anchor_still() {
        let mut cam = Camera::default();
        let anchor = Point::new(400.0, 300.0);
        let before = cam.to_canvas(anchor);
        cam.zoom_at(anchor, 2.0);
        let after = cam.to_canvas(anchor);
        assert!(close(before.x, after.x) && close(before.y, after.y));
        assert!(close(cam.zoom, 2.0));
        cam.zoom_at(anchor, 1000.0);
        assert!(close(cam.zoom, MAX_ZOOM));
    }

    #[test]
    fn fit_centres_and_caps() {
        let content = Rectangle::new(Point::new(0.0, 0.0), Size::new(2000.0, 1000.0));
        let view = Size::new(1096.0, 1096.0);
        let cam = Camera::fit(content, view, 1.0);
        assert!(close(cam.zoom, 0.5));
        let c = cam.to_screen(content.center());
        assert!(close(c.x, 548.0) && close(c.y, 548.0));
        let small = Rectangle::new(Point::new(10.0, 10.0), Size::new(100.0, 100.0));
        assert!(close(Camera::fit(small, view, 1.0).zoom, 1.0));
    }

    #[test]
    fn wheel_up_zooms_in() {
        assert!(wheel_factor(ScrollDelta::Lines { x: 0.0, y: 1.0 }) > 1.0);
        assert!(wheel_factor(ScrollDelta::Pixels { x: 0.0, y: -40.0 }) < 1.0);
    }
}
