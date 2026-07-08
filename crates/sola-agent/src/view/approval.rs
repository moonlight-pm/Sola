//! Inline approval strip: the pending action (`App.pending`) plus
//! Approve / Always allow / Deny buttons wired to `agent_send` via `Msg`.
//! Borders/fills only — this iced stack does not blur shadows.
use iced::widget::{button, column, container, row, text};
use iced::{Background, Border, Element, Length, Padding, Theme};

use crate::{Msg, PendingApproval};

pub(crate) fn strip<'a>(p: &'a PendingApproval, theme: &Theme) -> Element<'a, Msg> {
    let pal = theme.extended_palette();
    let bg = pal.background.weak.color;
    let border = pal.warning.base.color;

    let buttons = row![
        button(text("Approve"))
            .style(sola_kit::components::button::primary)
            .on_press(Msg::Approve),
        button(text("Always allow"))
            .style(sola_kit::components::button::secondary)
            .on_press(Msg::Always),
        button(text("Deny"))
            .style(sola_kit::components::button::danger)
            .on_press(Msg::Deny),
    ]
    .spacing(8);

    let body = column![
        text(format!("Allow {}?", p.tool)).font(sola_kit::fonts::ui_medium()).size(14),
        text(p.preview.as_str())
            .font(sola_kit::fonts::mono())
            .size(12)
            .style(sola_kit::components::text::muted),
        buttons,
    ]
    .spacing(8);

    container(body)
        .padding(Padding::new(12.0))
        .width(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border { color: border, width: 1.0, radius: 8.0.into() },
            ..container::Style::default()
        })
        .into()
}
