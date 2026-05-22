//! Pre-styled toolbar button — condensed-bold label, kit accent
//! color when pressed, kit border on hover. Anchors the visual
//! language for top-of-window toolbars (the monitor's pause/clear,
//! settings panels, etc.).

use iced::widget::{button, text};
use iced::{Element, Length};

use crate::fonts;

/// Compact toolbar button with the kit's condensed-bold label
/// font and a fixed minimum width so a row of them aligns
/// regardless of label length. Returns the configured button
/// without an `on_press` so the caller picks whether to enable it.
pub fn toolbar_button<'a, Message>(
    label: &'a str,
) -> button::Button<'a, Message>
where
    Message: Clone + 'a,
{
    button(
        text(label)
            .font(fonts::CONDENSED_BOLD)
            .size(12),
    )
    .padding([4, 10])
    .width(Length::Shrink)
}

/// Boxed `Element` form for callers that want to stash a row of
/// already-wired buttons in a `Vec<Element>`. Equivalent to
/// `toolbar_button(label).on_press(msg).into()`.
pub fn toolbar_button_msg<'a, Message>(
    label: &'a str,
    on_press: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    toolbar_button(label).on_press(on_press).into()
}
