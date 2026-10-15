//! Annotation geometry in screen points: section boxes, arrow curves, and
//! where a note's card goes. Pure; the share viewer mirrors it in JS.

use iced::{Point, Rectangle, Size, Vector};

/// Section padding around its boards, in canvas units…
pub const SECTION_PAD: f32 = 64.0;
/// …never under this on screen.
pub const SECTION_MIN_PAD: f32 = 12.0;
/// Room above a section's top row for its boards' titles.
pub const TITLE_BAND: f32 = 24.0;
/// Room a fit keeps above the boards for section titles.
pub const HEADER_ROOM: f32 = 44.0;
/// Most space between arrows that enter one side.
const SPREAD: f32 = 28.0;

/// A section's region around its boards' screen rects.
pub fn section_box(boards: impl IntoIterator<Item = Rectangle>, zoom: f32) -> Option<Rectangle> {
    let union = boards.into_iter().reduce(|a, b| a.union(&b))?;
    let pad = (SECTION_PAD * zoom).max(SECTION_MIN_PAD);
    let top = pad + TITLE_BAND;
    Some(Rectangle::new(
        Point::new(union.x - pad, union.y - top),
        Size::new(union.width + 2.0 * pad, union.height + pad + top),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    /// Unit vector pointing out of the side.
    pub fn out(self) -> Vector {
        match self {
            Side::Left => Vector::new(-1.0, 0.0),
            Side::Right => Vector::new(1.0, 0.0),
            Side::Top => Vector::new(0.0, -1.0),
            Side::Bottom => Vector::new(0.0, 1.0),
        }
    }

    fn horizontal(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
}

/// The sides an arrow from `a` to `b` leaves and enters by: across the
/// wider gap between them, else toward the other's centre.
pub fn sides(a: Rectangle, b: Rectangle) -> (Side, Side) {
    let gap_x = (b.x - (a.x + a.width)).max(a.x - (b.x + b.width));
    let gap_y = (b.y - (a.y + a.height)).max(a.y - (b.y + b.height));
    let d = b.center() - a.center();
    let across_x = if gap_x > 0.0 || gap_y > 0.0 { gap_x >= gap_y } else { d.x.abs() >= d.y.abs() };
    if across_x {
        if d.x >= 0.0 { (Side::Right, Side::Left) } else { (Side::Left, Side::Right) }
    } else if d.y >= 0.0 {
        (Side::Bottom, Side::Top)
    } else {
        (Side::Top, Side::Bottom)
    }
}

/// Point `k` of `n` along `side` of `r`, spread around its middle.
pub fn along(r: Rectangle, side: Side, k: usize, n: usize) -> Point {
    let len = if side.horizontal() { r.width } else { r.height };
    let gap = (len / (n as f32 + 1.0)).min(SPREAD);
    let off = (k as f32 - (n as f32 - 1.0) / 2.0) * gap;
    let c = r.center();
    match side {
        Side::Left => Point::new(r.x, c.y + off),
        Side::Right => Point::new(r.x + r.width, c.y + off),
        Side::Top => Point::new(c.x + off, r.y),
        Side::Bottom => Point::new(c.x + off, r.y + r.height),
    }
}

/// A cubic from `from` to `to`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    pub from: Point,
    pub c1: Point,
    pub c2: Point,
    pub to: Point,
}

impl Curve {
    /// Leaves `from` along `out`, arrives at `to` against `into_out` (the
    /// target side's outward direction).
    pub fn new(from: Point, out: Vector, to: Point, into_out: Vector) -> Self {
        let d = to - from;
        let reach = ((d.x * d.x + d.y * d.y).sqrt() * 0.45).clamp(24.0, 240.0);
        Self {
            from,
            c1: from + out * reach,
            c2: to + into_out * reach,
            to,
        }
    }

    pub fn at(&self, t: f32) -> Point {
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        let p = [self.from, self.c1, self.c2, self.to];
        Point::new(
            p.iter().zip(w).map(|(p, w)| p.x * w).sum(),
            p.iter().zip(w).map(|(p, w)| p.y * w).sum(),
        )
    }

    /// The arrowhead: tip at `to`, pointing the way the curve arrives.
    pub fn head(&self, len: f32, half: f32) -> [Point; 3] {
        let d = self.to - self.c2;
        let n = (d.x * d.x + d.y * d.y).sqrt().max(1e-3);
        let dir = Vector::new(d.x / n, d.y / n);
        let base = self.to - dir * len;
        let side = Vector::new(-dir.y, dir.x) * half;
        [self.to, base + side, base - side]
    }
}

/// One arrow to route: screen rects of its source (an element or a board)
/// and target board, and which board the target is (arrows into one side
/// of one board spread along it).
#[derive(Debug, Clone, Copy)]
pub struct Arrow {
    pub from: Rectangle,
    pub to: Rectangle,
    pub target: usize,
}

/// Curves for `arrows`, in order.
pub fn route(arrows: &[Arrow]) -> Vec<Curve> {
    let sides: Vec<(Side, Side)> = arrows.iter().map(|a| sides(a.from, a.to)).collect();
    let starts: Vec<Point> = arrows
        .iter()
        .zip(&sides)
        .map(|(a, (s, _))| along(a.from, *s, 0, 1))
        .collect();
    // Rank each arrow among those entering the same side of the same board,
    // by where it comes from along that side.
    let mut slot = vec![(0, 1); arrows.len()];
    for i in 0..arrows.len() {
        let peers: Vec<usize> = (0..arrows.len())
            .filter(|&j| arrows[j].target == arrows[i].target && sides[j].1 == sides[i].1)
            .collect();
        let along_side = |j: usize| {
            if sides[j].1.horizontal() { starts[j].x } else { starts[j].y }
        };
        let rank = peers
            .iter()
            .filter(|&&j| along_side(j) < along_side(i) || (along_side(j) == along_side(i) && j < i))
            .count();
        slot[i] = (rank, peers.len());
    }
    arrows
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let (out, into) = sides[i];
            let (k, n) = slot[i];
            Curve::new(starts[i], out.out(), along(a.to, into, k, n), into.out())
        })
        .collect()
}

