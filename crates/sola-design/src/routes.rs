//! Annotation geometry: section boxes, connectors, and where a note's card
//! goes. Pure; the share viewer mirrors it in JS.
//!
//! Connectors run through the gutters between boards, never across one:
//! a port on the source board's side (level with the exit element), a stub
//! straight out, an orthogonal path on lines beside every board's edges
//! (shortest, with a cost per bend), the target's stub, its port. Routing
//! is in canvas units (it moves with the boards, not the camera); parallel
//! runs that share a gutter are nudged apart on screen.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use iced::{Point, Rectangle, Size, Vector};

/// Section padding around its boards, in canvas units…
pub const SECTION_PAD: f32 = 64.0;
/// …never under this on screen.
pub const SECTION_MIN_PAD: f32 = 12.0;
/// Room above a section's top row for its boards' titles.
pub const TITLE_BAND: f32 = 24.0;
/// Room a fit keeps above the boards for section titles.
pub const HEADER_ROOM: f32 = 44.0;
/// Room kept between a connector and any board (canvas units).
pub const CLEAR: f32 = 28.0;
/// What a bend costs against length (canvas units): fewer, longer runs.
const BEND: f32 = 320.0;
/// Least space between ports into one side (canvas units).
const PORT_GAP: f32 = 24.0;
/// Ports stay this far from a side's corners (canvas units).
const INSET: f32 = 24.0;

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

    /// The grid direction of [`Side::out`].
    fn dir(self) -> usize {
        match self {
            Side::Right => 0,
            Side::Left => 1,
            Side::Bottom => 2,
            Side::Top => 3,
        }
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

/// Ports into one side of one board at least [`PORT_GAP`] apart, in order,
/// still on the side.
fn space_ports(arrows: &[Arrow], sides: &[(Side, Side)], ends: &mut [Point]) {
    let mut done = vec![false; arrows.len()];
    for i in 0..arrows.len() {
        if done[i] {
            continue;
        }
        let into = sides[i].1;
        let along_x = into.horizontal();
        let get = |p: Point| if along_x { p.x } else { p.y };
        let mut peers: Vec<usize> = (i..arrows.len())
            .filter(|&j| arrows[j].target == arrows[i].target && sides[j].1 == into)
            .collect();
        for &j in &peers {
            done[j] = true;
        }
        peers.sort_by(|&a, &b| get(ends[a]).total_cmp(&get(ends[b])).then(a.cmp(&b)));
        let r = arrows[i].to;
        let (lo, len) = if along_x { (r.x, r.width) } else { (r.y, r.height) };
        let inset = INSET.min(len / 2.0);
        let (lo, hi) = (lo + inset, lo + len - inset);
        let mut at: Vec<f32> = Vec::with_capacity(peers.len());
        for &j in &peers {
            let c = get(ends[j]).max(at.last().map_or(f32::NEG_INFINITY, |p| p + PORT_GAP));
            at.push(c);
        }
        let over = at.last().map_or(0.0, |last| (last - hi).max(0.0));
        for (&j, c) in peers.iter().zip(at) {
            let c = (c - over).max(lo);
            if along_x { ends[j].x = c } else { ends[j].y = c }
        }
    }
}

/// `p` moved onto `side` of `r` (level with it), kept off the corners.
fn project(r: Rectangle, side: Side, p: Point) -> Point {
    let clamp = |v: f32, lo: f32, len: f32| v.clamp(lo + INSET.min(len / 2.0), lo + len - INSET.min(len / 2.0));
    match side {
        Side::Left => Point::new(r.x, clamp(p.y, r.y, r.height)),
        Side::Right => Point::new(r.x + r.width, clamp(p.y, r.y, r.height)),
        Side::Top => Point::new(clamp(p.x, r.x, r.width), r.y),
        Side::Bottom => Point::new(clamp(p.x, r.x, r.width), r.y + r.height),
    }
}

/// One arrow to lay out, in canvas units: its source (an exit element, or
/// the whole board) on board `board`, and its target board (`target` names
/// it, so arrows into one side spread along it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrow {
    pub board: Rectangle,
    pub from: Rectangle,
    pub to: Rectangle,
    pub target: usize,
}

