//! In-window titlebar for a floating kit app that opts into drawing chrome.
//!
//! macOS-adjacent floating chrome: taller bar, left traffic-light close
//! (solid circle, no glyph), horizontally centered title, whole bar is a
//! drag handle. Pair with [`floating_frame`] for rounded window corners and
//! edge/corner resize grips.
//!
//! Borders/fills only — no drop shadow (they render hard here).

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, renderer};
use iced::border::Radius;
use iced::widget::{Space, button, container, mouse_area, row, stack, text};
use iced::window::Direction;
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Padding, Point, Rectangle, Size,
    Theme,
};

use crate::components::style::{HAIRLINE_A, RADIUS_XL, SPACE_LG, mix_white};
use crate::fonts;

/// Titlebar strip height, logical px. Taller than the old 28px strip so
/// the traffic light and title breathe like macOS window chrome.
pub const HEIGHT: f32 = 38.0;

/// Corner radius for a floating window frame (matches kit `RADIUS_XL`).
pub const WINDOW_RADIUS: f32 = RADIUS_XL;

/// Inward edge resize band (logical px). Straight sides only — corners use
/// the larger [`CORNER_GRIP`] square so the rounded visual arc stays inside
/// a single diagonal zone (no edge↔corner cursor twiggle on the curve).
const EDGE_GRIP: f32 = 12.0;
/// Square corner hit box (logical px). ≥ [`WINDOW_RADIUS`] so the
/// transparent "ear" between the curved paint and the AABB corner still
/// belongs to this window for pointer hits (obscures apps below).
const CORNER_GRIP: f32 = 18.0;
/// Nearly-invisible alpha for the square corner pads. Non-zero so the
/// buffer isn't empty there; low enough to stay visually transparent.
const CORNER_PAD_A: f32 = 0.02;

/// Outer border width. Face content is inset by this so children never
/// paint over the hairline (same trick as kit `card`).
const FRAME_BORDER: f32 = 1.0;

/// Inner face corner radius (outer [`WINDOW_RADIUS`] minus the 1px pad).
pub(crate) fn face_radius() -> f32 {
    (WINDOW_RADIUS - FRAME_BORDER).max(0.0)
}

/// Traffic-light close disc diameter.
const CLOSE_DOT: f32 = 12.0;

/// Fixed width of the left control cluster — mirrored on the right so the
/// title can sit truly centered in the remaining space.
const CONTROLS_W: f32 = 52.0;

/// Classic macOS close traffic light (`#FF5F57`).
const CLOSE_RED: Color = Color::from_rgb(1.0, 0.372_5, 0.341_2);
/// Slightly darker rim so the disc reads on graphite chrome.
const CLOSE_RIM: Color = Color::from_rgb(0.78, 0.22, 0.20);
/// Hover / pressed variants.
const CLOSE_HOVER: Color = Color::from_rgb(1.0, 0.45, 0.42);
const CLOSE_PRESSED: Color = Color::from_rgb(0.90, 0.30, 0.27);

/// Titlebar strip alone — title + left close circle + drag handle.
///
/// Prefer [`floating_frame`] when the whole window should round its corners.
pub fn titlebar<'a, Message>(
    title: impl Into<String>,
    on_drag: Message,
    on_close: Message,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    titlebar_inner(title, on_drag, on_close, /*round_top*/ false)
}

/// Floating window chrome: rounded outer frame, titlebar on top, content
/// below, and invisible edge/corner resize grips around the perimeter.
///
/// Structure mirrors kit [`crate::components::card`]: a 1px outer pad keeps
/// the hairline border outside the content layout box so the full-bleed
/// titlebar cannot paint over the top/side edges. The inner face is clipped
/// to a rounded rect (iced's `clip(true)` is AABB-only, so overflowing
/// children would otherwise square the bottom corners).
///
/// The host window should be `transparent: true` and (while floating) use
/// [`crate::theme::overlay`] so the corners outside this frame stay see-through.
///
/// `on_resize` is invoked with an iced [`Direction`] when a grip is pressed;
/// the consumer should return `iced::window::drag_resize(id, direction)`.
pub fn floating_frame<'a, Message>(
    title: impl Into<String>,
    on_drag: Message,
    on_close: Message,
    on_resize: impl Fn(Direction) -> Message + 'a,
    content: Element<'a, Message, Theme>,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let body = iced::widget::column![
        titlebar_inner(title, on_drag, on_close, /*round_top*/ true),
        content,
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    // iced `clip(true)` is AABB-only. Punch the rounded-rect ears so
    // full-bleed children cannot square the bottom corners.
    let body = super::float_clip::wrap(body, face_radius());

    // Inner face: solid fill + rounded corners. No border — the outer
    // frame owns the continuous hairline.
    let face = container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(face_style)
        .clip(true);

    // Outer frame: 1px pad = border ring that children cannot cover.
    let framed = container(face)
        .padding(FRAME_BORDER)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(frame_style);

    // Inward resize grips overlaid on the frame — entirely inside the
    // window bounds so they never steal hits from apps behind the float.
    with_resize_grips(framed.into(), on_resize)
}

