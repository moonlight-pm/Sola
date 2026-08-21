//! Pre-styled toolbar button — medium-weight label, kit accent color
//! when pressed, kit border on hover. Anchors the visual language for
//! top-of-window toolbars (the monitor's pause/clear, settings panels,
//! etc.).
//!
//! The button itself uses iced's `button::Style`; the kit's named
//! [`style`] fn is what gives it the toolbar look. Apps that already
//! have a custom `iced::widget::button` and just want kit-toolbar
//! styling can pass `.style(sola_kit::components::toolbar::style)`
//! directly.

use std::time::Duration;

use iced::widget::text::IntoFragment;
use iced::widget::tooltip::Position as TooltipPosition;
use iced::widget::{button, container, svg, text, tooltip};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::components::icon::icon_svg;
use crate::components::popover;
use crate::components::style::{PAD_CONTROL_SM, RADIUS_SM};

use crate::fonts;

/// Compact toolbar button with the kit's medium-weight label font and
/// shrink-to-content width. Returns the configured button without an
/// `on_press` so the caller picks whether to enable it.
///
/// Density matches [`crate::components::button::labeled_sm`]: 12px type
/// + [`PAD_CONTROL_SM`].
///
/// `label` accepts both `&str` and `String` (anything that's
/// `IntoFragment` — matches iced's own `text(...)` signature).
pub fn toolbar_button<'a, Message>(label: impl IntoFragment<'a>) -> button::Button<'a, Message>
where
    Message: Clone + 'a,
{
    button(text(label).font(fonts::ui_medium()).size(12))
        .padding(PAD_CONTROL_SM)
        .width(Length::Shrink)
        .style(style)
}

/// Compact toolbar icon button — same density as [`toolbar_button`],
/// for chrome that has moved off unicode arrows.
pub fn toolbar_icon<'a, Message: Clone + 'a>(
    handle: iced::widget::svg::Handle,
    size: u16,
) -> button::Button<'a, Message> {
    button(icon_svg(handle, size))
        .padding(PAD_CONTROL_SM)
        .width(Length::Shrink)
        .style(style)
}

/// Icon tool with a delayed hover tooltip. `on_press = None` is muted
/// and has no tip — the action is not available.
pub fn toolbar_icon_tip<'a, Message: Clone + 'a>(
    handle: iced::widget::svg::Handle,
    tip: &'static str,
    on_press: Option<Message>,
) -> Element<'a, Message, Theme> {
    let enabled = on_press.is_some();
    let icon = if enabled {
        icon_svg(handle, 16)
    } else {
        svg(handle)
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(16.0))
            .style(|theme: &Theme, _status| svg::Style {
                color: Some(theme.extended_palette().secondary.base.text),
            })
            .into()
    };
    let mut btn = button(icon)
        .padding(PAD_CONTROL_SM)
        .width(Length::Shrink)
        .style(style);
    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }
    if !enabled {
        return btn.into();
    }
    let tip = container(text(tip).font(fonts::ui()).size(12))
        .padding(Padding {
            top: 5.0,
            right: 8.0,
            bottom: 5.0,
            left: 8.0,
        })
        .style(popover::style);
    tooltip(btn, tip, TooltipPosition::Bottom)
        .gap(6)
        .delay(Duration::from_millis(280))
        .into()
}

/// Boxed `Element` form for callers that want to stash a row of
/// already-wired buttons in a `Vec<Element>`. Equivalent to
/// `toolbar_button(label).on_press(msg).into()`.
pub fn toolbar_button_msg<'a, Message>(
    label: impl IntoFragment<'a>,
    on_press: Message,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    toolbar_button(label).on_press(on_press).into()
}

/// Style fn for toolbar buttons. Background lifts to BG_HOVER on
/// hover/press, transparent at rest. Borders are subtle so a row of
/// toolbar buttons reads as a single chrome strip, not a stack of
/// individual buttons.
pub fn style(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let bg = match status {
        button::Status::Hovered => p.background.strong.color,
        button::Status::Pressed => p.background.stronger.color,
        _ => Color::TRANSPARENT,
    };
    let text_color = match status {
        button::Status::Disabled => p.secondary.base.text,
        _ => p.background.base.text,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_SM.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}
