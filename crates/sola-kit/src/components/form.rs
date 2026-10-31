//! Settings-grade form primitives — horizontal rows plus checkbox /
//! toggler styles so P8 apps inherit a single path instead of inventing
//! layout per screen.
//!
//! State stays parent-owned: these are layout + style helpers over iced
//! widgets, not self-contained form controllers.

use iced::widget::row::Row;
use iced::widget::{checkbox, row, toggler};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};

use crate::components::style::{RADIUS_SM, SPACE_MD};
use crate::components::text::body;

/// Target height for a settings row (label left, control right).
const ROW_H: f32 = 32.0;

/// Horizontal settings row: label (left, fill) | control (right, shrink).
///
/// Height ~32; vertically centered; no card chrome. Stack with
/// `column![...].spacing(SPACE_MD)` for vertical rhythm.
///
/// Label is body (13) at full contrast — not muted — unless the caller
/// dims it themselves for an inactive row.
///
/// ```ignore
/// form_row(
///     "Notifications",
///     toggler(enabled).on_toggle(Msg::Set).style(toggle_style),
/// )
/// ```
pub fn form_row<'a, Message: 'a>(
    label: impl Into<String>,
    control: impl Into<Element<'a, Message, Theme>>,
) -> Row<'a, Message, Theme> {
    row![body(label.into()).width(Length::Fill), control.into(),]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center)
        .height(Length::Fixed(ROW_H))
        .width(Length::Fill)
}

/// Checkbox style: selected = accent fill + check; unselected = raised
/// fill with hairline border (macOS-like quiet box).
pub fn checkbox_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let p = theme.extended_palette();
    let (is_checked, hover, disabled) = match status {
        checkbox::Status::Active { is_checked } => (is_checked, false, false),
        checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
        checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
    };

    let (background, border_color, icon_color) = if is_checked {
        let accent = if hover {
            p.primary.strong
        } else {
            p.primary.base
        };
        let accent = if disabled {
            p.background.strong
        } else {
            accent
        };
        (
            Background::Color(accent.color),
            accent.color,
            p.primary.base.text,
        )
    } else {
        let fill = if hover {
            p.background.strong.color
        } else {
            p.background.weaker.color
        };
        let fill = if disabled {
            p.background.weak.color
        } else {
            fill
        };
        (
            Background::Color(fill),
            p.background.stronger.color,
            p.background.base.text,
        )
    };

    let mut style = checkbox::Style {
        background,
        icon_color,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: RADIUS_SM.into(),
        },
        text_color: None,
    };

    if disabled {
        // Soften disabled so it reads inactive without vanishing.
        if let Background::Color(c) = style.background {
            style.background = Background::Color(Color { a: c.a * 0.6, ..c });
        }
        style.icon_color = Color {
            a: style.icon_color.a * 0.6,
            ..style.icon_color
        };
        style.border.color = Color {
            a: style.border.color.a * 0.6,
            ..style.border.color
        };
    }

    style
}

/// Toggler style: on = accent track, its text color for the knob; off = a
/// hairline-grey track with a muted-text knob (lit to the text color on
/// hover), so an off switch reads on a raised row. Disabled: the same at
/// part alpha.
pub fn toggle_style(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let p = theme.extended_palette();
    let (is_toggled, hovered, disabled) = match status {
        toggler::Status::Active { is_toggled } => (is_toggled, false, false),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true, false),
        toggler::Status::Disabled { is_toggled } => (is_toggled, false, true),
    };

    let (background, foreground) = match (is_toggled, hovered) {
        (true, false) => (p.primary.base.color, p.primary.base.text),
        (true, true) => (
            p.primary.base.color,
            Color {
                a: 0.85,
                ..p.primary.base.text
            },
        ),
        (false, false) => (p.background.stronger.color, p.secondary.base.text),
        (false, true) => (p.background.stronger.color, p.background.base.text),
    };
    let faded = |c: Color, a: f32| if disabled { Color { a: c.a * a, ..c } } else { c };
    let (background, foreground) = (faded(background, 0.4), faded(foreground, 0.5));

    toggler::Style {
        background: background.into(),
        foreground: foreground.into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.12,
    }
}
