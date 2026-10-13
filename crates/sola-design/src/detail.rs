//! Detail renders: past a board's raster scale, the part of it on screen is
//! re-rastered at the screen's scale (`Cmd::BoardDetail`; see
//! `sola_browser::cef::board`). No relayout, so text holds still; the
//! board's own texture keeps zoom and pan immediate, and the detail lands
//! over it once the camera rests for [`SETTLE`].
//!
//! Each request has a `seq`; a frame carries the `seq` it shows, so the
//! plane knows whether a frame is the whole board or a detail, and where a
//! detail goes.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::{Point, Rectangle, Size};
use sola_browser::engine::{BoardDetail, TabId};

use crate::boards::{self, Board};
use crate::camera::Camera;

/// Camera rest before a detail is asked for.
pub const SETTLE: Duration = Duration::from_millis(120);
/// Extra board around the visible part, in screen px, so a small pan stays
/// covered.
const MARGIN_PX: f64 = 192.0;

/// What the app sends for one board.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// Widen the board's surface to the plane first (a detail needs the
    /// room): `SetBoard` with this size.
    Grow {
        tab: TabId,
        size: sola_browser::engine::BoardSize,
    },
    /// `BoardDetail`.
    View {
        tab: TabId,
        seq: u32,
        detail: Option<BoardDetail>,
    },
}

/// One request as the plane needs it: frames tagged `seq` show the whole
/// board (`None`) or `detail`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub seq: u32,
    pub detail: Option<BoardDetail>,
}

/// What the camera asks of one board.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Plan {
    /// At or under the raster scale, or off screen: the board's texture.
    Whole,
    /// `visible` (board-local CSS) needs a detail; `detail` covers it.
    Detail {
        visible: Rectangle,
        detail: BoardDetail,
    },
}

/// Screen px per CSS px.
fn screen_scale(camera: &Camera, sf: f32) -> f64 {
    f64::from(camera.zoom) * f64::from(sf)
}

/// Board-local CSS rect of `board` inside the plane, if any.
fn visible_part(board: Rectangle, camera: &Camera, plane: Size) -> Option<Rectangle> {
    let tl = camera.to_canvas(Point::ORIGIN);
    let br = camera.to_canvas(Point::new(plane.width, plane.height));
    let view = Rectangle::new(tl, Size::new(br.x - tl.x, br.y - tl.y));
    let part = board.intersection(&view)?;
    (part.width > 0.0 && part.height > 0.0)
        .then(|| Rectangle::new(Point::new(part.x - board.x, part.y - board.y), part.size()))
}

/// The surface does not hold the visible part (plus margin) at the
/// screen's scale.
fn needs_room(board: &Board, camera: &Camera, plane: Size, sf: f32) -> bool {
    let k = screen_scale(camera, sf);
    if k <= board.scale * 1.001 {
        return false;
    }
    let Some(v) = visible_part(board.rect(), camera, plane) else {
        return false;
    };
    let margin = 2.0 * MARGIN_PX / k;
    let need = |len: f32, side: u32| (f64::from(len) + margin).min(f64::from(side)) * k;
    need(v.width, board.art.width) > f64::from(board.surface.0)
        || need(v.height, board.art.height) > f64::from(board.surface.1)
}

fn plan(board: &Board, camera: &Camera, plane: Size, sf: f32) -> Plan {
    let k = screen_scale(camera, sf);
    if k <= board.scale * 1.001 {
        return Plan::Whole;
    }
    let rect = board.rect();
    let Some(visible) = visible_part(rect, camera, plane) else {
        return Plan::Whole;
    };
    let (w, h) = (f64::from(board.art.width), f64::from(board.art.height));
    let margin = MARGIN_PX / k;
    let span = |lo: f32, len: f32, cap: f64, side: f64| -> f64 {
        let (lo, hi) = (f64::from(lo), f64::from(lo + len));
        let (a, b) = ((lo - margin).max(0.0), (hi + margin).min(side));
        if b - a <= cap {
            a
        } else {
            // More on screen than the surface holds: center on the view.
            ((lo + hi) / 2.0 - cap / 2.0).clamp(0.0, (side - cap).max(0.0))
        }
    };
    let x = span(visible.x, visible.width, f64::from(board.surface.0) / k, w);
    let y = span(visible.y, visible.height, f64::from(board.surface.1) / k, h);
    // Start on a whole screen pixel (rounding right keeps x, y ≥ 0, which
    // Chromium needs), so detail texels land on pixels.
    let align = |css: f64, offset: f32, at: f32| -> f64 {
        let s = f64::from(sf);
        let px = (f64::from(offset) + (f64::from(at) + css) * f64::from(camera.zoom)) * s;
        let snapped = (px - 1e-6).ceil();
        ((snapped / s - f64::from(offset)) / f64::from(camera.zoom) - f64::from(at)).max(0.0)
    };
    Plan::Detail {
        visible,
        detail: BoardDetail {
            x: align(x, camera.offset.x, rect.x),
            y: align(y, camera.offset.y, rect.y),
            scale: k,
        },
    }
}

