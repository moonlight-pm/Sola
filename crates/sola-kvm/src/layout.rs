//! Virtual Mac rectangle math in the primary Sola output coordinate space.
//!
//! See design §3 — bottoms-aligned Mac to the right of novus:
//!
//! ```text
//!   y=-720  ┌─────────────────┐
//!           │  virtual ember  │  2560×2880
//!   y=0     │                 │
//!           │                 ├──────────────────────────────
//!   y=2160  └─────────────────┘         real novus 5120×2160
//!           x=0             x=5120                         x=7680
//! ```

use serde::{Deserialize, Serialize};

/// Which side of the primary output the virtual Mac rect attaches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Right,
    Left,
    Top,
    Bottom,
}

/// Alignment along the shared edge.
///
/// For left/right sides this is vertical placement (top / bottom / center).
/// For top/bottom sides this maps onto horizontal placement:
/// `top` → toward x=0 (start), `bottom` → toward max x (end), `center` mid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Top,
    #[default]
    Bottom,
    Center,
}

/// Computed virtual Mac rectangle + primary output size.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Primary (real) output width in logical pixels.
    pub primary_w: i32,
    /// Primary (real) output height in logical pixels.
    pub primary_h: i32,
    /// Virtual Mac width.
    pub mac_w: i32,
    /// Virtual Mac height.
    pub mac_h: i32,
    /// Origin of the virtual Mac rect in primary space.
    pub origin_x: i32,
    pub origin_y: i32,
    pub side: Side,
    pub align: Align,
    /// Motion scale applied when integrating HID deltas into the virtual cursor.
    pub scale: f32,
}

/// Inputs used to place the virtual rect.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutSpec {
    pub primary_w: i32,
    pub primary_h: i32,
    pub mac_w: i32,
    pub mac_h: i32,
    pub side: Side,
    pub align: Align,
    pub scale: f32,
    /// Manual origin override (when set, side/align only affect edge-hit logic).
    pub offset_x: Option<i32>,
    pub offset_y: Option<i32>,
}

impl Layout {
    /// Place the virtual Mac rect from a layout spec.
    pub fn compute(spec: &LayoutSpec) -> Self {
        let (auto_x, auto_y) = auto_origin(
            spec.primary_w,
            spec.primary_h,
            spec.mac_w,
            spec.mac_h,
            spec.side,
            spec.align,
        );
        Self {
            primary_w: spec.primary_w,
            primary_h: spec.primary_h,
            mac_w: spec.mac_w,
            mac_h: spec.mac_h,
            origin_x: spec.offset_x.unwrap_or(auto_x),
            origin_y: spec.offset_y.unwrap_or(auto_y),
            side: spec.side,
            align: spec.align,
            scale: spec.scale,
        }
    }

    /// Virtual Mac rect right edge (exclusive) in primary space.
    pub fn mac_right(&self) -> i32 {
        self.origin_x + self.mac_w
    }

    /// Virtual Mac rect bottom edge (exclusive) in primary space.
    pub fn mac_bottom(&self) -> i32 {
        self.origin_y + self.mac_h
    }

    /// Convert a primary-space point to Mac-local coords (may be outside [0,W)×[0,H)).
    pub fn to_mac_local(&self, px: i32, py: i32) -> (i32, i32) {
        (px - self.origin_x, py - self.origin_y)
    }

    /// Convert Mac-local coords to primary space.
    pub fn to_primary(&self, mx: i32, my: i32) -> (i32, i32) {
        (mx + self.origin_x, my + self.origin_y)
    }

    /// Whether a primary-space point lies inside the virtual Mac rect
    /// (half-open: `origin ≤ p < origin+size`).
    pub fn contains_primary(&self, px: i32, py: i32) -> bool {
        px >= self.origin_x
            && px < self.mac_right()
            && py >= self.origin_y
            && py < self.mac_bottom()
    }

    /// Whether Mac-local coords are inside the Mac rect.
    pub fn contains_mac_local(&self, mx: i32, my: i32) -> bool {
        mx >= 0 && mx < self.mac_w && my >= 0 && my < self.mac_h
    }

