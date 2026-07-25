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
    Alignment, Border, Color, Element, Event, Length, Padding, Point, Rectangle, Size, Theme,
};

use crate::components::style::{HAIRLINE_A, RADIUS_XL, SPACE_LG, mix_white};
use crate::fonts;

/// Titlebar strip height, logical px. Taller than the old 28px strip so
/// the traffic light and title breathe like macOS window chrome.
pub const HEIGHT: f32 = 38.0;

/// Corner radius for a floating window frame (matches kit `RADIUS_XL`).
pub const WINDOW_RADIUS: f32 = RADIUS_XL;

/// Inward edge/corner resize grip thickness (logical px). One size for both
/// so the eight regions tile the perimeter without gaps or overlaps — a
/// hairline-thick mismatch was letting the default cursor flash on the
/// border, and taller corner layers were stealing mid-edge hits.
const GRIP: f32 = 14.0;

/// Outer border width. Face content is inset by this so children never
/// paint over the hairline (same trick as kit `card`).
const FRAME_BORDER: f32 = 1.0;

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
/// titlebar cannot paint over the top/side edges.
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

/// Invisible full-window rim that maps the pointer to one of eight resize
/// directions when it sits within [`GRIP`] px of the edge (corners win on
/// the g×g squares). Outside the rim, interaction is `None` so the stack
/// falls through to titlebar/content.
struct ResizeRim<'a, Message> {
    on_resize: Box<dyn Fn(Direction) -> Message + 'a>,
    grip: f32,
}

impl<'a, Message> ResizeRim<'a, Message> {
    fn new(on_resize: impl Fn(Direction) -> Message + 'a) -> Self {
        Self {
            on_resize: Box::new(on_resize),
            grip: GRIP,
        }
    }
}

/// Which resize direction contains `p` inside `bounds`, if any.
/// Corners take priority over pure edges when the pointer is in a g×g square.
fn resize_zone(bounds: Rectangle, p: Point, grip: f32) -> Option<Direction> {
    let x0 = bounds.x;
    let y0 = bounds.y;
    let x1 = bounds.x + bounds.width;
    let y1 = bounds.y + bounds.height;
    // Inclusive outer edge, exclusive inner — covers the hairline border
    // pixel and the inward grip band with no gap.
    let left = p.x >= x0 && p.x < x0 + grip;
    let right = p.x < x1 && p.x >= x1 - grip;
    let top = p.y >= y0 && p.y < y0 + grip;
    let bottom = p.y < y1 && p.y >= y1 - grip;

    match (top, bottom, left, right) {
        (true, _, true, _) => Some(Direction::NorthWest),
        (true, _, _, true) => Some(Direction::NorthEast),
        (_, true, true, _) => Some(Direction::SouthWest),
        (_, true, _, true) => Some(Direction::SouthEast),
        (true, _, _, _) => Some(Direction::North),
        (_, true, _, _) => Some(Direction::South),
        (_, _, true, _) => Some(Direction::West),
        (_, _, _, true) => Some(Direction::East),
        _ => None,
    }
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
        let Some(dir) = resize_zone(layout.bounds(), p, self.grip) else {
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
        match resize_zone(layout.bounds(), p, self.grip) {
            Some(dir) => interaction_for(dir),
            None => mouse::Interaction::None,
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        // Invisible — hit-test only.
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
        let g = 14.0;
        // Mid-right → East (not a corner). Direction has no PartialEq.
        assert!(matches!(
            resize_zone(b, Point::new(b.x + b.width - 2.0, b.y + b.height / 2.0), g),
            Some(Direction::East)
        ));
        assert!(matches!(
            resize_zone(b, Point::new(b.x + 2.0, b.y + b.height / 2.0), g),
            Some(Direction::West)
        ));
        assert!(matches!(
            resize_zone(b, Point::new(b.x + b.width - 2.0, b.y + b.height - 2.0), g),
            Some(Direction::SouthEast)
        ));
        // Hairline on the right edge (outermost pixel) → East.
        assert!(matches!(
            resize_zone(
                b,
                Point::new(b.x + b.width - 0.5, b.y + b.height / 2.0),
                g
            ),
            Some(Direction::East)
        ));
        // Centre → None (pass through).
        assert!(
            resize_zone(b, Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0), g).is_none()
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
    let r = (WINDOW_RADIUS - FRAME_BORDER).max(0.0);
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
    let r = (WINDOW_RADIUS - FRAME_BORDER).max(0.0);
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