/// `held` still serves: same scale, covers `visible`, and still lands on
/// whole screen pixels.
fn still_serves(
    held: &BoardDetail,
    visible: Rectangle,
    board: &Board,
    camera: &Camera,
    sf: f32,
) -> bool {
    let k = screen_scale(camera, sf);
    if (held.scale - k).abs() > k * 1e-4 {
        return false;
    }
    let (cw, ch) = (
        f64::from(board.surface.0) / k,
        f64::from(board.surface.1) / k,
    );
    let (vx, vy) = (f64::from(visible.x), f64::from(visible.y));
    let (vr, vb) = (
        vx + f64::from(visible.width),
        vy + f64::from(visible.height),
    );
    let covers = held.x <= vx + 1e-3
        && held.y <= vy + 1e-3
        && held.x + cw >= vr.min(f64::from(board.art.width)) - 1e-3
        && held.y + ch >= vb.min(f64::from(board.art.height)) - 1e-3;
    let on_pixel = |css: f64, offset: f32, at: f32| -> bool {
        let px =
            (f64::from(offset) + (f64::from(at) + css) * f64::from(camera.zoom)) * f64::from(sf);
        (px - px.round()).abs() < 0.02
    };
    let rect = board.rect();
    covers && on_pixel(held.x, camera.offset.x, rect.x) && on_pixel(held.y, camera.offset.y, rect.y)
}

/// Per board: the request in force, the one before (its frames may still
/// arrive), and how many agent captures hold it.
#[derive(Debug, Clone, Copy)]
struct Track {
    now: View,
    before: Option<View>,
    /// Agent screenshots / PDFs in flight. The board shows its whole self
    /// to them, untouched by the camera: no requests while held (the old
    /// detail stays drawn).
    held: u32,
}

#[derive(Debug, Default)]
pub struct Details {
    next_seq: u32,
    tabs: HashMap<u64, Track>,
    /// When to (re)plan: the camera, plane, or boards changed.
    pub due: Option<Instant>,
}

impl Details {
    /// Plan again once things rest.
    pub fn poke(&mut self, now: Instant) {
        self.due = Some(now + SETTLE);
    }

    pub fn forget(&mut self, tab: u64) {
        self.tabs.remove(&tab);
    }

    pub fn clear(&mut self) {
        self.tabs.clear();
        self.due = None;
    }

    /// A new request for `tab`. After a surface grow (`fresh`) the requests
    /// before it are forgotten: their frames at the new size are only the
    /// resize's stopgap.
    fn issue(&mut self, tab: u64, detail: Option<BoardDetail>, fresh: bool) -> Ask {
        self.next_seq += 1;
        let seq = self.next_seq;
        let view = View { seq, detail };
        let held = self.tabs.get(&tab).map_or(0, |t| t.held);
        let before = if fresh {
            None
        } else {
            self.tabs.get(&tab).map(|t| t.now)
        };
        self.tabs.insert(
            tab,
            Track {
                now: view,
                before,
                held,
            },
        );
        Ask::View {
            tab: TabId(tab),
            seq,
            detail,
        }
    }

    /// Asks that bring every board in line with the camera. `cap` is the
    /// plane in px (a grown surface's floor). The app applies a `Grow` to
    /// its board before the next ask.
    pub fn refresh(
        &mut self,
        boards: &[Board],
        camera: &Camera,
        plane: Size,
        sf: f32,
        cap: (u32, u32),
    ) -> Vec<Ask> {
        self.due = None;
        let mut out = Vec::new();
        for b in boards {
            let tab = b.tab.0;
            if self.tabs.get(&tab).is_some_and(|t| t.held > 0) {
                continue;
            }
            let mut grown = None;
            if !b.grown && needs_room(b, camera, plane, sf) {
                let size = boards::board_size(&b.art, b.scale, cap);
                if size.surface != b.surface {
                    grown = Some(Board {
                        surface: size.surface,
                        grown: true,
                        ..b.clone()
                    });
                    out.push(Ask::Grow { tab: b.tab, size });
                }
            }
            let fresh = grown.is_some();
            let b = grown.as_ref().unwrap_or(b);
            let held = self.tabs.get(&tab).copied();
            let held_detail = held.and_then(|t| t.now.detail);
            match plan(b, camera, plane, sf) {
                Plan::Whole => {
                    if held_detail.is_some() {
                        out.push(self.issue(tab, None, fresh));
                    }
                }
                Plan::Detail { visible, detail } => {
                    let keep = !fresh
                        && held_detail.is_some_and(|d| still_serves(&d, visible, b, camera, sf));
                    if !keep {
                        out.push(self.issue(tab, Some(detail), fresh));
                    }
                }
            }
        }
        out
    }