/// Overlay a pure-geometry resize rim on `inner`. Hit testing uses window
/// bounds math (not nested layout strips), so mid-edge / corner / hairline
/// regions stay correct and confined inside the surface.
fn with_resize_grips<'a, Message>(
    inner: Element<'a, Message, Theme>,
    on_resize: impl Fn(Direction) -> Message + 'a,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    stack![inner, ResizeRim::new(on_resize)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Full-window rim: square corner pads (drawn nearly invisible) + eight-way
/// resize hit zones. Outside the rim, interaction is `None` so the stack
/// falls through to titlebar/content.
struct ResizeRim<'a, Message> {
    on_resize: Box<dyn Fn(Direction) -> Message + 'a>,
    edge: f32,
    corner: f32,
}

impl<'a, Message> ResizeRim<'a, Message> {
    fn new(on_resize: impl Fn(Direction) -> Message + 'a) -> Self {
        Self {
            on_resize: Box::new(on_resize),
            edge: EDGE_GRIP,
            corner: CORNER_GRIP,
        }
    }
}

/// Which resize direction contains `p` inside `bounds`, if any.
///
/// Corners are **axis-aligned squares** of side `corner` (not just the
/// curved paint), so the transparent ear outside the rounded chrome still
/// counts as this window. Straight edges use the thinner `edge` band and
/// stop where the corner squares begin.
fn resize_zone(bounds: Rectangle, p: Point, edge: f32, corner: f32) -> Option<Direction> {
    let x0 = bounds.x;
    let y0 = bounds.y;
    let x1 = bounds.x + bounds.width;
    let y1 = bounds.y + bounds.height;
    // Inclusive outer edge — covers the hairline pixel.
    let dl = p.x - x0;
    let dr = x1 - p.x;
    let dt = p.y - y0;
    let db = y1 - p.y;
    if dl < 0.0 || dr < 0.0 || dt < 0.0 || db < 0.0 {
        return None;
    }

    let in_left_c = dl < corner;
    let in_right_c = dr < corner;
    let in_top_c = dt < corner;
    let in_bottom_c = db < corner;

    // Square corner cells first (full AABB corner, not just the curve).
    if in_top_c && in_left_c {
        return Some(Direction::NorthWest);
    }
    if in_top_c && in_right_c {
        return Some(Direction::NorthEast);
    }
    if in_bottom_c && in_left_c {
        return Some(Direction::SouthWest);
    }
    if in_bottom_c && in_right_c {
        return Some(Direction::SouthEast);
    }

    // Straight edges — only outside the corner squares.
    if dt < edge {
        return Some(Direction::North);
    }
    if db < edge {
        return Some(Direction::South);
    }
    if dl < edge {
        return Some(Direction::West);
    }
    if dr < edge {
        return Some(Direction::East);
    }
    None
}