/// A laid-out arrow, in canvas units: port to port through the gutters
/// (axis-aligned runs).
#[derive(Debug, Clone, PartialEq)]
pub struct Wire {
    pub path: Vec<Point>,
}

/// Connectors for `arrows` around `boards` (every board on the canvas).
pub fn lay_out(boards: &[Rectangle], arrows: &[Arrow]) -> Vec<Wire> {
    let sides: Vec<(Side, Side)> = arrows.iter().map(|a| sides(a.board, a.to)).collect();
    let starts: Vec<Point> = arrows
        .iter()
        .zip(&sides)
        .map(|(a, (out, _))| project(a.board, *out, a.from.center()))
        .collect();
    // Each arrow enters level with where it leaves when the side allows (a
    // straight run between neighbours); ports into one side keep apart.
    let mut ends: Vec<Point> = (0..arrows.len())
        .map(|i| project(arrows[i].to, sides[i].1, starts[i]))
        .collect();
    space_ports(arrows, &sides, &mut ends);
    let stub = |p: Point, side: Side| p + side.out() * CLEAR;
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for b in boards {
        xs.extend([b.x - CLEAR, b.x + b.width + CLEAR]);
        ys.extend([b.y - CLEAR, b.y + b.height + CLEAR]);
    }
    for i in 0..arrows.len() {
        for (p, side) in [(starts[i], sides[i].0), (ends[i], sides[i].1)] {
            let s = stub(p, side);
            xs.push(s.x);
            ys.push(s.y);
        }
    }
    let grid = Grid::new(xs, ys, boards);
    (0..arrows.len())
        .map(|i| {
            let (out, into) = sides[i];
            let (a, b) = (stub(starts[i], out), stub(ends[i], into));
            let middle = grid.path(a, out, b, into).unwrap_or_else(|| vec![a, b]);
            let mut path = vec![starts[i]];
            path.extend(middle);
            path.push(ends[i]);
            Wire {
                path: simplify(path),
            }
        })
        .collect()
}

