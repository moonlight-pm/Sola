//! Live artboards: one CEF board tab per `canvas.toml` artboard.
//!
//! [`sync`] diffs the open boards against a freshly read layout and says
//! which tabs to open, resize, re-point, or close. Applying the ops to the
//! engine is the app's job.

use sola_browser::engine::{BoardSize, TabId};

use crate::canvas::Artboard;

/// Raster scale a board renders at (through page zoom) when it fits: crisp
/// to 200% at output scale 1 without a detail render. Fixed per board:
/// zooming the canvas only scales textures (no re-raster pop), and past
/// this a detail render sharpens the visible part.
pub const RASTER_SCALE: f64 = 2.0;
/// Largest texture side the plane uploads (wgpu default limit).
const MAX_TEXTURE_SIDE: f64 = 8192.0;
/// Frame budget under the helper → chrome frame IPC cap.
const MAX_FRAME_BYTES: f64 = 160.0 * 1024.0 * 1024.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    pub tab: TabId,
    pub art: Artboard,
    pub scale: f64,
    /// OSR surface in px: the board's pixels, or (once a detail render
    /// needed the room) widened to the plane.
    pub surface: (u32, u32),
    /// The surface is widened to the plane (see [`Board::surface`]).
    pub grown: bool,
    /// Why the board does not show its page (missing `src`, HTTP 404).
    pub error: Option<String>,
}

impl Board {
    /// The surface floor for this board given the plane `cap`.
    pub fn cap(&self, cap: (u32, u32)) -> (u32, u32) {
        if self.grown { cap } else { (0, 0) }
    }

    pub fn size(&self) -> BoardSize {
        BoardSize {
            width: self.art.width,
            height: self.art.height,
            scale: self.scale,
            surface: self.surface,
        }
    }

    pub fn rect(&self) -> iced::Rectangle {
        iced::Rectangle::new(
            iced::Point::new(self.art.x, self.art.y),
            iced::Size::new(self.art.width as f32, self.art.height as f32),
        )
    }

