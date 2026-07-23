//! Inline approval strip: the pending action (`App.pending`) plus
//! Approve / Always allow / Deny buttons wired to `agent_send` via `Msg`.
//! Borders/fills only — this iced stack does not blur shadows.
use iced::widget::{column, container, row};
use iced::{Background, Border, Element, Length, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{RADIUS_LG, SPACE_LG, SPACE_MD};
use sola_kit::components::text as kit_text;

use crate::{Msg, PendingApproval};

pub(crate) fn strip<'a>(p: &'a PendingApproval, theme: &Theme) -> Element<'a, Msg> {
    let pal = theme.extended_palette();
    let bg = pal.background.weak.color;
    let border = pal.warning.base.color;

    let buttons = row![
        kit_btn::labeled("Approve", kit_btn::primary).on_press(Msg::Approve),
        kit_btn::labeled("Always allow", kit_btn::secondary).on_press(Msg::Always),
        kit_btn::labeled("Deny", kit_btn::danger).on_press(Msg::Deny),
    ]
    .spacing(SPACE_MD);

    let body = column![
        kit_text::subheading(format!("Allow {}?", p.tool)),
        kit_text::code(p.preview.as_str()).style(kit_text::muted),
        buttons,
    ]
    .spacing(SPACE_MD);

    container(body)
        .padding(SPACE_LG)
        .width(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border,
                width: 1.0,
                radius: RADIUS_LG.into(),
            },
            ..container::Style::default()
        })
        .into()
}
