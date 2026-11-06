//! Drawing helpers the canvases share: rounded blocks, labels cut to fit,
//! and the curves between packages.

use iced::alignment::Vertical;
use iced::widget::canvas::{Frame, LineDash, Path, Stroke, Text};
use iced::{Color, Font, Point, Rectangle};

pub fn rounded(r: Rectangle, radius: f32) -> Path {
    Path::new(|p| p.rounded_rectangle(r.position(), r.size(), radius.into()))
}

/// `s`, cut with an ellipsis to fit `width` at `size` (an estimate: the
/// UI face averages a little over half its size per character).
pub fn fit(s: &str, width: f32, size: f32) -> String {
    let per = size * 0.56;
    let room = (width / per).floor().max(0.0) as usize;
    if s.chars().count() <= room {
        return s.to_string();
    }
    if room <= 1 {
        return String::new();
    }
    let mut out: String = s.chars().take(room - 1).collect();
    out.push('…');
    out
}

/// A line of text, its left edge at `at.x` and centered on `at.y`.
pub fn label(f: &mut Frame, s: String, at: Point, size: f32, color: Color, font: Font) {
    f.fill_text(Text {
        content: s,
        position: at,
        color,
        size: size.into(),
        font,
        align_y: Vertical::Center,
        ..Text::default()
    });
}

/// From the bottom (or top) of one block to the top (or bottom) of
/// another: a vertical S.
pub fn vcurve(from: Rectangle, to: Rectangle) -> Option<Path> {
    if (from.y - to.y).abs() < 4.0 {
        return None;
    }
    let down = to.y >= from.y;
    let a = Point::new(from.center_x(), if down { from.y + from.height } else { from.y });
    let c = Point::new(to.center_x(), if down { to.y } else { to.y + to.height });
    let my = (a.y + c.y) / 2.0;
    Some(Path::new(|p| {
        p.move_to(a);
        p.bezier_curve_to(Point::new(a.x, my), Point::new(c.x, my), c);
    }))
}

/// From the right edge of one block to the left edge of another: a
/// horizontal S.
pub fn hcurve(from: Rectangle, to: Rectangle) -> Option<Path> {
    let a = Point::new(from.x + from.width, from.center_y());
    let c = Point::new(to.x, to.center_y());
    if c.x <= a.x {
        return None;
    }
    let mx = (a.x + c.x) / 2.0;
    Some(Path::new(|p| {
        p.move_to(a);
        p.bezier_curve_to(Point::new(mx, a.y), Point::new(mx, c.y), c);
    }))
}

pub fn line(color: Color, width: f32) -> Stroke<'static> {
    Stroke::default().with_color(color).with_width(width)
}

pub fn dashed(color: Color, width: f32) -> Stroke<'static> {
    Stroke { line_dash: LineDash { segments: &[3.0, 3.0], offset: 0 }, ..line(color, width) }
}

/// Diagonal hatching inside `r`, one line every `step` pixels.
pub fn hatch(f: &mut Frame, r: Rectangle, step: f32, color: Color) {
    if r.width <= 0.0 || r.height <= 0.0 {
        return;
    }
    let stroke = line(color, 1.5);
    // Lines x + y = c, clipped to the rectangle.
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
    let mut c = x0 + y0 + step;
    while c < x1 + y1 {
        let a = Point::new((c - y0).min(x1), 0.0);
        let a = Point::new(a.x, c - a.x);
        let b = Point::new((c - y1).max(x0), 0.0);
        let b = Point::new(b.x, c - b.x);
        if a.y <= y1 + 0.01 && b.y >= y0 - 0.01 {
            f.stroke(&Path::line(a, b), stroke);
        }
        c += step;
    }
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn labels_are_cut_to_fit() {
        assert_eq!(fit("glibc", 200.0, 12.0), "glibc");
        assert_eq!(fit("font-iosevka-term-slab", 60.0, 12.0), "font-io…");
        assert_eq!(fit("x", 2.0, 12.0), "");
    }
}
