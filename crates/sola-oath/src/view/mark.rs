//! One mark per standing, the same everywhere: a grey dot for current, a
//! ring for a local build, filled amber for not pushed, filled cyan for an
//! update, a thin ring for off, a dashed ring for the origin, and a red
//! diamond beside any of them for advisories.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, Vector};

use super::ink::Ink;
use crate::standing::Standing;

pub const SIZE: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Standing(Standing),
    Advisory,
    /// Not assessed, not in the report: a dashed grey ring.
    Unknown,
}

pub fn mark<'a, M: 'a>(kind: Kind) -> Element<'a, M> {
    canvas::Canvas::new(Mark(kind)).width(Length::Fixed(SIZE)).height(Length::Fixed(SIZE)).into()
}

pub fn standing<'a, M: 'a>(s: Standing) -> Element<'a, M> {
    mark(Kind::Standing(s))
}

struct Mark(Kind);

impl<M> canvas::Program<M> for Mark {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, theme: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        draw(&mut f, &Ink::of(theme), self.0, Point::new(bounds.width / 2.0, bounds.height / 2.0), 1.0);
        vec![f.into_geometry()]
    }
}

/// Draw `kind` centered at `c`, `scale` times the list's size; the map's
/// blocks call this too.
pub fn draw(f: &mut Frame, ink: &Ink, kind: Kind, c: Point, scale: f32) {
    let r = 4.0 * scale;
    let circle = |r: f32| Path::circle(c, r);
    match kind {
        Kind::Standing(Standing::Current) => f.fill(&circle(3.0 * scale), Color::from_rgb8(0x4c, 0x4c, 0x4c)),
        Kind::Standing(Standing::Local) => {
            f.stroke(&circle(r - 1.0), Stroke::default().with_color(ink.warning).with_width(2.0 * scale))
        }
        Kind::Standing(Standing::NotPushed) => {
            f.fill(&circle(r + 1.5 * scale), Color { a: 0.22, ..ink.warning });
            f.fill(&circle(r), ink.warning);
        }
        Kind::Standing(Standing::Update) => f.fill(&circle(r), ink.accent),
        Kind::Standing(Standing::Off) => {
            f.stroke(&circle(r - 0.5), Stroke::default().with_color(ink.fg3).with_width(1.0))
        }
        Kind::Standing(Standing::OriginOnly) => dashed_ring(f, c, r - 0.5, ink.accent),
        Kind::Unknown => dashed_ring(f, c, r - 0.5, ink.fg4),
        Kind::Advisory => {
            let d = r * 0.95;
            let diamond = Path::new(|p| {
                p.move_to(c + Vector::new(0.0, -d));
                p.line_to(c + Vector::new(d, 0.0));
                p.line_to(c + Vector::new(0.0, d));
                p.line_to(c + Vector::new(-d, 0.0));
                p.close();
            });
            f.fill(&diamond, ink.danger);
        }
    }
}

fn dashed_ring(f: &mut Frame, c: Point, r: f32, color: Color) {
    let stroke = Stroke::default().with_color(color).with_width(1.0);
    for i in 0..8 {
        let a0 = i as f32 * std::f32::consts::TAU / 8.0;
        let a1 = a0 + std::f32::consts::TAU / 16.0;
        let arc = Path::new(|p| {
            p.arc(canvas::path::Arc { center: c, radius: r, start_angle: a0.into(), end_angle: a1.into() });
        });
        f.stroke(&arc, stroke);
    }
}

/// A small block (the LED strip of the summary and categories): filled,
/// or only its edge when `edge` (off, and what is only on the origin).
pub fn led<'a, M: 'a>(fill: Color, edge: Option<Color>, w: f32, h: f32) -> Element<'a, M> {
    iced::widget::container(iced::widget::Space::new())
        .width(Length::Fixed(w))
        .height(Length::Fixed(h))
        .style(move |_| iced::widget::container::Style {
            background: Some(fill.into()),
            border: iced::Border { radius: 1.0.into(), width: if edge.is_some() { 1.0 } else { 0.0 }, color: edge.unwrap_or(fill) },
            ..Default::default()
        })
        .into()
}
