//! Spectrum widgets — the draggable colour-picking surfaces behind the
//! kit's [`ColorPicker`](crate::components::ColorPicker).
//!
//! Two custom `iced::advanced::Widget`s, because the picking gesture
//! needs the cursor position *relative to the widget's own bounds* —
//! something the built-in widgets don't expose. Both render their
//! surface with gradient-filled quads (no textures, no canvas) and
//! report a normalised value as the pointer drags across them:
//!
//! - [`SvSquare`] — a 2D saturation/value field for one hue. Drag to set
//!   saturation (x) and value (y). Emits `(s, v)` in `0..=1`.
//! - [`GradientStrip`] — a 1D rail painted with an arbitrary gradient,
//!   reused for the hue rainbow and the alpha ramp. Emits a single
//!   `0..=1` position; the caller scales it (×360 for hue, as-is for
//!   alpha).
//!
//! The drag mechanics mirror iced's own `slider` (capture the event,
//! track an `is_dragging` flag in widget state, keep tracking via
//! `cursor.land()` when the pointer leaves the bounds mid-drag).

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, renderer};
use iced::gradient::Linear;
use iced::{Background, Border, Color, Element, Event, Length, Radians, Rectangle, Size};

/// Side of the saturation/value field, logical px.
const SQUARE_W: f32 = 224.0;
const SQUARE_H: f32 = 168.0;
/// Long edge / thickness of the 1D strips.
const STRIP_LEN: f32 = 224.0;
const STRIP_THICK: f32 = 16.0;
/// Thumb radius on the SV field.
const THUMB_R: f32 = 7.0;

/// Gradient angle pointing left→right (iced: 0 rad = up, clockwise).
fn angle_right() -> Radians {
    Radians(std::f32::consts::FRAC_PI_2)
}
/// Gradient angle pointing top→bottom.
fn angle_down() -> Radians {
    Radians(std::f32::consts::PI)
}

#[derive(Default)]
struct DragState {
    is_dragging: bool,
}

// ---------------------------------------------------------------------
// SvSquare
// ---------------------------------------------------------------------

/// 2D saturation/value field for a fixed hue. `hue_color` is that hue at
/// full saturation+value (the caller computes it); the field paints
/// white→hue horizontally and transparent→black vertically on top, the
/// standard HSV square. The thumb sits at `(s, 1 - v)`.
pub struct SvSquare<'a, Message> {
    hue_color: Color,
    s: f32,
    v: f32,
    on_change: Box<dyn Fn(f32, f32) -> Message + 'a>,
}

/// A saturation/value field. `s`/`v` are the current point in `0..=1`;
/// `on_change(s, v)` fires as the pointer drags.
pub fn sv_square<'a, Message>(
    hue_color: Color,
    s: f32,
    v: f32,
    on_change: impl Fn(f32, f32) -> Message + 'a,
) -> SvSquare<'a, Message> {
    SvSquare {
        hue_color,
        s,
        v,
        on_change: Box::new(on_change),
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for SvSquare<'_, Message>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DragState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DragState::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(SQUARE_W),
            height: Length::Fixed(SQUARE_H),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fixed(SQUARE_W), Length::Fixed(SQUARE_H))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<DragState>();
        let bounds = layout.bounds();

        let publish = |shell: &mut Shell<'_, Message>, point: iced::Point| {
            let s = ((point.x - bounds.x) / bounds.width).clamp(0.0, 1.0);
            // y grows downward; value grows upward.
            let v = (1.0 - (point.y - bounds.y) / bounds.height).clamp(0.0, 1.0);
            shell.publish((self.on_change)(s, v));
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(point) = cursor.position_over(bounds) {
                    state.is_dragging = true;
                    publish(shell, point);
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.is_dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.is_dragging {
                    if let Some(point) = cursor.land().position() {
                        publish(shell, point);
                        shell.capture_event();
                    }
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        // Layer 1: white → pure hue, left to right.
        let hue_ramp = Linear::new(angle_right())
            .add_stop(0.0, Color::WHITE)
            .add_stop(1.0, self.hue_color);
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..renderer::Quad::default()
            },
            Background::Gradient(hue_ramp.into()),
        );

        // Layer 2: transparent → black, top to bottom, composited over (1).
        let dark_ramp = Linear::new(angle_down())
            .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0))
            .add_stop(1.0, Color::BLACK);
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..renderer::Quad::default()
            },
            Background::Gradient(dark_ramp.into()),
        );

        // Thumb: a white ring with a dark outer ring for contrast on any
        // background. Centre clamped so it stays fully inside the field.
        let cx = (bounds.x + self.s.clamp(0.0, 1.0) * bounds.width)
            .clamp(bounds.x + THUMB_R, bounds.x + bounds.width - THUMB_R);
        let cy = (bounds.y + (1.0 - self.v.clamp(0.0, 1.0)) * bounds.height)
            .clamp(bounds.y + THUMB_R, bounds.y + bounds.height - THUMB_R);
        thumb_ring(renderer, cx, cy, THUMB_R);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message, Theme, Renderer> From<SvSquare<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(w: SvSquare<'a, Message>) -> Self {
        Element::new(w)
    }
}

