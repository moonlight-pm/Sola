//! The titlebar row: where the stoplight, the menu titles and the window
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
/// A light of the stoplight, and the room between two.
const DOT: f32 = 12.0;
const DOT_GAP: f32 = 8.0;
/// From the last light to the first menu.
const DOTS_GAP: f32 = 9.0;
pub(super) const PILL_H: f32 = 24.0;
pub(super) const PILL_PAD: f32 = 8.0;
const PILL_GAP: f32 = 1.0;
const PILL_RADIUS: f32 = 7.0;
const TITLE_GAP: f32 = 18.0;
const MENU_SIZE: f32 = 13.0;
const TITLE_SIZE: f32 = 12.5;
/// Narrower than this and the title is left out rather than squeezed.
const TITLE_MIN: f32 = 32.0;

/// A light of the stoplight, left to right: close the window, hide its
/// app, pick its zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Dot {
    Close,
    Hide,
    Zone,
}

impl Dot {
    pub const ALL: [Dot; 3] = [Dot::Close, Dot::Hide, Dot::Zone];
}

/// What a point on the titlebar lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hit {
    Dot(Dot),
    Menu(usize),
    /// Anywhere else on the row (the drag handle).
    Bar,
}

/// The laid-out row.
#[derive(Debug, Default)]
pub(super) struct Bar {
    key: Option<(Vec<String>, String, u32)>,
    /// The lights, in [`Dot::ALL`] order.
    pub dots: [Rectangle; 3],
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
        for (i, dot) in self.dots.iter_mut().enumerate() {
            *dot = Rectangle::new(
                Point::new(PAD_LEFT + i as f32 * (DOT + DOT_GAP), (HEIGHT - DOT) / 2.0),
                Size::new(DOT, DOT),
            );
        }
        let limit = width - PAD_RIGHT;
        let mut x = self.dots[2].x + DOT + DOTS_GAP;
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
        // A generous square around each light: they are small.
        if let Some(i) = self.dots.iter().position(|d| d.expand(4.0).contains(p)) {
            return Some(Hit::Dot(Dot::ALL[i]));
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
    /// Which lights show, in [`Dot::ALL`] order (a window that cannot be
    /// closed has no red one).
    pub dots: [bool; 3],
    /// The light drawn hovered.
    pub lit: Option<Dot>,
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

    for (i, dot) in Dot::ALL.into_iter().enumerate() {
        if !d.dots[i] {
            continue;
        }
        let (fill, hover) = d.colors.dots[i];
        let fill = if d.lit == Some(dot) { hover } else { fill };
        paint::fill(renderer, shift(d.bar.dots[i], o), fill, (DOT / 2.0).into());
    }

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
    fonts::line_width_advanced(content, size, font)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar_with(menus: &[(f32, f32)]) -> Bar {
        Bar {
            key: None,
            dots: [13.0, 33.0, 53.0]
                .map(|x| Rectangle::new(Point::new(x, 13.0), Size::new(12.0, 12.0))),
            menus: menus
                .iter()
                .map(|(x, w)| Rectangle::new(Point::new(*x, 7.0), Size::new(*w, 24.0)))
                .collect(),
            labels: menus.iter().map(|_| String::from("M")).collect(),
            title: None,
        }
    }

    #[test]
    fn hits_the_lights_menus_and_the_handle() {
        let bar = bar_with(&[(74.0, 50.0), (125.0, 40.0)]);
        assert_eq!(bar.hit(Point::new(18.0, 19.0)), Some(Hit::Dot(Dot::Close)));
        assert_eq!(bar.hit(Point::new(38.0, 19.0)), Some(Hit::Dot(Dot::Hide)));
        assert_eq!(bar.hit(Point::new(58.0, 19.0)), Some(Hit::Dot(Dot::Zone)));
        assert_eq!(bar.hit(Point::new(80.0, 5.0)), Some(Hit::Menu(0)));
        assert_eq!(bar.hit(Point::new(140.0, 30.0)), Some(Hit::Menu(1)));
        assert_eq!(bar.hit(Point::new(300.0, 19.0)), Some(Hit::Bar));
        assert_eq!(bar.hit(Point::new(300.0, 40.0)), None);
    }

    #[test]
    fn the_gap_between_pills_is_the_handle() {
        let bar = bar_with(&[(74.0, 50.0), (125.0, 40.0)]);
        assert_eq!(bar.hit(Point::new(124.5, 19.0)), Some(Hit::Bar));
    }

    #[test]
    fn the_lights_sit_twenty_apart() {
        let mut bar = Bar::default();
        bar.lay_out(600.0, &[], "");
        assert_eq!(bar.dots.map(|d| d.x), [13.0, 33.0, 53.0]);
        assert_eq!(bar.dots[0].y, 13.0);
    }
}
