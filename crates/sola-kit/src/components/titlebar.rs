//! In-window titlebar for a floating kit app that opts into drawing chrome.
//!
//! macOS-adjacent floating chrome: taller bar, left traffic-light close
//! (solid circle, no glyph), horizontally centered title, whole bar is a
//! drag handle. Pair with [`floating_frame`] for rounded window corners and
//! edge/corner resize grips.
//!
//! Borders/fills only — no drop shadow (they render hard here).

use iced::border::Radius;
use iced::mouse;
use iced::widget::{Space, button, container, mouse_area, row, text};
use iced::window::Direction;
use iced::{Alignment, Border, Color, Element, Length, Padding, Theme};

use crate::components::style::{HAIRLINE_A, RADIUS_XL, SPACE_LG, mix_white};
use crate::fonts;

/// Titlebar strip height, logical px. Taller than the old 28px strip so
/// the traffic light and title breathe like macOS window chrome.
pub const HEIGHT: f32 = 38.0;

/// Corner radius for a floating window frame (matches kit `RADIUS_XL`).
pub const WINDOW_RADIUS: f32 = RADIUS_XL;

/// Edge resize grip thickness (logical px). Thin enough not to steal much
/// content, thick enough to grab without hunting.
const EDGE_GRIP: f32 = 10.0;
/// Corner grip size — larger than the edge strip so diagonal grabs are easy.
const CORNER_GRIP: f32 = 20.0;

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

    // Resize grips take real layout space around the frame (not a stack
    // overlay). Overlay side strips were too easy to miss / lose to the
    // content layer; a 3×3 chrome with fat corners is reliably hittable.
    with_resize_grips(framed.into(), on_resize)
}

/// 3×3 resize chrome: large corner squares + edge strips around `inner`.
/// Corners use [`CORNER_GRIP`]; edges use [`EDGE_GRIP`] thickness (top/bottom
/// strips are corner-tall so the whole top/bottom band is easy to grab).
fn with_resize_grips<'a, Message>(
    inner: Element<'a, Message, Theme>,
    on_resize: impl Fn(Direction) -> Message + 'a,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let e = EDGE_GRIP;
    let c = CORNER_GRIP;

    let grip = |dir: Direction, interaction: mouse::Interaction| -> Element<'a, Message, Theme> {
        mouse_area(
            Space::new()
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .interaction(interaction)
        .on_press(on_resize(dir))
        .into()
    };

    let top = row![
        container(grip(
            Direction::NorthWest,
            mouse::Interaction::ResizingDiagonallyDown,
        ))
        .width(Length::Fixed(c))
        .height(Length::Fixed(c)),
        container(grip(
            Direction::North,
            mouse::Interaction::ResizingVertically,
        ))
        .width(Length::Fill)
        .height(Length::Fixed(c)),
        container(grip(
            Direction::NorthEast,
            mouse::Interaction::ResizingDiagonallyUp,
        ))
        .width(Length::Fixed(c))
        .height(Length::Fixed(c)),
    ]
    .width(Length::Fill)
    .height(Length::Fixed(c));

    let middle = row![
        container(grip(
            Direction::West,
            mouse::Interaction::ResizingHorizontally,
        ))
        .width(Length::Fixed(e))
        .height(Length::Fill),
        container(inner)
            .width(Length::Fill)
            .height(Length::Fill),
        container(grip(
            Direction::East,
            mouse::Interaction::ResizingHorizontally,
        ))
        .width(Length::Fixed(e))
        .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    let bottom = row![
        container(grip(
            Direction::SouthWest,
            mouse::Interaction::ResizingDiagonallyUp,
        ))
        .width(Length::Fixed(c))
        .height(Length::Fixed(c)),
        container(grip(
            Direction::South,
            mouse::Interaction::ResizingVertically,
        ))
        .width(Length::Fill)
        .height(Length::Fixed(c)),
        container(grip(
            Direction::SouthEast,
            mouse::Interaction::ResizingDiagonallyDown,
        ))
        .width(Length::Fixed(c))
        .height(Length::Fixed(c)),
    ]
    .width(Length::Fill)
    .height(Length::Fixed(c));

    iced::widget::column![top, middle, bottom]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
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