fn interaction_for(dir: Direction) -> mouse::Interaction {
    // Prefer Col/Row resize over Horizontally/Vertically: iced maps the latter
    // to XDG `ew-resize` / `ns-resize`, which McMojave (and many themes) lack —
    // wlroots then silently falls back to default. `col-resize` / `row-resize`
    // exist and are what the monitor divider already uses for the same reason.
    // Diagonals (`nwse-resize` / `nesw-resize`) are present in McMojave.
    match dir {
        Direction::North | Direction::South => mouse::Interaction::ResizingRow,
        Direction::East | Direction::West => mouse::Interaction::ResizingColumn,
        Direction::NorthWest | Direction::SouthEast => mouse::Interaction::ResizingDiagonallyDown,
        Direction::NorthEast | Direction::SouthWest => mouse::Interaction::ResizingDiagonallyUp,
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for ResizeRim<'a, Message>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return;
        };
        let Some(p) = cursor.position() else {
            return;
        };
        let Some(dir) = resize_zone(layout.bounds(), p, self.edge, self.corner) else {
            return;
        };
        shell.publish((self.on_resize)(dir));
        shell.capture_event();
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let Some(p) = cursor.position() else {
            return mouse::Interaction::None;
        };
        match resize_zone(layout.bounds(), p, self.edge, self.corner) {
            Some(dir) => interaction_for(dir),
            None => mouse::Interaction::None,
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
        // Paint nearly-invisible square pads in the four AABB corners so the
        // transparent "ears" outside the rounded chrome still own pointer
        // hits (and visually obscure apps below in that tiny square).
        // Own layer so this runs *after* the face's rounded punch (a later
        // layer than the child content).
        let b = layout.bounds();
        renderer.with_layer(b, |renderer| {
            let c = self.corner;
            let pad = Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: CORNER_PAD_A,
            };
            let corners = [
                Rectangle {
                    x: b.x,
                    y: b.y,
                    width: c,
                    height: c,
                },
                Rectangle {
                    x: b.x + b.width - c,
                    y: b.y,
                    width: c,
                    height: c,
                },
                Rectangle {
                    x: b.x,
                    y: b.y + b.height - c,
                    width: c,
                    height: c,
                },
                Rectangle {
                    x: b.x + b.width - c,
                    y: b.y + b.height - c,
                    width: c,
                    height: c,
                },
            ];
            for rect in corners {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: rect,
                        border: Border::default(),
                        ..renderer::Quad::default()
                    },
                    Background::Color(pad),
                );
            }
        });
    }
}

impl<'a, Message, Theme, Renderer> From<ResizeRim<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(rim: ResizeRim<'a, Message>) -> Self {
        Element::new(rim)
    }
}

#[cfg(test)]
mod resize_zone_tests {
    use super::*;

    fn bounds() -> Rectangle {
        Rectangle {
            x: 100.0,
            y: 50.0,
            width: 400.0,
            height: 300.0,
        }
    }

    #[test]
    fn mid_edges_and_corners() {
        let b = bounds();
        let e = 12.0;
        let c = 18.0;
        // Mid-right → East (not a corner). Direction has no PartialEq.
        assert!(matches!(
            resize_zone(
                b,
                Point::new(b.x + b.width - 2.0, b.y + b.height / 2.0),
                e,
                c
            ),
            Some(Direction::East)
        ));
        assert!(matches!(
            resize_zone(b, Point::new(b.x + 2.0, b.y + b.height / 2.0), e, c),
            Some(Direction::West)
        ));
        // Geometric AABB corner (outside the visual curve) → SouthEast.
        assert!(matches!(
            resize_zone(
                b,
                Point::new(b.x + b.width - 1.0, b.y + b.height - 1.0),
                e,
                c
            ),
            Some(Direction::SouthEast)
        ));
        // On the rounded-arc region near SE → still SouthEast (square corner cell).
        assert!(matches!(
            resize_zone(
                b,
                Point::new(b.x + b.width - 4.0, b.y + b.height - 4.0),
                e,
                c
            ),
            Some(Direction::SouthEast)
        ));
        // Hairline mid-right → East.
        assert!(matches!(
            resize_zone(
                b,
                Point::new(b.x + b.width - 0.5, b.y + b.height / 2.0),
                e,
                c
            ),
            Some(Direction::East)
        ));
        // Centre → None (pass through).
        assert!(
            resize_zone(
                b,
                Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0),
                e,
                c
            )
            .is_none()
        );
    }

    #[test]
    fn aabb_corner_hits_resize_but_sits_outside_the_face_curve() {
        let b = bounds();
        let corner = Point::new(b.x + b.width - 1.0, b.y + b.height - 1.0);
        assert!(matches!(
            resize_zone(b, corner, 12.0, 18.0),
            Some(Direction::SouthEast)
        ));
        let dist = super::super::float_clip::rounded_rect_dist(corner, b, face_radius());
        assert!(
            dist >= 0.5,
            "visual clip must treat the AABB corner as an ear (dist={dist})"
        );
    }
}

