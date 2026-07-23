//! Permission approval strip — elevated card above the composer.

use iced::widget::{column, container, row, text};
use iced::{Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{RADIUS_LG, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::{Msg, PendingApproval};

pub(crate) fn strip(pending: &PendingApproval) -> Element<'_, Msg> {
    let mut actions = row![].spacing(SPACE_SM);
    for opt in &pending.options {
        let kind = opt.kind.to_lowercase();
        let style = if kind.contains("reject") || kind.contains("deny") {
            kit_btn::danger
        } else if kind.contains("allow_always") || kind.contains("always") {
            kit_btn::secondary
        } else {
            kit_btn::primary
        };
        actions = actions.push(
            kit_btn::labeled_sm(opt.name.as_str(), style)
                .on_press(Msg::PermissionPick(opt.option_id.clone())),
        );
    }
    if pending.options.is_empty() {
        actions = actions
            .push(
                kit_btn::labeled_sm("Allow", kit_btn::primary)
                    .on_press(Msg::PermissionAllowFirst),
            )
            .push(
                kit_btn::labeled_sm("Deny", kit_btn::danger).on_press(Msg::PermissionDeny),
            );
    }

    let body = column![
        kit_text::subheading("Permission required"),
        kit_text::body(pending.tool.clone()),
        text(truncate(&pending.preview, 360))
            .font(fonts::mono())
            .size(11)
            .style(kit_text::muted)
            .wrapping(iced::widget::text::Wrapping::Word),
        actions,
    ]
    .spacing(SPACE_SM);

    container(
        container(body.padding(Padding::from([SPACE_MD, SPACE_LG])))
            .width(Length::Fill)
            .max_width(720.0)
            .style(approval_card_style),
    )
    .width(Length::Fill)
    .center_x(Length::Fill)
    .padding(Padding::from([SPACE_SM, SPACE_XL]))
    .into()
}

fn approval_card_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let warn = p.warning.weak.color;
    container::Style {
        background: Some(Background::Color(warn)),
        border: Border {
            color: p.warning.base.color,
            width: 1.0,
            radius: RADIUS_LG.into(),
        },
        ..container::Style::default()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}
