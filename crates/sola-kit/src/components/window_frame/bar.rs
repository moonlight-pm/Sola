//! The titlebar row: where the close dot, the menu titles and the window
//! title sit (measured once per change of labels or width), what a point
//! hits, and how the row draws. Positions are relative to the frame's
//! top-left corner.

use iced::advanced::text::{self, Renderer as _};
use iced::alignment::Vertical;
use iced::border::Radius;
use iced::{Font, Pixels, Point, Rectangle, Size};

use super::HEIGHT;
use super::paint::{self, Colors};
use crate::fonts;

const PAD_LEFT: f32 = 13.0;
const PAD_RIGHT: f32 = 10.0;
const CLOSE: f32 = 12.0;
const CLOSE_GAP: f32 = 9.0;
pub(super) const PILL_H: f32 = 24.0;
pub(super) const PILL_PAD: f32 = 8.0;
const PILL_GAP: f32 = 1.0;
const PILL_RADIUS: f32 = 7.0;
const TITLE_GAP: f32 = 18.0;
const MENU_SIZE: f32 = 13.0;
const TITLE_SIZE: f32 = 12.5;
/// Narrower than this and the title is left out rather than squeezed.
const TITLE_MIN: f32 = 32.0;

/// What a point on the titlebar lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hit {
    Close,
    Menu(usize),
    /// Anywhere else on the row (the drag handle).
    Bar,
}

/// The laid-out row.
#[derive(Debug, Default)]
pub(super) struct Bar {
    key: Option<(Vec<String>, String, u32)>,
    pub close: Rectangle,
    /// One pill per menu that fits, in menu order.
    pub menus: Vec<Rectangle>,
    pub labels: Vec<String>,
    /// The shown (maybe shortened) title and where it goes.
    pub title: Option<(String, Rectangle)>,
}

impl Bar {
    /// Lay the row out for a frame `width` wide. Cheap when nothing changed.
    pub fn lay_out(&mut self, width: f32, labels: &[String], title: &str) {
        let key = (labels.to_vec(), title.to_string(), width.to_bits());
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let pill_y = (HEIGHT - PILL_H) / 2.0;
        self.close = Rectangle::new(
            Point::new(PAD_LEFT, (HEIGHT - CLOSE) / 2.0),
            Size::new(CLOSE, CLOSE),
        );
        let limit = width - PAD_RIGHT;
        let mut x = PAD_LEFT + CLOSE + CLOSE_GAP;
        self.menus.clear();
        self.labels.clear();
        for (i, label) in labels.iter().enumerate() {
            let w = measure(label, MENU_SIZE, menu_font(i)) + 2.0 * PILL_PAD;
            if x + w > limit {
                break;
            }
            self.menus
                .push(Rectangle::new(Point::new(x, pill_y), Size::new(w, PILL_H)));
            self.labels.push(label.clone());
            x += w + PILL_GAP;
        }
        let repeats_name = labels
            .first()
            .is_some_and(|name| name.eq_ignore_ascii_case(title.trim()));
        let start = x + TITLE_GAP;
        let room = limit - start;
        self.title = if title.trim().is_empty() || repeats_name || room < TITLE_MIN {
            None
        } else {
            let shown = fonts::fit_line(title.trim(), TITLE_SIZE, fonts::chrome(), room);
            let w = measure(&shown, TITLE_SIZE, fonts::chrome());
            Some((
                shown,
                Rectangle::new(Point::new(limit - w, 0.0), Size::new(w, HEIGHT)),
            ))
        };
        self.key = Some(key);
    }

    /// What `p` (relative to the frame) lands on, if it is on the row.
    pub fn hit(&self, p: Point) -> Option<Hit> {
        if p.y < 0.0 || p.y >= HEIGHT {
            return None;
        }
        // A generous square around the dot: it is small.
        if self.close.expand(4.0).contains(p) {
            return Some(Hit::Close);
        }
        let on_menu = self
            .menus
            .iter()
            .position(|r| p.x >= r.x && p.x < r.x + r.width);
        Some(on_menu.map_or(Hit::Bar, Hit::Menu))
    }
}

/// The app's name (menu 0) is bold; the rest are regular.
fn menu_font(index: usize) -> Font {
    if index == 0 {
        Font {
            weight: iced::font::Weight::Semibold,
            ..fonts::chrome()
        }
    } else {
        fonts::chrome()
    }
}

pub(super) struct DrawBar<'a> {
    pub bar: &'a Bar,
    pub origin: Point,
    pub width: f32,
    pub colors: &'a Colors,
    pub open: Option<usize>,
    pub close_hover: bool,
}