    /// Board-local CSS px for a canvas point, if the point is on the board.
    pub fn local(&self, canvas: iced::Point) -> Option<(f32, f32)> {
        let x = canvas.x - self.art.x;
        let y = canvas.y - self.art.y;
        let inside = x >= 0.0 && y >= 0.0 && x < self.art.width as f32 && y < self.art.height as f32;
        inside.then_some((x, y))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Open {
        tab: TabId,
        url: String,
        title: String,
        size: BoardSize,
    },
    View {
        tab: TabId,
        size: BoardSize,
        /// The CSS size changed (not just scale or surface).
        resized: bool,
    },
    Load {
        tab: TabId,
        url: String,
    },
    Close {
        tab: TabId,
    },
}

/// A `width × height` board at `scale` fits one plane texture and the
/// frame budget.
pub fn fits(width: u32, height: u32, scale: f64) -> bool {
    let (w, h) = (f64::from(width) * scale, f64::from(height) * scale);
    w <= MAX_TEXTURE_SIDE && h <= MAX_TEXTURE_SIDE && w * h * 4.0 <= MAX_FRAME_BYTES
}

/// Raster scale for one board: [`RASTER_SCALE`], else 1×, else the largest
/// hundredth that fits (a page taller than the texture side). A board under
/// 2× is still sharp zoomed in: detail renders cover it.
pub fn raster_scale(width: u32, height: u32) -> f64 {
    if fits(width, height, RASTER_SCALE) {
        return RASTER_SCALE;
    }
    if fits(width, height, 1.0) {
        return 1.0;
    }
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let s = (MAX_TEXTURE_SIDE / w)
        .min(MAX_TEXTURE_SIDE / h)
        .min((MAX_FRAME_BYTES / 4.0 / (w * h)).sqrt());
    ((s * 100.0).floor() / 100.0).max(0.01)
}

/// Geometry for `art` at `scale`, its surface widened to `cap` (the plane
/// in px, for a grown board; else zero).
pub fn board_size(art: &Artboard, scale: f64, cap: (u32, u32)) -> BoardSize {
    let mut size = BoardSize {
        width: art.width,
        height: art.height,
        scale,
        surface: (0, 0),
    };
    let (w, h) = size.pixels();
    size.surface = (w.max(cap.0), h.max(cap.1));
    size
}

/// Host of a URL (`scheme://host[:port]/…` → `host`); `""` for `file://`.
pub fn url_host(url: &str) -> &str {
    let Some((_, rest)) = url.split_once("://") else {
        return "";
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    authority.rsplit_once(':').map_or(authority, |(h, _)| h)
}

/// Raster scale per board, from `(host, width, height)`: its
/// [`raster_scale`], lowered to the smallest on its host. Page zoom is per
/// host in Chromium; served boards each have their own host.
pub fn host_scales(boards: &[(&str, u32, u32)]) -> Vec<f64> {
    let mut least: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    for &(host, w, h) in boards {
        let s = raster_scale(w, h);
        least
            .entry(host)
            .and_modify(|m| *m = m.min(s))
            .or_insert(s);
    }
    boards.iter().map(|(host, ..)| least[host]).collect()
}

/// Diff `current` against `next`. Boards keep their tab (and texture) when
/// the id survives; a changed URL re-points the tab instead of reopening.
/// A surviving board keeps its raster scale (the app re-checks scales after
/// a layout), an `auto` board keeps its measured height, and a grown
/// board keeps its surface widened to `cap`.
pub fn sync(
    current: Vec<Board>,
    next: &[Artboard],
    cap: (u32, u32),
    mut alloc: impl FnMut() -> TabId,
) -> (Vec<Board>, Vec<Op>) {
    let mut ops = Vec::new();
    let mut old = current;
    let mut boards = Vec::with_capacity(next.len());
    for art in next {
        match old.iter().position(|b| b.art.id == art.id) {
            Some(i) => {
                let prev = old.swap_remove(i);
                let tab = prev.tab;
                let mut art = art.clone();
                if art.auto_height && prev.art.auto_height {
                    art.height = prev.art.height;
                }
                if prev.art.url != art.url {
                    ops.push(Op::Load {
                        tab,
                        url: art.url.clone(),
                    });
                }
                let size = board_size(&art, prev.scale, prev.cap(cap));
                if size != prev.size() {
                    let resized = (art.width, art.height) != (prev.art.width, prev.art.height);
                    ops.push(Op::View { tab, size, resized });
                }
                let error = if prev.art.url == art.url { prev.error } else { None };
                boards.push(Board {
                    tab,
                    art,
                    scale: size.scale,
                    surface: size.surface,
                    grown: prev.grown,
                    error,
                });
            }
            None => {
                let size = board_size(art, raster_scale(art.width, art.height), (0, 0));
                let tab = alloc();
                ops.push(Op::Open {
                    tab,
                    url: art.url.clone(),
                    title: art.title.clone(),
                    size,
                });
                boards.push(Board {
                    tab,
                    art: art.clone(),
                    scale: size.scale,
                    surface: size.surface,
                    grown: false,
                    error: None,
                });
            }
        }
    }
    for gone in old {
        ops.push(Op::Close { tab: gone.tab });
    }
    (boards, ops)
}

/// Topmost board under a canvas point (later artboards draw on top).
pub fn hit(boards: &[Board], canvas: iced::Point) -> Option<(&Board, (f32, f32))> {
    boards
        .iter()
        .rev()
        .find_map(|b| b.local(canvas).map(|local| (b, local)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(id: &str, url: &str, w: u32, h: u32) -> Artboard {
        Artboard {
            id: id.into(),
            title: id.into(),
            url: url.into(),
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            auto_height: false,
        }
    }

    fn ids() -> impl FnMut() -> TabId {
        let mut n = 100;
        move || {
            n += 1;
            TabId(n)
        }
    }

    #[test]
    fn scale_is_per_host() {
        // Over the texture side at 2× (and over the frame budget).
        assert!(!fits(1440, 6000, 2.0));
        let scales = host_scales(&[("a.p.localhost", 1440, 900), ("b.p.localhost", 1440, 6000)]);
        assert_eq!(scales, vec![2.0, 1.0]);
        // file:// boards share the empty host, so one tall board drags them.
        assert_eq!(host_scales(&[("", 390, 844), ("", 1440, 6000)]), vec![1.0, 1.0]);
        assert!(host_scales(&[]).is_empty());
    }

    #[test]
    fn a_page_past_the_texture_side_scales_down_to_fit() {
        assert_eq!(raster_scale(1440, 900), 2.0);
        assert_eq!(raster_scale(1440, 8192), 1.0);
        let s = raster_scale(1440, 20000);
        assert!(s < 1.0 && fits(1440, 20000, s), "{s}");
        assert!(!fits(1440, 20000, s + 0.01));
    }

    #[test]
    fn surfaces_widen_to_the_plane() {
        let size = board_size(&art("p", "u", 390, 844), 2.0, (2560, 1536));
        assert_eq!(size.pixels(), (780, 1688));
        assert_eq!(size.surface, (2560, 1688));
        assert_eq!(board_size(&art("d", "u", 1440, 900), 2.0, (0, 0)).surface, (2880, 1800));
    }

    #[test]
    fn hosts_from_urls() {
        assert_eq!(url_host("http://light--home.proj-x.localhost:4321/design/a.html"), "light--home.proj-x.localhost");
        assert_eq!(url_host("https://example.com/x?y"), "example.com");
        assert_eq!(url_host("file:///w/a.html"), "");
        assert_eq!(url_host("nonsense"), "");
    }

    #[test]
    fn resync_keeps_scale_and_measured_auto_height() {
        let mut alloc = ids();
        let mut a = art("a", "file:///a", 100, 900);
        a.auto_height = true;
        let (mut boards, _) = sync(Vec::new(), &[a.clone()], (0, 0), &mut alloc);
        boards[0].scale = 3.0;
        boards[0].art.height = 2400;
        boards[0].surface = (300, 7200);
        let (boards, ops) = sync(boards, &[a], (0, 0), &mut alloc);
        assert!(ops.is_empty(), "{ops:?}");
        assert_eq!(boards[0].art.height, 2400);
        assert_eq!(boards[0].scale, 3.0);
    }

    #[test]
    fn first_sync_opens_everything() {
        let next = [art("a", "file:///a", 100, 100), art("b", "file:///b", 50, 50)];
        let (boards, ops) = sync(Vec::new(), &next, (0, 0), ids());
        assert_eq!(boards.len(), 2);
        assert!(matches!(ops[0], Op::Open { tab: TabId(101), .. }));
        assert!(matches!(ops[1], Op::Open { tab: TabId(102), .. }));
    }

    #[test]
    fn resync_resizes_repoints_and_closes() {
        let mut alloc = ids();
        let first = [art("a", "file:///a", 100, 100), art("b", "file:///b", 50, 50)];
        let (boards, _) = sync(Vec::new(), &first, (0, 0), &mut alloc);
        let next = [art("a", "file:///a2", 100, 200), art("c", "file:///c", 10, 10)];
        let (boards, ops) = sync(boards, &next, (0, 0), &mut alloc);
        assert_eq!(boards[0].tab, TabId(101));
        let size = |w, h| BoardSize {
            width: w,
            height: h,
            scale: 2.0,
            surface: (w * 2, h * 2),
        };
        assert_eq!(
            ops,
            vec![
                Op::Load {
                    tab: TabId(101),
                    url: "file:///a2".into()
                },
                Op::View {
                    tab: TabId(101),
                    size: size(100, 200),
                    resized: true,
                },
                Op::Open {
                    tab: TabId(103),
                    url: "file:///c".into(),
                    title: "c".into(),
                    size: size(10, 10),
                },
                Op::Close { tab: TabId(102) },
            ]
        );
    }

    #[test]
    fn a_bigger_plane_resizes_grown_surfaces_only() {
        let next = [art("a", "file:///a", 100, 100), art("b", "file:///b", 100, 100)];
        let (mut boards, _) = sync(Vec::new(), &next, (0, 0), ids());
        boards[0].grown = true;
        let (boards, ops) = sync(boards, &next, (640, 400), ids());
        assert_eq!(boards[0].surface, (640, 400));
        assert_eq!(boards[1].surface, (200, 200));
        assert!(matches!(ops[..], [Op::View { resized: false, .. }]), "{ops:?}");
    }

    #[test]
    fn unchanged_layout_is_quiet() {
        let next = [art("a", "file:///a", 100, 100)];
        let (boards, _) = sync(Vec::new(), &next, (0, 0), ids());
        let (_, ops) = sync(boards, &next, (0, 0), ids());
        assert!(ops.is_empty());
    }

    #[test]
    fn hit_prefers_the_later_board() {
        let mut a = art("a", "u", 100, 100);
        let mut b = art("b", "u", 100, 100);
        a.x = 0.0;
        b.x = 50.0;
        let (boards, _) = sync(Vec::new(), &[a, b], (0, 0), ids());
        let (board, local) = hit(&boards, iced::Point::new(60.0, 10.0)).unwrap();
        assert_eq!(board.art.id, "b");
        assert_eq!(local, (10.0, 10.0));
        assert!(hit(&boards, iced::Point::new(200.0, 10.0)).is_none());
    }
}
