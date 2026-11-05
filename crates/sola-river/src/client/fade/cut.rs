//! The overlay in pieces: the output less the holes (the videos, with no
//! overlay over them), the pieces inside the kept rectangle (the flower and
//! the agent islands, never past the rest) marked.

/// A rectangle of the output, logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x.saturating_add(self.w)
    }

    pub fn bottom(&self) -> i32 {
        self.y.saturating_add(self.h)
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn area(&self) -> i64 {
        if self.is_empty() {
            0
        } else {
            i64::from(self.w) * i64::from(self.h)
        }
    }

    /// The part of `self` inside `other` (empty: none).
    pub fn intersect(&self, other: &Rect) -> Rect {
        let (x0, y0) = (self.x.max(other.x), self.y.max(other.y));
        let (x1, y1) = (self.right().min(other.right()), self.bottom().min(other.bottom()));
        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0),
            h: (y1 - y0).max(0),
        }
    }
}

/// `r` less every rectangle of `cuts`, in pieces.
pub fn subtract(r: Rect, cuts: &[Rect]) -> Vec<Rect> {
    let mut out = if r.is_empty() { Vec::new() } else { vec![r] };
    for c in cuts {
        out = out.into_iter().flat_map(|p| minus(p, c)).collect();
    }
    out
}