/// Drop repeated and collinear points.
fn simplify(path: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(path.len());
    for p in path {
        if out.last().is_some_and(|q| q.distance(p) < 0.01) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let same_x = (a.x - b.x).abs() < 0.01 && (b.x - p.x).abs() < 0.01;
            let same_y = (a.y - b.y).abs() < 0.01 && (b.y - p.y).abs() < 0.01;
            if same_x || same_y {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// Grid of candidate points for orthogonal routing: every crossing of the
/// lines, minus those within [`CLEAR`] of a board; neighbours along a line
/// when the run between them stays clear.
struct Grid {
    xs: Vec<f32>,
    ys: Vec<f32>,
    /// Per node, the neighbour in each direction (+x, −x, +y, −y).
    next: Vec<[Option<usize>; 4]>,
}

impl Grid {
    fn new(mut xs: Vec<f32>, mut ys: Vec<f32>, boards: &[Rectangle]) -> Self {
        let dedup = |v: &mut Vec<f32>| {
            v.retain(|x| x.is_finite());
            v.sort_by(f32::total_cmp);
            v.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        };
        dedup(&mut xs);
        dedup(&mut ys);
        let blocks: Vec<Rectangle> = boards.iter().map(|b| b.expand(CLEAR - 1.0)).collect();
        let inside = |x: f32, y: f32| {
            blocks.iter().any(|b| x > b.x && x < b.x + b.width && y > b.y && y < b.y + b.height)
        };
        let (nx, ny) = (xs.len(), ys.len());
        let open: Vec<bool> = (0..nx * ny).map(|n| !inside(xs[n % nx], ys[n / nx])).collect();
        let run_clear = |a: (f32, f32), b: (f32, f32)| {
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
            !blocks.iter().any(|r| x0 < r.x + r.width && x1 > r.x && y0 < r.y + r.height && y1 > r.y)
        };
        let mut next = vec![[None; 4]; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                let n = j * nx + i;
                if !open[n] {
                    continue;
                }
                if i + 1 < nx && open[n + 1] && run_clear((xs[i], ys[j]), (xs[i + 1], ys[j])) {
                    next[n][0] = Some(n + 1);
                    next[n + 1][1] = Some(n);
                }
                if j + 1 < ny && open[n + nx] && run_clear((xs[i], ys[j]), (xs[i], ys[j + 1])) {
                    next[n][2] = Some(n + nx);
                    next[n + nx][3] = Some(n);
                }
            }
        }
        Self { xs, ys, next }
    }

    fn node(&self, p: Point) -> Option<usize> {
        let i = self.xs.iter().position(|x| (x - p.x).abs() < 0.5)?;
        let j = self.ys.iter().position(|y| (y - p.y).abs() < 0.5)?;
        let n = j * self.xs.len() + i;
        self.next[n].iter().any(Option::is_some).then_some(n)
    }

    fn point(&self, n: usize) -> Point {
        Point::new(self.xs[n % self.xs.len()], self.ys[n / self.xs.len()])
    }

    /// Cheapest run from `a` (leaving out of side `out`) to `b` (arriving to
    /// go into side `into`): length plus [`BEND`] per turn.
    fn path(&self, a: Point, out: Side, b: Point, into: Side) -> Option<Vec<Point>> {
        let (start, goal) = (self.node(a)?, self.node(b)?);
        let inward = into.dir() ^ 1;
        let mut cost = vec![f32::INFINITY; self.next.len() * 4];
        let mut prev = vec![usize::MAX; self.next.len() * 4];
        let mut heap = BinaryHeap::new();
        let first = start * 4 + out.dir();
        cost[first] = 0.0;
        heap.push((Reverse(0u64), first));
        let mut best: Option<(f32, usize)> = None;
        while let Some((Reverse(c), state)) = heap.pop() {
            let c = f32::from_bits(c as u32);
            if c > cost[state] || best.is_some_and(|(b, _)| c >= b) {
                continue;
            }
            let (n, dir) = (state / 4, state % 4);
            if n == goal {
                let total = c + if dir == inward { 0.0 } else { BEND };
                if best.is_none_or(|(b, _)| total < b) {
                    best = Some((total, state));
                }
                continue;
            }
            for (d, m) in self.next[n].iter().enumerate() {
                let Some(m) = *m else { continue };
                if d == dir ^ 1 {
                    continue;
                }
                let step = self.point(n).distance(self.point(m)) + if d == dir { 0.0 } else { BEND };
                let s = m * 4 + d;
                if c + step < cost[s] {
                    cost[s] = c + step;
                    prev[s] = state;
                    // Non-negative f32 bits order like the values.
                    heap.push((Reverse(u64::from(cost[s].to_bits())), s));
                }
            }
        }
        let (_, mut state) = best?;
        let mut out = vec![self.point(state / 4)];
        while prev[state] != usize::MAX {
            state = prev[state];
            out.push(self.point(state / 4));
        }
        out.reverse();
        Some(out)
    }
}

/// Spread runs that share a gutter line: overlapping ones (screen px,
/// axis-aligned) are moved apart by `gap`. A path's first and last runs
/// (port to stub) stay on their ports.
pub fn nudge(paths: &mut [Vec<Point>], gap: f32) {
    // (horizontal, line, from, to, path, run)
    let mut runs: Vec<(bool, f32, f32, f32, usize, usize)> = Vec::new();
    for (w, p) in paths.iter().enumerate() {
        for k in 1..p.len().saturating_sub(2) {
            let (a, b) = (p[k], p[k + 1]);
            if (a.y - b.y).abs() < 0.01 {
                runs.push((true, a.y, a.x.min(b.x), a.x.max(b.x), w, k));
            } else if (a.x - b.x).abs() < 0.01 {
                runs.push((false, a.x, a.y.min(b.y), a.y.max(b.y), w, k));
            }
        }
    }
    runs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)));
    let mut i = 0;
    while i < runs.len() {
        let mut j = i;
        while j < runs.len() && runs[j].0 == runs[i].0 && (runs[j].1 - runs[i].1).abs() < 0.5 {
            j += 1;
        }
        // Lanes: each run takes the first lane whose last run ended before it.
        let mut lane_end: Vec<f32> = Vec::new();
        let mut lanes = Vec::with_capacity(j - i);
        for r in &runs[i..j] {
            let lane = match lane_end.iter().position(|end| *end < r.2 - 0.5) {
                Some(l) => l,
                None => {
                    lane_end.push(f32::NEG_INFINITY);
                    lane_end.len() - 1
                }
            };
            lane_end[lane] = r.3;
            lanes.push(lane);
        }
        let count = lane_end.len() as f32;
        for (r, lane) in runs[i..j].iter().zip(lanes) {
            let off = (lane as f32 - (count - 1.0) / 2.0) * gap;
            let p = &mut paths[r.4];
            for q in [r.5, r.5 + 1] {
                if r.0 { p[q].y += off } else { p[q].x += off }
            }
        }
        i = j;
    }
}

