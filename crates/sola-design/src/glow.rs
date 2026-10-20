//! Agent glow: a shimmer over what an agent is touching — elements it
//! inspects, the selection it reads, an artboard it screenshots or prints,
//! a board reloading because its files changed. Drawn on the overlay only;
//! it never reaches the pages, so agents see nothing of it.

use std::time::{Duration, Instant};

use iced::widget::canvas::{Frame, Stroke, gradient::Linear};
use iced::{Color, Point, Rectangle, Size, Vector};

use crate::select;

/// An element glow's whole life.
const ELEMENT: Duration = Duration::from_millis(1800);
/// A board glow's life (screenshot / pdf / a look at the whole board).
const BOARD: Duration = Duration::from_millis(1400);
/// Tail after a reloading board settles.
const TAIL: Duration = Duration::from_millis(700);
const FADE_IN: f32 = 0.12;
const FADE_OUT: f32 = 0.4;
/// One shimmer sweep.
const PERIOD: f32 = 1.4;
/// Element glows from one call, at most.
const MAX_ELEMENTS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq)]
enum End {
    At(Instant),
    /// While the board reloads; then [`TAIL`].
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Glow {
    tab: u64,
    /// Board-local CSS rect; `None` is the whole board.
    rect: Option<select::Rect>,
    start: Instant,
    end: End,
}

/// One glow as the overlay draws it this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lit {
    pub tab: u64,
    pub rect: Option<select::Rect>,
    /// 0..1, fades in and out.
    pub alpha: f32,
    /// 0..1 position of the shimmer sweep.
    pub phase: f32,
}

#[derive(Debug, Default)]
pub struct Glows {
    items: Vec<Glow>,
}

impl Glows {
    /// Elements an agent looked at (board-local CSS rects).
    pub fn elements(
        &mut self,
        tab: u64,
        rects: impl IntoIterator<Item = select::Rect>,
        now: Instant,
    ) {
        let fresh: Vec<Glow> = rects
            .into_iter()
            .filter(|r| r.width > 0.0 && r.height > 0.0)
            .take(MAX_ELEMENTS)
            .map(|rect| Glow {
                tab,
                rect: Some(rect),
                start: now,
                end: End::At(now + ELEMENT),
            })
            .collect();
        // A second look at the same element restarts its glow.
        self.items
            .retain(|g| !fresh.iter().any(|f| f.tab == g.tab && f.rect == g.rect));
        self.items.extend(fresh);
    }

    /// A whole board an agent touched.
    pub fn board(&mut self, tab: u64, now: Instant) {
        self.put_board(tab, now, End::At(now + BOARD));
    }

    /// A board reloading: glows until it settles.
    pub fn reloading(&mut self, tab: u64, now: Instant) {
        self.put_board(tab, now, End::Settled);
    }

    fn put_board(&mut self, tab: u64, now: Instant, end: End) {
        let start = match self
            .items
            .iter()
            .position(|g| g.tab == tab && g.rect.is_none())
        {
            // Keep the sweep running; just move the end.
            Some(i) => self.items.swap_remove(i).start,
            None => now,
        };
        self.items.push(Glow {
            tab,
            rect: None,
            start,
            end,
        });
    }

    pub fn forget(&mut self, tab: u64) {
        self.items.retain(|g| g.tab != tab);
    }

    pub fn is_active(&self) -> bool {
        !self.items.is_empty()
    }

    /// Advance: settled boards start their tail; finished glows go.
    pub fn tick(&mut self, now: Instant, settled: impl Fn(u64) -> bool) {
        for g in &mut self.items {
            if g.end == End::Settled && settled(g.tab) {
                g.end = End::At(now + TAIL);
            }
        }
        self.items
            .retain(|g| !matches!(g.end, End::At(t) if t <= now));
    }

