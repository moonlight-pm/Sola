//! Selection marquee view — freeze still + cyan rectangle while dragging.
//!
//! No dim: the freeze is the desktop. Overlay stays out of composition
//! until the freeze texture is on the GPU (`SelectionTextureReady`).

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Event, Frame, Geometry, Path, Stroke};
use iced::widget::{container, stack, text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::app::{Msg, Shell};
use crate::selection::freeze::FreezeLayer;

/// Render the selection overlay for `shell`.
pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.selection.active {
        return container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let marquee = Canvas::new(Marquee {
        drag_start: shell.selection.drag_start,
        drag_current: shell.selection.drag_current,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let Some(handle) = shell.selection.freeze.clone() else {
        return marquee.into();
    };

    stack![FreezeLayer::new(handle), marquee]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[derive(Debug, Clone)]
struct Marquee {
    drag_start: Option<(f32, f32)>,
    drag_current: Option<(f32, f32)>,
}

impl canvas::Program<Msg> for Marquee {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Msg>> {
        let Some(pos) = cursor.position_in(bounds) else {
            // Still capture release outside so a drag ending off-window
            // finishes rather than sticking.
            if matches!(
                event,
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            ) {
                if let Some((x, y)) = self.drag_current {
                    return Some(
                        canvas::Action::publish(Msg::SelectionRelease { x, y }).and_capture(),
                    );
                }
            }
            return None;
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => Some(
                canvas::Action::publish(Msg::SelectionPress { x: pos.x, y: pos.y }).and_capture(),
            ),
            Event::Mouse(mouse::Event::CursorMoved { .. }) if self.drag_start.is_some() => Some(
                canvas::Action::publish(Msg::SelectionMove { x: pos.x, y: pos.y }).and_capture(),
            ),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(
                canvas::Action::publish(Msg::SelectionRelease { x: pos.x, y: pos.y }).and_capture(),
            ),
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        if let (Some((x0, y0)), Some((x1, y1))) = (self.drag_start, self.drag_current) {
            let left = x0.min(x1);
            let top = y0.min(y1);
            let w = (x0 - x1).abs();
            let h = (y0 - y1).abs();
            if w >= 1.0 && h >= 1.0 {
                let rect = Path::rectangle(Point::new(left, top), Size::new(w, h));
                frame.stroke(
                    &rect,
                    Stroke::default().with_width(1.5).with_color(Color {
                        r: 0.0,
                        g: 0.83,
                        b: 1.0,
                        a: 0.95,
                    }),
                );
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::Crosshair
    }
}
