//! Stage: zoom/pan image on a checker well, plus crop overlay.
//!
//! Geometry is cached on `App` — hover / bus redraws must not retessellate
//! the checker or re-submit the raster. Idle pointer motion does not
//! publish messages (only crop + pan do).

use iced::mouse;
use iced::widget::canvas::{self, Cache, Canvas, Event, Frame, Geometry, Path, Stroke};
use iced::widget::image::FilterMethod;
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme, Vector};

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
    panning: bool,
    theme: &Theme,
    cache: &'a Cache,
) -> Element<'a, Msg> {
    let img_size = Size::new(doc.pixels.width() as f32, doc.pixels.height() as f32);
    let p = theme.extended_palette();
    let well = p.background.base.color;
    let accent = p.primary.base.color;

    Canvas::new(StageCanvas {
        handle: doc.handle.clone(),
        img: img_size,
        zoom: doc.zoom,
        pan: doc.pan,
        cropping,
        crop,
        panning,
        well,
        accent,
        cache,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct StageCanvas<'a> {
    handle: iced::widget::image::Handle,
    img: Size,
    zoom: f32,
    pan: Vector,
    cropping: bool,
    crop: Option<CropGesture>,
    panning: bool,
    well: Color,
    accent: Color,
    cache: &'a Cache,
}

impl canvas::Program<Msg> for StageCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Msg>> {
        let size = bounds.size();
        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let pos = cursor.position_in(bounds)?;
                let steps = match *delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / 60.0,
                };
                if steps.abs() < f32::EPSILON {
                    return None;
                }
                Some(
                    canvas::Action::publish(Msg::ZoomAt {
                        cursor: pos,
                        size,
                        factor: geom::zoom_factor(steps),
                    })
                    .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(Msg::StagePress(pos, size)).and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                // Idle hover must not hit App::update — that rebuilds the
                // whole chrome (and used to retessellate the checker).
                if !self.cropping && !self.panning {
                    return None;
                }
                let pos = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(Msg::StageMove(pos, size)))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !self.cropping && !self.panning {
                    return None;
                }
                Some(canvas::Action::publish(Msg::StageRelease).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.cropping {
            return mouse::Interaction::Crosshair;
        }
        if self.panning {
            return mouse::Interaction::Grabbing;
        }
        if cursor.position_in(bounds).is_none() {
            return mouse::Interaction::None;
        }
        let dest = geom::dest_rect(self.img, bounds.size(), self.zoom, self.pan);
        if dest.width > bounds.width + 0.5 || dest.height > bounds.height + 0.5 {
            mouse::Interaction::Grab
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
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            paint_stage(
                frame,
                bounds.size(),
                &self.handle,
                self.img,
                self.zoom,
                self.pan,
                self.cropping,
                self.crop,
                self.well,
                self.accent,
            );
        });
        vec![geometry]
    }
}

fn paint_stage(
    frame: &mut Frame,
    view: Size,
    handle: &iced::widget::image::Handle,
    img: Size,
    zoom: f32,
    pan: Vector,
    cropping: bool,
    crop: Option<CropGesture>,
    well: Color,
    accent: Color,
) {
    frame.fill(
        &Path::rectangle(Point::ORIGIN, view),
        well,
    );

    let dest = geom::dest_rect(img, view, zoom, pan);
    let view_rect = Rectangle::new(Point::ORIGIN, view);
    if let Some(vis) = dest.intersection(&view_rect) {
        // Checker only where the raster sits (alpha). Skip when the
        // dest covers the well — a full-window cell grid was thousands
        // of fills per redraw and made hover feel like molasses.
        if vis.width + 1.0 < view.width || vis.height + 1.0 < view.height {
            draw_checker(frame, vis, well);
        }
    }

    if dest.width > 0.0 && dest.height > 0.0 {
        let mut raster = canvas::Image::from(handle);
        raster.filter_method = if zoom >= 3.0 {
            FilterMethod::Nearest
        } else {
            FilterMethod::Linear
        };
        frame.draw_image(dest, raster);
    }

    if cropping && dest.width > 0.0 && dest.height > 0.0 {
        let dim = Color {
            a: 0.45,
            ..Color::BLACK
        };
        fill_outside(frame, view, dest, dim);
        if let Some(g) = crop {
            let sel = geom::norm_rect(g.origin, g.current, dest);
            fill_outside(frame, view, sel, dim);
            frame.fill(
                &Path::rectangle(sel.position(), sel.size()),
                Color {
                    a: 0.08,
                    ..accent
                },
            );
            frame.stroke(
                &Path::rectangle(sel.position(), sel.size()),
                Stroke::default().with_width(1.0).with_color(accent),
            );
        }
    }
}

fn draw_checker(frame: &mut Frame, area: Rectangle, well: Color) {
    let cell = 16.0;
    let a = mix(well, Color::WHITE, 0.04);
    let b = mix(well, Color::BLACK, 0.06);
    let x0 = area.x.max(0.0);
    let y0 = area.y.max(0.0);
    let x1 = area.x + area.width;
    let y1 = area.y + area.height;
    let col0 = (x0 / cell).floor() as i32;
    let row0 = (y0 / cell).floor() as i32;
    let col1 = (x1 / cell).ceil() as i32;
    let row1 = (y1 / cell).ceil() as i32;
    for y in row0..row1 {
        for x in col0..col1 {
            let color = if (x + y) % 2 == 0 { a } else { b };
            let rx = (x as f32 * cell).max(x0);
            let ry = (y as f32 * cell).max(y0);
            let rw = ((x as f32 + 1.0) * cell).min(x1) - rx;
            let rh = ((y as f32 + 1.0) * cell).min(y1) - ry;
            if rw <= 0.0 || rh <= 0.0 {
                continue;
            }
            frame.fill(
                &Path::rectangle(Point::new(rx, ry), Size::new(rw, rh)),
                color,
            );
        }
    }
}

fn fill_outside(frame: &mut Frame, view: Size, inner: Rectangle, color: Color) {
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