pub(super) fn draw(renderer: &mut iced::Renderer, d: DrawBar<'_>, clip: Rectangle) {
    let o = d.origin;
    let strip = Rectangle::new(o, Size::new(d.width, HEIGHT));
    paint::fill(
        renderer,
        strip,
        d.colors.bar,
        Radius {
            top_left: super::RADIUS,
            top_right: super::RADIUS,
            bottom_right: 0.0,
            bottom_left: 0.0,
        },
    );
    paint::fill(
        renderer,
        Rectangle::new(Point::new(o.x, o.y + HEIGHT - 1.0), Size::new(d.width, 1.0)),
        d.colors.bar_line,
        Radius::default(),
    );

    let close = shift(d.bar.close, o);
    let close_fill = if d.close_hover {
        d.colors.close_hover
    } else {
        d.colors.close
    };
    paint::fill(renderer, close, close_fill, (CLOSE / 2.0).into());

    for (i, (pill, label)) in d.bar.menus.iter().zip(&d.bar.labels).enumerate() {
        let pill = shift(*pill, o);
        if d.open == Some(i) {
            paint::fill(renderer, pill, d.colors.pill, PILL_RADIUS.into());
        }
        label_at(
            renderer,
            label,
            MENU_SIZE,
            menu_font(i),
            Point::new(pill.x + PILL_PAD, o.y),
            d.colors.ink,
            clip,
        );
    }

    if let Some((title, r)) = &d.bar.title {
        label_at(
            renderer,
            title,
            TITLE_SIZE,
            fonts::chrome(),
            Point::new(o.x + r.x, o.y),
            d.colors.ink_dim,
            clip,
        );
    }
}

fn shift(r: Rectangle, o: Point) -> Rectangle {
    Rectangle::new(Point::new(o.x + r.x, o.y + r.y), r.size())
}

/// One line of text, vertically centred on the row starting at `at`.
pub(super) fn label_at(
    renderer: &mut iced::Renderer,
    content: &str,
    size: f32,
    font: Font,
    at: Point,
    color: iced::Color,
    clip: Rectangle,
) {
    line(renderer, content, size, font, at, HEIGHT, color, clip);
}

/// One line of text centred in a row `height` tall starting at `at`.
#[allow(clippy::too_many_arguments)]
pub(super) fn line(
    renderer: &mut iced::Renderer,
    content: &str,
    size: f32,
    font: Font,
    at: Point,
    height: f32,
    color: iced::Color,
    clip: Rectangle,
) {
    renderer.fill_text(
        text::Text {
            content: content.to_string(),
            // No wrapping; the clip ends the line. Finite for the shaper.
            bounds: Size::new(clip.width.max(1.0), height),
            size: Pixels(size),
            line_height: text::LineHeight::default(),
            font,
            align_x: text::Alignment::Left,
            align_y: Vertical::Center,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        },
        Point::new(at.x, at.y + height / 2.0),
        color,
        clip,
    );
}

/// Width of one line of `content`.
pub(super) fn measure(content: &str, size: f32, font: Font) -> f32 {
    fonts::line_width(content, size, font)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar_with(menus: &[(f32, f32)]) -> Bar {
        Bar {
            key: None,
            close: Rectangle::new(Point::new(13.0, 13.0), Size::new(12.0, 12.0)),
            menus: menus
                .iter()
                .map(|(x, w)| Rectangle::new(Point::new(*x, 7.0), Size::new(*w, 24.0)))
                .collect(),
            labels: menus.iter().map(|_| String::from("M")).collect(),
            title: None,
        }
    }

    #[test]
    fn hits_close_menus_and_the_handle() {
        let bar = bar_with(&[(34.0, 50.0), (85.0, 40.0)]);
        assert_eq!(bar.hit(Point::new(18.0, 19.0)), Some(Hit::Close));
        assert_eq!(bar.hit(Point::new(40.0, 5.0)), Some(Hit::Menu(0)));
        assert_eq!(bar.hit(Point::new(100.0, 30.0)), Some(Hit::Menu(1)));
        assert_eq!(bar.hit(Point::new(300.0, 19.0)), Some(Hit::Bar));
        assert_eq!(bar.hit(Point::new(300.0, 40.0)), None);
    }

    #[test]
    fn the_gap_between_pills_is_the_handle() {
        let bar = bar_with(&[(34.0, 50.0), (85.0, 40.0)]);
        assert_eq!(bar.hit(Point::new(84.5, 19.0)), Some(Hit::Bar));
    }
}