    /// Detect leaving the Mac rect toward the shared edge with the primary.
    ///
    /// Returns `Some(return_point)` in **primary** coords on the real edge
    /// where the local pointer should reappear, or `None` if still remote.
    pub fn leave_toward_primary(&self, mx: i32, my: i32) -> Option<(i32, i32)> {
        // Clamp the along-edge coordinate into the primary range for warp.
        match self.side {
            Side::Right => {
                if mx < 0 {
                    let py = (my + self.origin_y).clamp(0, self.primary_h.saturating_sub(1));
                    Some((self.primary_w.saturating_sub(1), py))
                } else {
                    None
                }
            }
            Side::Left => {
                if mx >= self.mac_w {
                    let py = (my + self.origin_y).clamp(0, self.primary_h.saturating_sub(1));
                    Some((0, py))
                } else {
                    None
                }
            }
            Side::Top => {
                if my >= self.mac_h {
                    let px = (mx + self.origin_x).clamp(0, self.primary_w.saturating_sub(1));
                    Some((px, 0))
                } else {
                    None
                }
            }
            Side::Bottom => {
                if my < 0 {
                    let px = (mx + self.origin_x).clamp(0, self.primary_w.saturating_sub(1));
                    Some((px, self.primary_h.saturating_sub(1)))
                } else {
                    None
                }
            }
        }
    }

    /// Map a local pointer at the primary edge into Mac-local enter coords.
    ///
    /// `local_x`/`local_y` are the last primary coords before leaving
    /// (typically on the shared edge). Returns Mac-local `(mx, my)`.
    pub fn enter_mac_coords(&self, local_x: i32, local_y: i32) -> (i32, i32) {
        let (mut mx, mut my) = self.to_mac_local(local_x, local_y);
        // Snap onto the Mac edge that abuts the primary.
        match self.side {
            Side::Right => {
                mx = 0;
                my = my.clamp(0, self.mac_h.saturating_sub(1));
            }
            Side::Left => {
                mx = self.mac_w.saturating_sub(1);
                my = my.clamp(0, self.mac_h.saturating_sub(1));
            }
            Side::Top => {
                my = self.mac_h.saturating_sub(1);
                mx = mx.clamp(0, self.mac_w.saturating_sub(1));
            }
            Side::Bottom => {
                my = 0;
                mx = mx.clamp(0, self.mac_w.saturating_sub(1));
            }
        }
        (mx, my)
    }

    /// Wire-protocol edge for enter packets (matches layout side).
    pub fn enter_edge(&self) -> crate::protocol::Edge {
        match self.side {
            Side::Left => crate::protocol::Edge::Left,
            Side::Right => crate::protocol::Edge::Right,
            Side::Top => crate::protocol::Edge::Top,
            Side::Bottom => crate::protocol::Edge::Bottom,
        }
    }

    /// Clamp a primary-space point into the real output (inclusive max).
    pub fn clamp_primary(&self, px: i32, py: i32) -> (i32, i32) {
        (
            px.clamp(0, self.primary_w.saturating_sub(1)),
            py.clamp(0, self.primary_h.saturating_sub(1)),
        )
    }

    /// Apply an unscaled relative delta to a primary-space position (local mode).
    pub fn apply_local_motion(&self, px: i32, py: i32, dx: f32, dy: f32) -> (i32, i32) {
        let nx = (px as f32 + dx).round() as i32;
        let ny = (py as f32 + dy).round() as i32;
        (nx, ny)
    }