/// Where each label goes on its curve: the middle, else a little either
/// side, whichever does not cover a label already placed. `sizes[i]` is
/// `None` for an arrow without one.
pub fn label_spots(curves: &[Curve], sizes: &[Option<Size>]) -> Vec<Option<Point>> {
    const TRIES: [f32; 5] = [0.5, 0.35, 0.65, 0.22, 0.78];
    let mut placed: Vec<Rectangle> = Vec::new();
    curves
        .iter()
        .zip(sizes)
        .map(|(c, size)| {
            let size = (*size)?;
            let at = |t: f32| {
                let m = c.at(t);
                Rectangle::new(Point::new(m.x - size.width / 2.0, m.y - size.height / 2.0), size)
            };
            let spot = TRIES
                .iter()
                .map(|&t| at(t))
                .find(|r| !placed.iter().any(|p| p.intersects(r)))
                .unwrap_or_else(|| at(0.5));
            placed.push(spot);
            Some(spot.center())
        })
        .collect()
}

/// Top-left of a note card of `size` beside its marker at `m`: to the
/// right, else to the left, kept on the plane.
pub fn card_at(m: Point, size: Size, plane: Size) -> Point {
    const GAP: f32 = 14.0;
    const EDGE: f32 = 8.0;
    let mut x = m.x + GAP;
    if x + size.width > plane.width - EDGE {
        x = m.x - GAP - size.width;
    }
    let y = (m.y - 12.0).min(plane.height - size.height - EDGE).max(EDGE);
    Point::new(x.max(EDGE), y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn sides_cross_the_wider_gap() {
        assert_eq!(sides(r(0., 0., 100., 100.), r(300., 50., 100., 100.)), (Side::Right, Side::Left));
        assert_eq!(sides(r(300., 0., 100., 100.), r(0., 0., 100., 100.)), (Side::Left, Side::Right));
        assert_eq!(sides(r(0., 0., 100., 100.), r(20., 400., 100., 100.)), (Side::Bottom, Side::Top));
        // Diagonal: the wider gap wins.
        assert_eq!(sides(r(0., 0., 100., 100.), r(150., 500., 100., 100.)), (Side::Bottom, Side::Top));
        // An element inside its board, the target beside the board.
        assert_eq!(sides(r(40., 40., 20., 10.), r(300., 0., 100., 100.)), (Side::Right, Side::Left));
    }

    #[test]
    fn arrows_into_one_side_spread_in_order() {
        let target = r(500., 0., 100., 300.);
        let arrows = [
            Arrow { from: r(0., 200., 50., 20.), to: target, target: 1 },
            Arrow { from: r(0., 0., 50., 20.), to: target, target: 1 },
        ];
        let c = route(&arrows);
        assert_eq!(c[0].to.x, 500.0);
        assert!(c[1].to.y < c[0].to.y, "the higher source enters higher");
        assert!(((c[0].to.y + c[1].to.y) / 2.0 - 150.0).abs() < 0.01, "centred on the side");
        let alone = route(&arrows[..1]);
        assert_eq!(alone[0].to, Point::new(500.0, 150.0));
    }

    #[test]
    fn curves_end_on_their_points_and_heads_point_in() {
        let c = Curve::new(Point::new(0., 0.), Vector::new(1., 0.), Point::new(200., 0.), Vector::new(-1., 0.));
        assert_eq!(c.at(0.0), c.from);
        assert_eq!(c.at(1.0), c.to);
        let [tip, a, b] = c.head(9.0, 4.0);
        assert_eq!(tip, c.to);
        assert!(a.x < tip.x && b.x < tip.x && a.y != b.y);
    }

    #[test]
    fn sections_pad_in_canvas_units_with_a_floor() {
        let b = section_box([r(100., 100., 50., 50.), r(300., 100., 50., 50.)], 1.0).unwrap();
        assert_eq!(b, r(36., 100. - 64. - TITLE_BAND, 250. + 128., 50. + 128. + TITLE_BAND));
        let small = section_box([r(0., 0., 10., 10.)], 0.05).unwrap();
        assert_eq!(small.x, -SECTION_MIN_PAD);
        assert!(section_box([], 1.0).is_none());
    }

    #[test]
    fn labels_slide_off_each_other() {
        let a = Curve::new(Point::new(0., 0.), Vector::new(1., 0.), Point::new(400., 0.), Vector::new(-1., 0.));
        let b = Curve::new(Point::new(400., 0.), Vector::new(-1., 0.), Point::new(0., 0.), Vector::new(1., 0.));
        let pill = Some(Size::new(60., 18.));
        let spots = label_spots(&[a, b, a], &[pill, pill, None]);
        assert_eq!(spots[0], Some(a.at(0.5)));
        let moved = spots[1].unwrap();
        assert!((moved.x - 200.0).abs() > 60.0, "the second label moved along: {moved:?}");
        assert_eq!(spots[2], None);
    }

    #[test]
    fn cards_flip_and_stay_on_the_plane() {
        let plane = Size::new(800., 600.);
        let size = Size::new(300., 100.);
        assert_eq!(card_at(Point::new(100., 100.), size, plane), Point::new(114., 88.));
        assert_eq!(card_at(Point::new(700., 590.), size, plane).x, 700. - 14. - 300.);
        assert_eq!(card_at(Point::new(700., 590.), size, plane).y, 600. - 100. - 8.);
    }
}
