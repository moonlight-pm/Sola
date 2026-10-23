//! Where a floating frame's edges resize it, and the cursor each edge shows.

use iced::advanced::mouse;
use iced::window::Direction;
use iced::{Point, Rectangle};

/// Inward band along a straight edge.
const EDGE: f32 = 8.0;
/// Square corner cell: the corner radius, so the ear outside the curve
/// still belongs to this window and the close dot (13 px in) stays clear.
const CORNER: f32 = 12.0;

/// The resize direction `p` falls in, if it is on an edge of `bounds`.
pub(super) fn zone(bounds: Rectangle, p: Point) -> Option<Direction> {
    zone_with(bounds, p, EDGE, CORNER)
}

fn zone_with(bounds: Rectangle, p: Point, edge: f32, corner: f32) -> Option<Direction> {
    let dl = p.x - bounds.x;
    let dr = bounds.x + bounds.width - p.x;
    let dt = p.y - bounds.y;
    let db = bounds.y + bounds.height - p.y;
    if dl < 0.0 || dr < 0.0 || dt < 0.0 || db < 0.0 {
        return None;
    }
    let (l, r, t, b) = (dl < corner, dr < corner, dt < corner, db < corner);
    if t && l {
        return Some(Direction::NorthWest);
    }
    if t && r {
        return Some(Direction::NorthEast);
    }
    if b && l {
        return Some(Direction::SouthWest);
    }
    if b && r {
        return Some(Direction::SouthEast);
    }
    if dt < edge {
        return Some(Direction::North);
    }
    if db < edge {
        return Some(Direction::South);
    }
    if dl < edge {
        return Some(Direction::West);
    }
    if dr < edge {
        return Some(Direction::East);
    }
    None
}

/// Col/Row resize rather than Horizontally/Vertically: iced maps the latter
/// to `ew-resize` / `ns-resize`, which McMojave lacks (wlroots then falls
/// back to the arrow).
pub(super) fn interaction(dir: Direction) -> mouse::Interaction {
    match dir {
        Direction::North | Direction::South => mouse::Interaction::ResizingRow,
        Direction::East | Direction::West => mouse::Interaction::ResizingColumn,
        Direction::NorthWest | Direction::SouthEast => mouse::Interaction::ResizingDiagonallyDown,
        Direction::NorthEast | Direction::SouthWest => mouse::Interaction::ResizingDiagonallyUp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b() -> Rectangle {
        Rectangle {
            x: 100.0,
            y: 50.0,
            width: 400.0,
            height: 300.0,
        }
    }

    #[test]
    fn edges_corners_and_the_middle() {
        let b = b();
        // Direction has no PartialEq.
        assert!(matches!(
            zone(b, Point::new(b.x + b.width - 2.0, b.y + b.height / 2.0)),
            Some(Direction::East)
        ));
        assert!(matches!(
            zone(b, Point::new(b.x + 2.0, b.y + b.height / 2.0)),
            Some(Direction::West)
        ));
        assert!(matches!(
            zone(b, Point::new(b.x + b.width - 1.0, b.y + b.height - 1.0)),
            Some(Direction::SouthEast)
        ));
        assert!(matches!(
            zone(b, Point::new(b.x + 4.0, b.y + 4.0)),
            Some(Direction::NorthWest)
        ));
        assert!(zone(b, Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0)).is_none());
        assert!(zone(b, Point::new(b.x - 1.0, b.y + 10.0)).is_none());
    }

    #[test]
    fn the_corner_ear_resizes_though_it_is_outside_the_curve() {
        let b = b();
        let ear = Point::new(b.x + b.width - 1.0, b.y + b.height - 1.0);
        assert!(matches!(zone(b, ear), Some(Direction::SouthEast)));
        let dist = crate::components::float_clip::rounded_rect_dist(ear, b, super::super::RADIUS);
        assert!(dist >= 0.5, "the ear is outside the face (dist={dist})");
    }
}
