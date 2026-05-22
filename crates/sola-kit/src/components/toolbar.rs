//! Pre-styled toolbar button — condensed-bold label, kit accent color
//! when pressed, kit border on hover. Anchors the visual language for
//! top-of-window toolbars (the monitor's pause/clear, settings panels,
//! etc.).
//!
//! The button itself uses iced's `button::Style`; the kit's named
//! [`style`] fn is what gives it the toolbar look. Apps that already
//! have a custom `iced::widget::button` and just want kit-toolbar
//! styling can pass `.style(sola_kit::components::toolbar::style)`
//! directly.

use iced::widget::{button, text};
use iced::widget::text::IntoFragment;
use iced::{Background, Border, Color, Element, Length, Theme};

use crate::fonts;

/// Compact toolbar button with the kit's condensed-bold label font and
/// shrink-to-content width. Returns the configured button without an
/// `on_press` so the caller picks whether to enable it.
///
/// `label` accepts both `&str` and `String` (anything that's
/// `IntoFragment` — matches iced's own `text(...)` signature).
pub fn toolbar_button<'a, Message>(
    label: impl IntoFragment<'a>,
) -> button::Button<'a, Message>
where
    Message: Clone + 'a,
{
    button(text(label).font(fonts::CONDENSED_BOLD).size(12))
        .padding([4, 10])
        .width(Length::Shrink)
        .style(style)
}

/// Boxed `Element` form for callers that want to stash a row of
/// already-wired buttons in a `Vec<Element>`. Equivalent to
/// `toolbar_button(label).on_press(msg).into()`.
pub fn toolbar_button_msg<'a, Message>(
    label: impl IntoFragment<'a>,
    on_press: Message,
) -> Element<'a, Message>
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
            radius: 4.0.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}
