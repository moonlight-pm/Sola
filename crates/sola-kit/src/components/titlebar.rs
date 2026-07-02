//! In-window titlebar for a floating kit app that opts into drawing chrome.
//!
//! Title text + a close button; the whole bar is a drag handle. `on_drag`
//! fires on press anywhere on the bar; the close button consumes its own press
//! and fires `on_close` instead. The app maps `on_drag` to
//! `iced::window::drag(id)` and `on_close` to its close action.
//!
//! Borders/fills only — no drop shadow (they render hard here).

use iced::widget::{Space, button, container, mouse_area, row, text};
use iced::{Alignment, Border, Element, Length, Theme};

use crate::components::button as kit_btn;
use crate::components::text as kit_text;

/// Titlebar strip height, logical px.
pub const HEIGHT: f32 = 28.0;

pub fn titlebar<'a, Message>(
    title: impl Into<String>,
    on_drag: Message,
    on_close: Message,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let label = kit_text::body(title.into()).size(13);
    let close = button(text("✕").size(12))
        .padding([2, 8])
        .style(kit_btn::ghost)
        .on_press(on_close);

    let bar = container(
        row![label, Space::new().width(Length::Fill), close]
            .align_y(Alignment::Center)
            .spacing(8)
            .padding([0, 8]),
    )
    .width(Length::Fill)
    .height(Length::Fixed(HEIGHT))
    .style(bar_style);

    mouse_area(bar).on_press(on_drag).into()
}

/// Raised background + hairline bottom-ish border. No shadow.
fn bar_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(p.background.weak.color.into()),
        border: Border {
            color: p.background.strong.color,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}