    /// If a relative motion from primary `(px, py)` would leave the real
    /// output into the virtual Mac rect, return Mac-local enter coords.
    ///
    /// Used while local to detect the edge hit that starts remote mode.
    /// Motion that leaves the primary *away* from the Mac is ignored
    /// (caller should clamp instead).
    pub fn try_enter_from_motion(
        &self,
        px: i32,
        py: i32,
        dx: f32,
        dy: f32,
    ) -> Option<(i32, i32)> {
        let (nx, ny) = self.apply_local_motion(px, py, dx, dy);
        // Still inside primary → no enter.
        if nx >= 0 && nx < self.primary_w && ny >= 0 && ny < self.primary_h {
            return None;
        }
        // Extrapolated point lands in the virtual Mac rect → enter from the
        // last in-primary position (clamped to edge for enter_mac_coords).
        if self.contains_primary(nx, ny) {
            let (ex, ey) = self.clamp_primary(px, py);
            return Some(self.enter_mac_coords(ex, ey));
        }
        // Also accept the shared-edge case where the extrapolated point is
        // just past primary but still maps into Mac via side attachment
        // (e.g. right side at x=primary_w, y within primary that projects
        // into the Mac vertical range).
        match self.side {
            Side::Right if nx >= self.primary_w => {
                let (ex, ey) = self.clamp_primary(px, py);
                let (mx, my) = self.enter_mac_coords(ex, ey);
                // Only enter if the along-edge coordinate projects into Mac.
                if self.contains_mac_local(mx, my) || (my >= 0 && my < self.mac_h) {
                    return Some((mx, my));
                }
            }
            Side::Left if nx < 0 => {
                let (ex, ey) = self.clamp_primary(px, py);
                let (mx, my) = self.enter_mac_coords(ex, ey);
                if my >= 0 && my < self.mac_h {
                    return Some((mx, my));
                }
            }
            Side::Top if ny < 0 => {
                let (ex, ey) = self.clamp_primary(px, py);
                let (mx, my) = self.enter_mac_coords(ex, ey);
                if mx >= 0 && mx < self.mac_w {
                    return Some((mx, my));
                }
            }
            Side::Bottom if ny >= self.primary_h => {
                let (ex, ey) = self.clamp_primary(px, py);
                let (mx, my) = self.enter_mac_coords(ex, ey);
                if mx >= 0 && mx < self.mac_w {
                    return Some((mx, my));
                }
            }
            _ => {}
        }
        None
    }

    /// Integrate a relative HID delta into Mac-local position with motion scale.
    pub fn integrate_motion(&self, mx: i32, my: i32, dx: f32, dy: f32) -> (i32, i32) {
        let sdx = dx * self.scale;
        let sdy = dy * self.scale;
        // Round half-away-from-zero-ish via as-i32 after add; keep f32 accum later if needed.
        let nx = mx as f32 + sdx;
        let ny = my as f32 + sdy;
        (nx.round() as i32, ny.round() as i32)
    }
}

fn auto_origin(
    primary_w: i32,
    primary_h: i32,
    mac_w: i32,
    mac_h: i32,
    side: Side,
    align: Align,
) -> (i32, i32) {
    match side {
        Side::Right => {
            let x = primary_w;
            let y = align_axis(primary_h, mac_h, align);
            (x, y)
        }
        Side::Left => {
            let x = -mac_w;
            let y = align_axis(primary_h, mac_h, align);
            (x, y)
        }
        Side::Top => {
            let y = -mac_h;
            // Shared edge is horizontal: top→start (x=0), bottom→end, center mid.
            let x = align_axis(primary_w, mac_w, align);
            (x, y)
        }
        Side::Bottom => {
            let y = primary_h;
            let x = align_axis(primary_w, mac_w, align);
            (x, y)
        }
    }
}