// ---------------------------------------------------------------------
// GradientStrip
// ---------------------------------------------------------------------

/// A horizontal rail painted with an arbitrary gradient. Reused for the
/// hue rainbow and the alpha ramp; emits a normalised `0..=1` position
/// the caller scales.
pub struct GradientStrip<'a, Message> {
    value: f32,
    stops: Vec<(f32, Color)>,
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
}

/// The rainbow hue rail. `hue` is the current hue in `0..=360`;
/// `on_change(hue)` fires as it drags. Seven stops — exactly the iced
/// gradient cap.
pub fn hue_strip<'a, Message>(
    hue: f32,
    on_change: impl Fn(f32) -> Message + 'a,
) -> GradientStrip<'a, Message> {
    let stops = vec![
        (0.0 / 6.0, Color::from_rgb(1.0, 0.0, 0.0)),
        (1.0 / 6.0, Color::from_rgb(1.0, 1.0, 0.0)),
        (2.0 / 6.0, Color::from_rgb(0.0, 1.0, 0.0)),
        (3.0 / 6.0, Color::from_rgb(0.0, 1.0, 1.0)),
        (4.0 / 6.0, Color::from_rgb(0.0, 0.0, 1.0)),
        (5.0 / 6.0, Color::from_rgb(1.0, 0.0, 1.0)),
        (6.0 / 6.0, Color::from_rgb(1.0, 0.0, 0.0)),
    ];
    GradientStrip {
        value: (hue / 360.0).clamp(0.0, 1.0),
        stops,
        on_change: Box::new(move |t| on_change(t * 360.0)),
    }
}

/// The alpha rail for `color`: transparent → opaque. `a` is the current
/// alpha in `0..=1`; `on_change(a)` fires as it drags.
pub fn alpha_strip<'a, Message>(
    color: Color,
    a: f32,
    on_change: impl Fn(f32) -> Message + 'a,
) -> GradientStrip<'a, Message> {
    let stops = vec![
        (0.0, Color { a: 0.0, ..color }),
        (1.0, Color { a: 1.0, ..color }),
    ];
    GradientStrip {
        value: a.clamp(0.0, 1.0),
        stops,
        on_change: Box::new(on_change),
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for GradientStrip<'_, Message>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DragState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DragState::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(STRIP_LEN),
            height: Length::Fixed(STRIP_THICK),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fixed(STRIP_LEN), Length::Fixed(STRIP_THICK))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<DragState>();
        let bounds = layout.bounds();

        let publish = |shell: &mut Shell<'_, Message>, point: iced::Point| {
            let t = ((point.x - bounds.x) / bounds.width).clamp(0.0, 1.0);
            shell.publish((self.on_change)(t));
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(point) = cursor.position_over(bounds) {
                    state.is_dragging = true;
                    publish(shell, point);
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.is_dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.is_dragging {
                    if let Some(point) = cursor.land().position() {
                        publish(shell, point);
                        shell.capture_event();
                    }
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        let mut ramp = Linear::new(angle_right());
        for (offset, color) in &self.stops {
            ramp = ramp.add_stop(*offset, *color);
        }
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    radius: (STRIP_THICK / 2.0).into(),
                    ..Border::default()
                },
                ..renderer::Quad::default()
            },
            Background::Gradient(ramp.into()),
        );

        // Thumb: a vertical capsule straddling the rail at the value.
        let half = STRIP_THICK / 2.0 + 2.0;
        let cx = (bounds.x + self.value.clamp(0.0, 1.0) * bounds.width)
            .clamp(bounds.x + 3.0, bounds.x + bounds.width - 3.0);
        thumb_ring(renderer, cx, bounds.y + bounds.height / 2.0, half);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<DragState>();
        if state.is_dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message, Theme, Renderer> From<GradientStrip<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(w: GradientStrip<'a, Message>) -> Self {
        Element::new(w)
    }
}

/// Draw a ring thumb centred at `(cx, cy)`: a dark outer ring for
/// contrast on light surfaces and a white inner ring on top. Transparent
/// fill so the underlying colour shows through.
fn thumb_ring<Renderer: renderer::Renderer>(renderer: &mut Renderer, cx: f32, cy: f32, r: f32) {
    let ring = |renderer: &mut Renderer, radius: f32, width: f32, color: Color| {
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: cx - radius,
                    y: cy - radius,
                    width: radius * 2.0,
                    height: radius * 2.0,
                },
                border: Border {
                    radius: radius.into(),
                    width,
                    color,
                },
                ..renderer::Quad::default()
            },
            Background::Color(Color::TRANSPARENT),
        );
    };
    ring(
        renderer,
        r + 1.0,
        1.0,
        Color::from_rgba(0.0, 0.0, 0.0, 0.45),
    );
    ring(renderer, r, 2.0, Color::WHITE);
}
