//! Split / column dividers — 1px hairline, fat hit strip, consumer colors.
//!
//! Layout reserves [`DIVIDER_HIT_PX`] so nested splits stay easy to grab.
//! Visually the strip is three bands — **a | line | b** — so the consumer
//! can match the sides to whatever surfaces are being split (terminal
//! cell bg, raised sidebar, card fill, …). Only the center band is the
//! hairline; when `a`/`b` match the adjacent panes the reserved hit
//! width disappears into the layout and the line reads as a true 1px
//! separator (no black/grey/black gutter).
//!
//! ```ignore
//! // Terminal pane split — both sides cell bg, quiet border line
//! let chrome = DividerColors::uniform(palette.bg, border);
//! vertical_divider_with(Msg::Drag, chrome)
//!
//! // Sidebar | content — raised on the left, canvas on the right
//! let chrome = DividerColors { a: raised, line: border, b: canvas };
//! ```
//!
//! Drag state stays with the caller (iced has no pointer-capture): the
//! divider emits `on_press`, and the consumer listens for that plus
//! global cursor motion / release.
//!
//! Cursor interaction is `ResizingColumn` / `ResizingRow` so winit→sctk
//! requests `col-resize` / `row-resize` (generic `ew-resize` / `ns-resize`
//! names are missing from many themes and wlroots falls back to default).

use iced::widget::{Space, column, container, mouse_area, row};
use iced::{Background, Border, Color, Element, Length, Theme, mouse};

/// Layout thickness of the draggable divider strip (logical px). The
/// visible hairline is exactly [`LINE_PX`] centered inside this; consumers
/// that compute pane rects from a split tree must use the same value.
pub const DIVIDER_HIT_PX: f32 = 8.0;

/// Painted hairline thickness (logical px). Keep at 1.0 — anything
/// thicker reads as a groove, not a macOS separator.
pub const LINE_PX: f32 = 1.0;

/// Per-band colours for a draggable divider: side toward pane **a**,
/// the **line**, side toward pane **b**.
///
/// Orientation: for a vertical split, `a` is left and `b` is right; for
/// a horizontal split, `a` is top and `b` is bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DividerColors {
    pub a: Color,
    pub line: Color,
    pub b: Color,
}

impl DividerColors {
    /// Same fill on both sides of the hairline (typical when both panes
    /// share a background).
    pub fn uniform(side: Color, line: Color) -> Self {
        Self {
            a: side,
            line,
            b: side,
        }
    }

    /// Theme defaults: canvas | border | canvas. Fine for generic
    /// chrome; prefer an explicit match when adjacent surfaces differ
    /// from the window canvas (terminal grids, raised sidebars, cards).
    pub fn from_theme(theme: &Theme) -> Self {
        let p = theme.extended_palette();
        Self {
            a: p.background.weakest.color,
            line: p.background.stronger.color,
            b: p.background.weakest.color,
        }
    }

    /// Raised panel | border | canvas — common for sidebar | content.
    pub fn raised_to_canvas(theme: &Theme) -> Self {
        let p = theme.extended_palette();
        Self {
            a: p.background.weaker.color,
            line: p.background.stronger.color,
            b: p.background.weakest.color,
        }
    }

    /// Raised | border | raised — storybook cards / same-elevation panes.
    pub fn raised(theme: &Theme) -> Self {
        let p = theme.extended_palette();
        Self {
            a: p.background.weaker.color,
            line: p.background.stronger.color,
            b: p.background.weaker.color,
        }
    }
}

fn fill(color: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(color)),
        border: Border::default(),
        ..container::Style::default()
    }
}

fn side_px() -> f32 {
    ((DIVIDER_HIT_PX - LINE_PX) / 2.0).max(0.0)
}

/// Style for the 1px painted hairline when used alone (non-drag
/// [`horizontal_divider`]). Border atom.
pub fn line_style(theme: &Theme) -> container::Style {
    fill(theme.extended_palette().background.stronger.color)
}

/// Draggable vertical divider with theme-default colours
/// ([`DividerColors::from_theme`]).
pub fn vertical_divider<'a, Message>(on_press: Message) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let side = side_px();
    let a = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(side))
        .height(Length::Fill)
        .style(|t: &Theme| fill(DividerColors::from_theme(t).a));
    let line = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(LINE_PX))
        .height(Length::Fill)
        .style(|t: &Theme| fill(DividerColors::from_theme(t).line));
    let b = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(side))
        .height(Length::Fill)
        .style(|t: &Theme| fill(DividerColors::from_theme(t).b));

    mouse_area(
        row![a, line, b]
            .width(Length::Fixed(DIVIDER_HIT_PX))
            .height(Length::Fill),
    )
    .interaction(mouse::Interaction::ResizingColumn)
    .on_press(on_press)
    .into()
}

/// Draggable vertical divider with explicit **a | line | b** colours.
pub fn vertical_divider_with<'a, Message>(
    on_press: Message,
    colors: DividerColors,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let side = side_px();
    let a = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(side))
        .height(Length::Fill)
        .style(move |_t: &Theme| fill(colors.a));
    let line = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(LINE_PX))
        .height(Length::Fill)
        .style(move |_t: &Theme| fill(colors.line));
    let b = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(side))
        .height(Length::Fill)
        .style(move |_t: &Theme| fill(colors.b));

    mouse_area(
        row![a, line, b]
            .width(Length::Fixed(DIVIDER_HIT_PX))
            .height(Length::Fill),
    )
    .interaction(mouse::Interaction::ResizingColumn)
    .on_press(on_press)
    .into()
}

/// Draggable horizontal divider with theme-default colours.
pub fn horizontal_divider_drag<'a, Message>(on_press: Message) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let side = side_px();
    let a = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(side))
        .style(|t: &Theme| fill(DividerColors::from_theme(t).a));
    let line = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(LINE_PX))
        .style(|t: &Theme| fill(DividerColors::from_theme(t).line));
    let b = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(side))
        .style(|t: &Theme| fill(DividerColors::from_theme(t).b));

    mouse_area(
        column![a, line, b]
            .width(Length::Fill)
            .height(Length::Fixed(DIVIDER_HIT_PX)),
    )
    .interaction(mouse::Interaction::ResizingRow)
    .on_press(on_press)
    .into()
}

/// Draggable horizontal divider with explicit **a | line | b** colours.
pub fn horizontal_divider_drag_with<'a, Message>(
    on_press: Message,
    colors: DividerColors,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let side = side_px();
    let a = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(side))
        .style(move |_t: &Theme| fill(colors.a));
    let line = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(LINE_PX))
        .style(move |_t: &Theme| fill(colors.line));
    let b = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(side))
        .style(move |_t: &Theme| fill(colors.b));

    mouse_area(
        column![a, line, b]
            .width(Length::Fill)
            .height(Length::Fixed(DIVIDER_HIT_PX)),
    )
    .interaction(mouse::Interaction::ResizingRow)
    .on_press(on_press)
    .into()
}

/// A non-interactive 1px horizontal divider line (no hit strip).
pub fn horizontal_divider<'a, Message: 'a>() -> Element<'a, Message, Theme> {
    container(Space::new().width(Length::Fill).height(Length::Fixed(LINE_PX)))
        .width(Length::Fill)
        .height(Length::Fixed(LINE_PX))
        .style(line_style)
        .into()
}