fn titlebar_inner<'a, Message>(
    title: impl Into<String>,
    on_drag: Message,
    on_close: Message,
    round_top: bool,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let label = text(title.into())
        .font(fonts::ui_medium())
        .size(13)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            iced::widget::text::Style {
                color: Some(p.background.base.text),
            }
        });

    // Solid circle — no ✕ glyph. Empty contents; the style paints the disc.
    let close = button(Space::new().width(CLOSE_DOT).height(CLOSE_DOT))
        .padding(0)
        .width(Length::Fixed(CLOSE_DOT))
        .height(Length::Fixed(CLOSE_DOT))
        .style(close_style)
        .on_press(on_close);

    let left = container(close)
        .width(Length::Fixed(CONTROLS_W))
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Center)
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: SPACE_LG,
        });

    let center = container(label)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    // Mirror the left cluster so the title's geometric center is the bar's.
    let right = Space::new()
        .width(Length::Fixed(CONTROLS_W))
        .height(Length::Shrink);

    let bar_row = container(
        row![left, center, right]
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(HEIGHT - 1.0))
    .style(move |theme| bar_style(theme, round_top));

    // 1px hairline under the strip (iced Border is all-sides; a dedicated
    // row keeps the separator without boxing the whole bar).
    let rule = container(Space::new().width(Length::Fill).height(Length::Fixed(1.0)))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            let fill = p.background.weaker.color;
            container::Style {
                background: Some(mix_white(fill, HAIRLINE_A).into()),
                ..container::Style::default()
            }
        });

    let bar = iced::widget::column![bar_row, rule]
        .width(Length::Fill)
        .height(Length::Fixed(HEIGHT));

    mouse_area(bar).on_press(on_drag).into()
}

/// Traffic-light close: filled circle, subtle rim, brightens on hover.
fn close_style(_theme: &Theme, status: button::Status) -> button::Style {
    let (fill, rim) = match status {
        button::Status::Hovered => (CLOSE_HOVER, CLOSE_RIM),
        button::Status::Pressed => (CLOSE_PRESSED, CLOSE_RIM),
        button::Status::Disabled => (
            Color {
                a: 0.45,
                ..CLOSE_RED
            },
            Color {
                a: 0.45,
                ..CLOSE_RIM
            },
        ),
        button::Status::Active => (CLOSE_RED, CLOSE_RIM),
    };
    button::Style {
        background: Some(fill.into()),
        text_color: Color::TRANSPARENT,
        border: Border {
            color: rim,
            width: 0.5,
            radius: 999.0.into(),
        },
        shadow: iced::Shadow::default(),
        snap: true,
    }
}

/// Raised strip fill. Top corners round when embedded in [`floating_frame`]
/// so they match the outer window radius. Hairline is a separate row.
fn bar_style(theme: &Theme, round_top: bool) -> container::Style {
    let p = theme.extended_palette();
    let fill = p.background.weaker.color;
    // Face sits inside a 1px pad — shave the top radius so the bar meets
    // the outer rounded border cleanly.
    let r = face_radius();
    let radius = if round_top {
        Radius {
            top_left: r,
            top_right: r,
            bottom_right: 0.0,
            bottom_left: 0.0,
        }
    } else {
        Radius::default()
    };
    container::Style {
        background: Some(fill.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius,
        },
        ..container::Style::default()
    }
}

/// Inner face fill (no border). Radius matches the outer frame minus the
/// 1px pad so corners nest under the hairline.
fn face_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let fill = p.background.base.color;
    let fill = if fill.a < 0.01 {
        p.background.weaker.color
    } else {
        fill
    };
    let r = face_radius();
    container::Style {
        background: Some(fill.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: r.into(),
        },
        ..container::Style::default()
    }
}

/// Outer rounded hairline. Background matches the face so the 1px pad
/// ring is continuous graphite (not a transparent gap).
fn frame_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let fill = p.background.base.color;
    let fill = if fill.a < 0.01 {
        p.background.weaker.color
    } else {
        fill
    };
    container::Style {
        background: Some(fill.into()),
        border: Border {
            color: mix_white(fill, HAIRLINE_A),
            width: FRAME_BORDER,
            radius: WINDOW_RADIUS.into(),
        },
        ..container::Style::default()
    }
}