/// `p` less `c`: what is above, below, left and right of it.
fn minus(p: Rect, c: &Rect) -> Vec<Rect> {
    let i = p.intersect(c);
    if i.is_empty() {
        return vec![p];
    }
    [
        Rect { x: p.x, y: p.y, w: p.w, h: i.y - p.y },
        Rect { x: p.x, y: i.bottom(), w: p.w, h: p.bottom() - i.bottom() },
        Rect { x: p.x, y: i.y, w: i.x - p.x, h: i.h },
        Rect { x: i.right(), y: i.y, w: p.right() - i.right(), h: i.h },
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

/// The output (`w` × `h`) less `holes`, in pieces that cover the rest once;
/// those inside `keep` are marked (`true`). Cut in bands at every edge, the
/// runs of each band joined to the same run in the band above.
pub fn cut(w: i32, h: i32, keep: Option<Rect>, holes: &[Rect]) -> Vec<(Rect, bool)> {
    let output = Rect { x: 0, y: 0, w, h };
    let keep = keep.map(|k| k.intersect(&output)).filter(|k| !k.is_empty());
    let holes: Vec<Rect> = holes
        .iter()
        .map(|r| r.intersect(&output))
        .filter(|r| !r.is_empty())
        .collect();
    let mut ys = vec![0, h];
    for r in keep.iter().chain(&holes) {
        ys.extend([r.y, r.bottom()]);
    }
    ys.sort_unstable();
    ys.dedup();
    let mut done = Vec::new();
    // The pieces that reach the band being cut, from above.
    let mut open: Vec<(Rect, bool)> = Vec::new();
    for band in ys.windows(2) {
        let (y0, y1) = (band[0], band[1]);
        let mut next = Vec::new();
        for (x0, x1, kept) in runs(w, y0, y1, keep, &holes) {
            let above = open
                .iter()
                .position(|(r, k)| r.x == x0 && r.right() == x1 && *k == kept);
            match above {
                Some(i) => {
                    let (mut r, k) = open.swap_remove(i);
                    r.h += y1 - y0;
                    next.push((r, k));
                }
                None => next.push((Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, kept)),
            }
        }
        done.append(&mut open);
        open = next;
    }
    done.append(&mut open);
    done
}

/// The band `y0`–`y1` across the output as runs `(x0, x1, kept)`, holes
/// left out. Every edge is a band edge, so a rectangle spans the band or
/// misses it.
fn runs(w: i32, y0: i32, y1: i32, keep: Option<Rect>, holes: &[Rect]) -> Vec<(i32, i32, bool)> {
    let spans = |r: &Rect| r.y <= y0 && r.bottom() >= y1;
    let holes: Vec<&Rect> = holes.iter().filter(|r| spans(r)).collect();
    let keep = keep.filter(|k| spans(k));
    let mut xs = vec![0, w];
    for r in holes.iter().copied().chain(keep.as_ref()) {
        xs.extend([r.x, r.right()]);
    }
    xs.sort_unstable();
    xs.dedup();
    let mut out: Vec<(i32, i32, bool)> = Vec::new();
    for seg in xs.windows(2) {
        let (x0, x1) = (seg[0], seg[1]);
        if holes.iter().any(|r| r.x <= x0 && r.right() >= x1) {
            continue;
        }
        let kept = keep.is_some_and(|k| k.x <= x0 && k.right() >= x1);
        match out.last_mut() {
            Some(last) if last.1 == x0 && last.2 == kept => last.1 = x1,
            _ => out.push((x0, x1, kept)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    fn area(pieces: &[(Rect, bool)]) -> i64 {
        pieces.iter().map(|(r, _)| r.area()).sum()
    }

    /// No two pieces overlap.
    fn apart(pieces: &[(Rect, bool)]) -> bool {
        pieces.iter().enumerate().all(|(i, (a, _))| {
            pieces[i + 1..].iter().all(|(b, _)| a.intersect(b).is_empty())
        })
    }

    #[test]
    fn the_cut_covers_the_output_once() {
        let pieces = cut(1920, 1080, Some(r(0, 0, 240, 40)), &[]);
        assert_eq!(area(&pieces), 1920 * 1080);
        assert!(apart(&pieces));
        let kept: Vec<_> = pieces.iter().filter(|(_, k)| *k).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, r(0, 0, 240, 40));
        // In the middle: four around it.
        assert_eq!(cut(400, 300, Some(r(100, 100, 50, 50)), &[]).len(), 5);
        // Off the output, or none: one piece.
        let whole = r(0, 0, 400, 300);
        assert_eq!(cut(400, 300, Some(r(5000, 0, 10, 10)), &[]), vec![(whole, false)]);
        assert_eq!(cut(400, 300, None, &[]), vec![(whole, false)]);
    }

    #[test]
    fn a_video_is_left_out() {
        let video = r(500, 200, 1280, 720);
        let pieces = cut(1920, 1080, Some(r(0, 0, 240, 40)), &[video]);
        assert_eq!(area(&pieces), 1920 * 1080 - video.area());
        assert!(apart(&pieces));
        assert!(pieces.iter().all(|(p, _)| p.intersect(&video).is_empty()));
        // Above, left, right, below the video; the kept piece and the
        // rest of its rows.
        assert_eq!(pieces.len(), 6, "{pieces:?}");
    }

    #[test]
    fn holes_side_by_side_and_off_the_output() {
        let holes = [r(100, 100, 200, 100), r(300, 100, 200, 100), r(-50, 900, 100, 400)];
        let pieces = cut(1000, 1000, None, &holes);
        let left_out = 400 * 100 + 50 * 100;
        assert_eq!(area(&pieces), 1000 * 1000 - left_out);
        assert!(apart(&pieces));
        assert!(pieces.iter().all(|(_, k)| !k));
        // A hole as large as the output leaves nothing.
        assert!(cut(1000, 1000, None, &[r(-1, -1, 2000, 2000)]).is_empty());
    }

    #[test]
    fn what_covers_a_video_is_taken_from_it() {
        let video = r(0, 0, 100, 100);
        assert_eq!(subtract(video, &[]), vec![video]);
        assert!(subtract(video, &[r(-10, -10, 200, 200)]).is_empty());
        let rest = subtract(video, &[r(25, 25, 50, 50), r(0, 0, 10, 10)]);
        let total: i64 = rest.iter().map(Rect::area).sum();
        assert_eq!(total, 100 * 100 - 50 * 50 - 10 * 10);
        assert_eq!(subtract(video, &[r(200, 0, 10, 10)]), vec![video]);
    }
}
