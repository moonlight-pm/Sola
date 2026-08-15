//! Stage: contain-fit image on a checker well, plus crop overlay.

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Event, Frame, Geometry, Path, Stroke};
use iced::widget::{container, image, stack};
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme};

use crate::doc::Doc;
use crate::geom;
use crate::Msg;

#[derive(Debug, Clone, Copy)]
pub struct CropGesture {
    pub origin: Point,
    pub current: Point,
}

pub fn view<'a>(
    doc: &'a Doc,
    cropping: bool,
    crop: Option<CropGesture>,
    theme: &Theme,
) -> Element<'a, Msg> {
    let img_size = Size::new(doc.pixels.width() as f32, doc.pixels.height() as f32);
    let picture = image(doc.handle.clone())
        .width(Length::Fill)
        .height(Length::Fill)
        .content_fit(iced::ContentFit::Contain);

    let p = theme.extended_palette();
    let well = p.background.base.color;
    let accent = p.primary.base.color;

    let overlay = Canvas::new(StageCanvas {
        img: img_size,
        cropping,
        crop,
        well,
        accent,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let body: Element<'a, Msg> = stack![
        container(picture)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        overlay,
    ]
    .into();

    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(well)),
            ..container::Style::default()
        })
        .into()
}

struct StageCanvas {
    img: Size,
    cropping: bool,
    crop: Option<CropGesture>,
    well: Color,
    accent: Color,
}

impl canvas::Program<Msg> for StageCanvas {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Msg>> {
        if !self.cropping {
            return None;
        }
        let pos = cursor.position_in(bounds)?;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Some(canvas::Action::publish(Msg::CropPress(pos, bounds.size())).and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                Some(canvas::Action::publish(Msg::StageMove(pos, bounds.size())))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(canvas::Action::publish(Msg::CropRelease).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.cropping {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        draw_checker(&mut frame, bounds.size(), self.well);

        if self.cropping {
            let dest = geom::contain_rect(self.img, bounds.size());
            if dest.width > 0.0 && dest.height > 0.0 {
                let dim = Color {
                    a: 0.45,
                    ..Color::BLACK
                };
                fill_outside(&mut frame, bounds.size(), dest, dim);
                if let Some(g) = self.crop {
                    let sel = geom::norm_rect(g.origin, g.current, dest);
                    fill_outside(&mut frame, bounds.size(), sel, dim);
                    frame.fill(
                        &Path::rectangle(sel.position(), sel.size()),
                        Color {
                            a: 0.08,
                            ..self.accent
                        },
                    );
                    frame.stroke(
                        &Path::rectangle(sel.position(), sel.size()),
                        Stroke::default()
                            .with_width(1.0)
                            .with_color(self.accent),
                    );
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

fn draw_checker(frame: &mut Frame, size: Size, well: Color) {
    let cell = 12.0;
    let a = mix(well, Color::WHITE, 0.04);
    let b = mix(well, Color::BLACK, 0.06);
    let cols = (size.width / cell).ceil() as i32 + 1;
    let rows = (size.height / cell).ceil() as i32 + 1;
    for y in 0..rows {
        for x in 0..cols {
            let color = if (x + y) % 2 == 0 { a } else { b };
            frame.fill(
                &Path::rectangle(
                    Point::new(x as f32 * cell, y as f32 * cell),
                    Size::new(cell, cell),
                ),
                color,
            );
        }
    }
}

fn fill_outside(frame: &mut Frame, view: Size, inner: Rectangle, color: Color) {
    // Four slabs around `inner`.
    if inner.y > 0.0 {
        frame.fill(
            &Path::rectangle(Point::ORIGIN, Size::new(view.width, inner.y)),
            color,
        );
    }
    let bottom = inner.y + inner.height;
    if bottom < view.height {
        frame.fill(
            &Path::rectangle(
                Point::new(0.0, bottom),
                Size::new(view.width, view.height - bottom),
            ),
            color,
        );
    }
    if inner.x > 0.0 {
        frame.fill(
            &Path::rectangle(
                Point::new(0.0, inner.y),
                Size::new(inner.x, inner.height),
            ),
            color,
        );
    }
    let right = inner.x + inner.width;
    if right < view.width {
        frame.fill(
            &Path::rectangle(
                Point::new(right, inner.y),
                Size::new(view.width - right, inner.height),
            ),
            color,
        );
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}