/// Place `size` within `primary` along one axis per [`Align`].
fn align_axis(primary: i32, size: i32, align: Align) -> i32 {
    match align {
        Align::Top => 0,
        Align::Bottom => primary - size,
        Align::Center => (primary - size) / 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Desk geometry from the design doc.
    fn desk_spec() -> LayoutSpec {
        LayoutSpec {
            primary_w: 5120,
            primary_h: 2160,
            mac_w: 2560,
            mac_h: 2880,
            side: Side::Right,
            align: Align::Bottom,
            scale: 1.0,
            offset_x: None,
            offset_y: None,
        }
    }

    #[test]
    fn bottoms_aligned_right_origin() {
        let layout = Layout::compute(&desk_spec());
        assert_eq!(layout.origin_x, 5120);
        assert_eq!(layout.origin_y, 2160 - 2880); // -720
        assert_eq!(layout.origin_y, -720);
        assert_eq!(layout.mac_right(), 5120 + 2560);
        assert_eq!(layout.mac_bottom(), 2160); // bottoms meet
    }

    #[test]
    fn enter_near_bottom_of_novus_maps_near_bottom_of_mac() {
        let layout = Layout::compute(&desk_spec());
        // Local pointer at right edge near bottom of novus.
        let (mx, my) = layout.enter_mac_coords(5119, 2150);
        assert_eq!(mx, 0);
        // 2150 - (-720) = 2870 → clamp to mac_h-1 = 2879
        assert_eq!(my, 2150 - (-720));
        assert!(my < layout.mac_h);
        assert!(my > layout.mac_h - 50); // near bottom
    }

    #[test]
    fn leave_left_of_mac_returns_to_novus_right_edge() {
        let layout = Layout::compute(&desk_spec());
        let ret = layout.leave_toward_primary(-1, 1000).unwrap();
        assert_eq!(ret.0, 5119); // primary_w - 1
        // my=1000 → primary y = 1000 + (-720) = 280
        assert_eq!(ret.1, 280);
    }

    #[test]
    fn still_remote_inside_mac() {
        let layout = Layout::compute(&desk_spec());
        assert!(layout.leave_toward_primary(10, 10).is_none());
        assert!(layout.contains_mac_local(10, 10));
    }

    #[test]
    fn top_align_right() {
        let mut spec = desk_spec();
        spec.align = Align::Top;
        let layout = Layout::compute(&spec);
        assert_eq!(layout.origin_y, 0);
    }

    #[test]
    fn center_align_right() {
        let mut spec = desk_spec();
        spec.align = Align::Center;
        let layout = Layout::compute(&spec);
        assert_eq!(layout.origin_y, (2160 - 2880) / 2);
    }

    #[test]
    fn manual_offset_override() {
        let mut spec = desk_spec();
        spec.offset_x = Some(5000);
        spec.offset_y = Some(-100);
        let layout = Layout::compute(&spec);
        assert_eq!(layout.origin_x, 5000);
        assert_eq!(layout.origin_y, -100);
    }

    #[test]
    fn integrate_motion_scale() {
        let mut spec = desk_spec();
        spec.scale = 2.0;
        let layout = Layout::compute(&spec);
        let (nx, ny) = layout.integrate_motion(100, 200, 3.0, -4.0);
        assert_eq!(nx, 106); // 100 + 6
        assert_eq!(ny, 192); // 200 - 8
    }

    #[test]
    fn left_side_origin() {
        let mut spec = desk_spec();
        spec.side = Side::Left;
        let layout = Layout::compute(&spec);
        assert_eq!(layout.origin_x, -2560);
        assert_eq!(layout.origin_y, -720);
        let (mx, my) = layout.enter_mac_coords(0, 1000);
        assert_eq!(mx, 2559);
        assert_eq!(my, 1000 - (-720));
    }

    #[test]
    fn try_enter_right_edge_from_motion() {
        let layout = Layout::compute(&desk_spec());
        // Near right edge of novus, small rightward delta leaves into Mac.
        let enter = layout
            .try_enter_from_motion(5119, 2000, 2.0, 0.0)
            .expect("should enter Mac");
        assert_eq!(enter.0, 0);
        // 2000 - (-720) = 2720
        assert_eq!(enter.1, 2720);
    }

    #[test]
    fn try_enter_ignores_inward_motion() {
        let layout = Layout::compute(&desk_spec());
        assert!(layout
            .try_enter_from_motion(5119, 2000, -5.0, 0.0)
            .is_none());
        assert!(layout
            .try_enter_from_motion(100, 100, 1.0, 1.0)
            .is_none());
    }

    #[test]
    fn try_enter_ignores_top_exit_when_mac_is_right() {
        let layout = Layout::compute(&desk_spec());
        // Leave primary upward — Mac is on the right, not above.
        assert!(layout
            .try_enter_from_motion(100, 0, 0.0, -3.0)
            .is_none());
    }

    #[test]
    fn try_enter_left_side() {
        let mut spec = desk_spec();
        spec.side = Side::Left;
        let layout = Layout::compute(&spec);
        let enter = layout
            .try_enter_from_motion(0, 1000, -2.0, 0.0)
            .expect("enter from left edge");
        assert_eq!(enter.0, 2559);
        assert_eq!(enter.1, 1000 - (-720));
    }
}