/// Where a label of `size` goes on `path` (screen px): on its longest run
/// (middle, then quarters), else the next, at the first spot that covers
/// neither a board (`boards`) nor a label already `placed` (it is added).
/// `None` when nothing fits — unless `force` (a lit arrow's label is shown
/// anyway, on its longest run).
pub fn label_spot(
    path: &[Point],
    size: Size,
    placed: &mut Vec<Rectangle>,
    boards: &[Rectangle],
    force: bool,
) -> Option<Point> {
    let mut runs: Vec<(f32, Point, Point)> = path
        .windows(2)
        .enumerate()
        .map(|(k, w)| {
            // Port stubs are short and sit against a board: last resort.
            let inner = k > 0 && k + 2 < path.len();
            (w[0].distance(w[1]) * if inner { 1.0 } else { 0.25 }, w[0], w[1])
        })
        .collect();
    runs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let rect = |c: Point| Rectangle::new(Point::new(c.x - size.width / 2.0, c.y - size.height / 2.0), size);
    let at = |a: Point, b: Point, t: f32| Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
    let free = |r: &Rectangle| {
        !placed.iter().any(|p| p.intersects(r)) && !boards.iter().any(|b| b.intersects(r))
    };
    let spot = runs
        .iter()
        .flat_map(|&(_, a, b)| [0.5, 0.25, 0.75].map(|t| rect(at(a, b, t))))
        .find(free)
        .or_else(|| force.then(|| runs.first().map(|&(_, a, b)| rect(at(a, b, 0.5)))).flatten())?;
    placed.push(spot);
    Some(spot.center())
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

    fn crosses(path: &[Point], b: Rectangle) -> bool {
        path.windows(2).any(|w| {
            let (x0, x1) = (w[0].x.min(w[1].x), w[0].x.max(w[1].x));
            let (y0, y1) = (w[0].y.min(w[1].y), w[0].y.max(w[1].y));
            x0 < b.x + b.width - 0.5 && x1 > b.x + 0.5 && y0 < b.y + b.height - 0.5 && y1 > b.y + 0.5
        })
    }

    #[test]
    fn sides_cross_the_wider_gap() {
        assert_eq!(sides(r(0., 0., 100., 100.), r(300., 50., 100., 100.)), (Side::Right, Side::Left));
        assert_eq!(sides(r(300., 0., 100., 100.), r(0., 0., 100., 100.)), (Side::Left, Side::Right));
        assert_eq!(sides(r(0., 0., 100., 100.), r(20., 400., 100., 100.)), (Side::Bottom, Side::Top));
        assert_eq!(sides(r(0., 0., 100., 100.), r(150., 500., 100., 100.)), (Side::Bottom, Side::Top));
    }

    #[test]
    fn a_wire_goes_around_the_board_between() {
        // Three boards in a row; an exit in the first leads to the third.
        let boards = [r(0., 0., 400., 300.), r(480., 0., 400., 300.), r(960., 0., 400., 300.)];
        let exit = r(100., 200., 60., 20.);
        let w = &lay_out(&boards, &[Arrow { board: boards[0], from: exit, to: boards[2], target: 2 }])[0];
        let (first, last) = (w.path[0], *w.path.last().unwrap());
        assert_eq!(first, Point::new(400., 210.), "a port on the board's side, level with the exit");
        assert_eq!(last, Point::new(960., 210.), "into the target's facing side, level");
        for b in &boards {
            assert!(!crosses(&w.path, *b), "crosses {b:?}: {:?}", w.path);
        }
        for pair in w.path.windows(2) {
            assert!(pair[0].x == pair[1].x || pair[0].y == pair[1].y, "not orthogonal: {:?}", w.path);
        }
    }

    #[test]
    fn neighbours_go_straight_across_and_ports_keep_apart() {
        let boards = [r(0., 0., 400., 300.), r(480., 0., 400., 300.)];
        let exit = |y: f32| Arrow { board: boards[0], from: r(10., y, 10., 10.), to: boards[1], target: 1 };
        let wires = lay_out(&boards, &[exit(245.), exit(15.), exit(250.)]);
        assert_eq!(wires[0].path, vec![Point::new(400., 250.), Point::new(480., 250.)], "straight");
        assert_eq!(wires[1].path[0].y, 24.0, "kept off the corner");
        assert_eq!(wires[1].path.len(), 2);
        // Two exits level with each other: the second port moves along.
        let (a, b) = (wires[0].path.last().unwrap().y, wires[2].path.last().unwrap().y);
        assert!((a - b).abs() >= 24.0 && a.max(b) <= 276.0, "{a} {b}");
    }

    #[test]
    fn whole_board_links_go_straight_across_the_gap() {
        let boards = [r(0., 0., 400., 300.), r(0., 500., 400., 300.)];
        let w = &lay_out(&boards, &[Arrow { board: boards[0], from: boards[0], to: boards[1], target: 1 }])[0];
        assert_eq!(w.path, vec![Point::new(200., 300.), Point::new(200., 500.)]);
    }

    #[test]
    fn shared_runs_move_apart_but_ports_stay() {
        let p = |pts: &[(f32, f32)]| pts.iter().map(|&(x, y)| Point::new(x, y)).collect::<Vec<_>>();
        let mut paths = vec![
            p(&[(0., 0.), (10., 0.), (10., 50.), (90., 50.), (90., 100.), (100., 100.)]),
            p(&[(0., 20.), (20., 20.), (20., 50.), (80., 50.), (80., 120.), (100., 120.)]),
        ];
        nudge(&mut paths, 6.0);
        assert_eq!((paths[0][3].y - paths[1][3].y).abs(), 6.0, "{paths:?}");
        assert_eq!(paths[0][0], Point::new(0., 0.));
        assert_eq!(paths[1][5], Point::new(100., 120.));
    }

    #[test]
    fn labels_take_long_runs_and_keep_off_boards_and_each_other() {
        let path = [Point::new(0., 0.), Point::new(20., 0.), Point::new(20., 200.), Point::new(40., 200.)];
        let size = Size::new(60., 18.);
        let mut placed = Vec::new();
        assert_eq!(label_spot(&path, size, &mut placed, &[], false), Some(Point::new(20., 100.)));
        assert_eq!(label_spot(&path, size, &mut placed, &[], false), Some(Point::new(20., 50.)), "a quarter along");
        // A board beside the long run: the label does not cover it…
        let board = r(30., 0., 400., 300.);
        assert_eq!(label_spot(&path, size, &mut Vec::new(), &[board], false), None);
        // …unless its arrow is lit.
        assert_eq!(label_spot(&path, size, &mut Vec::new(), &[board], true), Some(Point::new(20., 100.)));
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
    fn cards_flip_and_stay_on_the_plane() {
        let plane = Size::new(800., 600.);
        let size = Size::new(300., 100.);
        assert_eq!(card_at(Point::new(100., 100.), size, plane), Point::new(114., 88.));
        assert_eq!(card_at(Point::new(700., 590.), size, plane).x, 700. - 14. - 300.);
        assert_eq!(card_at(Point::new(700., 590.), size, plane).y, 600. - 100. - 8.);
    }
}
