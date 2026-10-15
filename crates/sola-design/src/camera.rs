//! Plane camera. Canvas units are CSS px at 100%; screen points are
//! logical px relative to the plane widget's top-left.
//!
//! `screen = offset + canvas · zoom`

use iced::mouse::ScrollDelta;
use iced::{Point, Rectangle, Size, Vector};

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 32.0;
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
        Self::fit_below(content, view, max_zoom, 0.0)
    }

    /// [`Camera::fit`], keeping `top` px free above the content (section
    /// titles sit above their boards).
    pub fn fit_below(content: Rectangle, view: Size, max_zoom: f32, top: f32) -> Self {
        let avail = Size::new(
            (view.width - 2.0 * FIT_PAD).max(1.0),
            (view.height - 2.0 * FIT_PAD - top).max(1.0),
        );
        let zoom = (avail.width / content.width.max(1.0))
            .min(avail.height / content.height.max(1.0))
            .min(max_zoom);
        let mut cam = Self::centered(content, Size::new(view.width, view.height - top), zoom);
        cam.offset.y += top;
        cam
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

/// Frame one board for a tour step: whole and centred, at most 100%,
/// above `reserve` px at the bottom (the tour bar). A tall board (whole, it
/// would fill under half the width) is framed by its width, top first.
pub fn frame_board(board: Rectangle, view: Size, reserve: f32) -> Camera {
    const TOP: f32 = 40.0;
    let avail = Size::new(
        (view.width - 2.0 * FIT_PAD).max(1.0),
        (view.height - 2.0 * FIT_PAD - reserve).max(1.0),
    );
    let by_width = (avail.width / board.width.max(1.0)).min(1.0);
    let whole = by_width.min(avail.height / board.height.max(1.0));
    if board.width * whole < 0.5 * avail.width && by_width > whole {
        let zoom = by_width.clamp(MIN_ZOOM, MAX_ZOOM);
        return Camera {
            offset: Vector::new(
                view.width / 2.0 - board.center_x() * zoom,
                TOP - board.y * zoom,
            ),
            zoom,
        };
    }
    Camera::centered(board, Size::new(view.width, view.height - reserve), whole)
}

/// `a` → `b` at `t` (0..=1, eased by the caller): the canvas point at the
/// view's centre moves straight, zoom changes geometrically.
pub fn lerp(a: Camera, b: Camera, t: f32, view: Size) -> Camera {
    let mid = Point::new(view.width / 2.0, view.height / 2.0);
    let (ca, cb) = (a.to_canvas(mid), b.to_canvas(mid));
    let zoom = a.zoom * (b.zoom / a.zoom).powf(t);
    let c = Point::new(ca.x + (cb.x - ca.x) * t, ca.y + (cb.y - ca.y) * t);
    Camera {
        offset: Vector::new(mid.x - c.x * zoom, mid.y - c.y * zoom),
        zoom,
    }
}

/// Ease in and out (cubic).
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
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
    fn steps_frame_whole_boards_or_tall_ones_by_width() {
        let view = Size::new(1200.0, 900.0);
        let wide = Rectangle::new(Point::new(1000.0, 0.0), Size::new(1920.0, 1080.0));
        let cam = frame_board(wide, view, 100.0);
        assert!(cam.zoom < 1.0);
        let c = cam.to_screen(wide.center());
        assert!(close(c.x, 600.0) && close(c.y, 400.0), "centred above the reserve");
        let tall = Rectangle::new(Point::new(0.0, 0.0), Size::new(1440.0, 9000.0));
        let cam = frame_board(tall, view, 100.0);
        assert!(close(cam.zoom, (1200.0 - 96.0) / 1440.0));
        assert!(close(cam.to_screen(tall.position()).y, 40.0), "top first");
        let small = Rectangle::new(Point::new(0.0, 0.0), Size::new(390.0, 400.0));
        assert!(close(frame_board(small, view, 100.0).zoom, 1.0), "never past 100%");
    }

    #[test]
    fn lerp_ends_on_both_cameras() {
        let view = Size::new(800.0, 600.0);
        let a = Camera { offset: Vector::new(10.0, 20.0), zoom: 0.2 };
        let b = Camera { offset: Vector::new(-400.0, 30.0), zoom: 1.6 };
        let (s, e) = (lerp(a, b, 0.0, view), lerp(a, b, 1.0, view));
        assert!(close(s.zoom, a.zoom) && close(s.offset.x, a.offset.x) && close(s.offset.y, a.offset.y));
        assert!(close(e.zoom, b.zoom) && close(e.offset.x, b.offset.x) && close(e.offset.y, b.offset.y));
        assert!(close(lerp(a, b, 0.5, view).zoom, (0.2f32 * 1.6).sqrt()));
        assert!(close(ease(0.0), 0.0) && close(ease(1.0), 1.0) && close(ease(0.5), 0.5));
    }

    #[test]
    fn wheel_up_zooms_in() {
        assert!(wheel_factor(ScrollDelta::Lines { x: 0.0, y: 1.0 }) > 1.0);
        assert!(wheel_factor(ScrollDelta::Pixels { x: 0.0, y: -40.0 }) < 1.0);
    }
}