    pub fn lit(&self, now: Instant) -> Vec<Lit> {
        self.items
            .iter()
            .map(|g| {
                let age = now.saturating_duration_since(g.start).as_secs_f32();
                let fade_in = (age / FADE_IN).min(1.0);
                let fade_out = match g.end {
                    End::At(t) => {
                        (t.saturating_duration_since(now).as_secs_f32() / FADE_OUT).min(1.0)
                    }
                    End::Settled => 1.0,
                };
                Lit {
                    tab: g.tab,
                    rect: g.rect,
                    alpha: fade_in.min(fade_out),
                    phase: (age % PERIOD) / PERIOD,
                }
            })
            .collect()
    }
}

/// Draw one glow over screen rect `r`: a soft halo in `color` and a light
/// band sweeping across.
pub fn draw(frame: &mut Frame, r: Rectangle, lit: &Lit, color: Color) {
    if r.width < 1.0 || r.height < 1.0 || lit.alpha <= 0.0 {
        return;
    }
    let a = lit.alpha;
    frame.fill_rectangle(
        r.position(),
        r.size(),
        Color {
            a: 0.07 * a,
            ..color
        },
    );
    for (width, alpha) in [(9.0, 0.08), (5.0, 0.18), (1.5, 0.95)] {
        frame.stroke_rectangle(
            r.position(),
            r.size(),
            Stroke::default()
                .with_color(Color {
                    a: alpha * a,
                    ..color
                })
                .with_width(width),
        );
    }
    // The sweep runs corner to corner; beyond its ends the gradient is clear.
    let len = (r.width * r.width + r.height * r.height).sqrt();
    let dir = Vector::new(r.width / len, r.height / len);
    let band = (len * 0.16).clamp(24.0, 240.0);
    let center = Point::new(r.x, r.y) + dir * (-band + (len + 2.0 * band) * lit.phase);
    let clear = Color {
        a: 0.0,
        ..Color::WHITE
    };
    let sweep = Linear::new(center - dir * band, center + dir * band)
        .add_stop(0.0, clear)
        .add_stop(
            0.5,
            Color {
                a: 0.22 * a,
                ..Color::WHITE
            },
        )
        .add_stop(1.0, clear);
    frame.fill_rectangle(r.position(), Size::new(r.width, r.height), sweep);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32) -> select::Rect {
        select::Rect {
            x,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        }
    }

    #[test]
    fn element_glows_fade_and_end() {
        let t0 = Instant::now();
        let mut g = Glows::default();
        g.elements(
            1,
            [
                rect(0.0),
                rect(20.0),
                select::Rect {
                    width: 0.0,
                    ..rect(5.0)
                },
            ],
            t0,
        );
        assert_eq!(g.lit(t0).len(), 2, "empty rects do not glow");
        assert_eq!(g.lit(t0)[0].alpha, 0.0);
        assert_eq!(g.lit(t0 + Duration::from_millis(500))[0].alpha, 1.0);
        g.tick(t0 + ELEMENT, |_| true);
        assert!(!g.is_active());
    }

    #[test]
    fn a_reloading_board_glows_until_it_settles() {
        let t0 = Instant::now();
        let mut g = Glows::default();
        g.reloading(4, t0);
        g.tick(t0 + Duration::from_secs(30), |_| false);
        assert!(g.is_active(), "still loading");
        let t1 = t0 + Duration::from_secs(31);
        g.tick(t1, |_| true);
        assert!(g.is_active(), "tail");
        g.tick(t1 + TAIL, |_| true);
        assert!(!g.is_active());
    }

    #[test]
    fn looking_again_restarts_not_duplicates() {
        let t0 = Instant::now();
        let mut g = Glows::default();
        g.elements(1, [rect(0.0)], t0);
        g.elements(1, [rect(0.0)], t0 + Duration::from_secs(1));
        assert_eq!(g.lit(t0 + Duration::from_secs(1)).len(), 1);
        g.board(2, t0);
        g.reloading(2, t0);
        assert_eq!(g.lit(t0).iter().filter(|l| l.tab == 2).count(), 1);
        g.forget(1);
        assert!(g.lit(t0).iter().all(|l| l.tab == 2));
    }
}