    /// An agent capture of `tab` starts: the page shows its whole self and
    /// no camera move changes that until [`Details::release`]. The detail on
    /// screen stays drawn. The ask (when a detail was up) goes out before
    /// the capture.
    pub fn hold(&mut self, tab: u64) -> Option<Ask> {
        let t = self.tabs.entry(tab).or_insert(Track {
            now: View {
                seq: 0,
                detail: None,
            },
            before: None,
            held: 0,
        });
        t.held += 1;
        t.now.detail?;
        Some(self.issue(tab, None, false))
    }

    /// `tab` goes off screen: back to the whole board (the track stays, so
    /// its frames stay known). `None` when no detail was up.
    pub fn retire(&mut self, tab: u64) -> Option<Ask> {
        self.tabs.get(&tab)?.now.detail?;
        Some(self.issue(tab, None, false))
    }

    /// The capture finished; plan again (the app pokes).
    pub fn release(&mut self, tab: u64) {
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.held = t.held.saturating_sub(1);
        }
    }

    /// Requests the plane knows how to draw for `tab`, and whether its
    /// detail is up.
    pub fn plane_views(&self, tab: u64) -> ([Option<View>; 2], bool) {
        match self.tabs.get(&tab) {
            Some(t) => (
                [Some(t.now), t.before],
                t.now.detail.is_some() || t.held > 0,
            ),
            None => ([None, None], false),
        }
    }

    /// The detail in force for `tab` (status).
    pub fn current(&self, tab: u64) -> Option<BoardDetail> {
        self.tabs.get(&tab).and_then(|t| t.now.detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Artboard;
    use iced::Vector;

    fn board(w: u32, h: u32, scale: f64, surface: (u32, u32)) -> Board {
        Board {
            tab: TabId(7),
            art: Artboard {
                id: "a".into(),
                title: "a".into(),
                url: "u".into(),
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
                auto_height: false,
            },
            scale,
            surface,
            grown: false,
            error: None,
        }
    }

    fn cam(zoom: f32, ox: f32, oy: f32) -> Camera {
        Camera {
            offset: Vector::new(ox, oy),
            zoom,
        }
    }

    const PLANE: Size = Size {
        width: 1000.0,
        height: 800.0,
    };
    const CAP: (u32, u32) = (1024, 1024);

    fn views(asks: &[Ask]) -> Vec<(u32, Option<BoardDetail>)> {
        asks.iter()
            .filter_map(|a| match a {
                Ask::View { seq, detail, .. } => Some((*seq, *detail)),
                Ask::Grow { .. } => None,
            })
            .collect()
    }

    #[test]
    fn no_detail_at_or_under_the_raster_scale() {
        let b = board(1440, 900, 2.0, (2880, 1800));
        let mut d = Details::default();
        assert!(
            d.refresh(&[b.clone()], &cam(2.0, 0.0, 0.0), PLANE, 1.0, CAP)
                .is_empty()
        );
        assert!(
            d.refresh(&[b], &cam(0.5, 0.0, 0.0), PLANE, 1.0, CAP)
                .is_empty()
        );
    }

    #[test]
    fn past_the_raster_scale_the_visible_part_is_asked_for() {
        let b = board(1440, 900, 2.0, (2880, 1800));
        let mut d = Details::default();
        // 4×, board origin 400 px left of the plane.
        let c = cam(4.0, -400.0, 0.0);
        let asks = d.refresh(&[b.clone()], &c, PLANE, 1.0, CAP);
        let [
            Ask::View {
                tab,
                seq,
                detail: Some(det),
            },
        ] = asks[..]
        else {
            panic!("{asks:?}")
        };
        assert_eq!((tab, seq), (TabId(7), 1));
        assert_eq!(det.scale, 4.0);
        // Visible from x = 100 CSS; the margin reaches 48 CSS further left.
        assert!((det.x - 52.0).abs() < 1e-6, "{det:?}");
        assert_eq!(det.y, 0.0);
        // Resting camera: nothing new.
        assert!(d.refresh(&[b.clone()], &c, PLANE, 1.0, CAP).is_empty());
        // A small pan stays covered.
        assert!(
            d.refresh(&[b.clone()], &cam(4.0, -440.0, 0.0), PLANE, 1.0, CAP)
                .is_empty()
        );
        // A pan by a part of a pixel loses the pixel grid: ask again.
        assert_eq!(
            d.refresh(&[b.clone()], &cam(4.0, -440.5, 0.0), PLANE, 1.0, CAP)
                .len(),
            1
        );
        // Zoom back out: the whole board again.
        let asks = d.refresh(&[b], &cam(1.0, 0.0, 0.0), PLANE, 1.0, CAP);
        assert!(matches!(views(&asks)[..], [(_, None)]), "{asks:?}");
    }

    #[test]
    fn a_small_board_grows_its_surface_first() {
        // A phone board: 780 px of surface, 1560 px of it on screen at 4×.
        let b = board(390, 844, 2.0, (780, 1688));
        let mut d = Details::default();
        let asks = d.refresh(&[b.clone()], &cam(4.0, 0.0, 0.0), PLANE, 1.0, CAP);
        let [
            Ask::Grow { size, .. },
            Ask::View {
                seq,
                detail: Some(_),
                ..
            },
        ] = &asks[..]
        else {
            panic!("{asks:?}")
        };
        assert_eq!(size.surface, (1024, 1688));
        // Frames from before the grow are not drawn.
        let ([now, before], up) = d.plane_views(7);
        assert_eq!(now.unwrap().seq, *seq);
        assert!(before.is_none() && up);
        // Grown: no second grow.
        let grown = Board {
            surface: size.surface,
            grown: true,
            ..b
        };
        let asks = d.refresh(&[grown], &cam(4.0, 0.0, 0.0), PLANE, 1.0, CAP);
        assert!(
            asks.iter().all(|a| !matches!(a, Ask::Grow { .. })),
            "{asks:?}"
        );
    }

    #[test]
    fn a_detail_starts_on_a_screen_pixel() {
        let b = board(1440, 900, 2.0, (2880, 1800));
        let c = cam(3.3, -123.37, 10.2);
        let Plan::Detail { detail, .. } = plan(&b, &c, PLANE, 1.0) else {
            panic!()
        };
        let px = c.offset.x as f64 + detail.x * 3.3;
        assert!((px - px.round()).abs() < 1e-3, "{px}");
        assert!(detail.x >= 0.0 && detail.y >= 0.0);
    }

    #[test]
    fn output_scale_counts() {
        let b = board(1440, 900, 2.0, (2880, 1800));
        assert_eq!(plan(&b, &cam(1.5, 0.0, 0.0), PLANE, 1.0), Plan::Whole);
        assert!(matches!(
            plan(&b, &cam(1.5, 0.0, 0.0), PLANE, 2.0),
            Plan::Detail { .. }
        ));
    }

    #[test]
    fn off_screen_boards_stay_whole() {
        let b = board(100, 100, 2.0, (200, 200));
        assert_eq!(plan(&b, &cam(4.0, 5000.0, 0.0), PLANE, 1.0), Plan::Whole);
    }

    #[test]
    fn a_capture_holds_the_board_whole() {
        let b = board(1440, 900, 2.0, (2880, 1800));
        let mut d = Details::default();
        d.refresh(&[b.clone()], &cam(4.0, 0.0, 0.0), PLANE, 1.0, CAP);
        let Some(Ask::View {
            seq, detail: None, ..
        }) = d.hold(7)
        else {
            panic!()
        };
        let ([now, before], up) = d.plane_views(7);
        assert_eq!(now.unwrap().seq, seq);
        assert!(before.unwrap().detail.is_some());
        assert!(up, "the old detail stays drawn");
        // The human pans and zooms meanwhile: nothing reaches the page.
        assert!(
            d.refresh(&[b.clone()], &cam(6.0, -300.0, 0.0), PLANE, 1.0, CAP)
                .is_empty()
        );
        d.release(7);
        assert_eq!(
            d.refresh(&[b.clone()], &cam(6.0, -300.0, 0.0), PLANE, 1.0, CAP)
                .len(),
            1
        );
        // A board with no detail is held too.
        let other = Board { tab: TabId(9), ..b };
        assert!(d.hold(9).is_none());
        assert!(
            d.refresh(&[other], &cam(4.0, 0.0, 0.0), PLANE, 1.0, CAP)
                .is_empty()
        );
    }
}
