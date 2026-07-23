//! Permission approval strip.

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;

use crate::{Msg, PendingApproval};

pub(crate) fn strip<'a>(
    pending: &'a PendingApproval,
    _theme: &iced::Theme,
) -> Element<'a, Msg> {
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
            kit_btn::labeled(opt.name.as_str(), style)
                .on_press(Msg::PermissionPick(opt.option_id.clone())),
        );
    }
    if pending.options.is_empty() {
        actions = actions
            .push(kit_btn::labeled("Allow", kit_btn::primary).on_press(Msg::PermissionAllowFirst))
            .push(
                kit_btn::labeled("Deny", kit_btn::danger).on_press(Msg::PermissionDeny),
            );
    }

    container(
        column![
            kit_text::subheading("Permission required"),
            text(format!("{} — {}", pending.tool, truncate(&pending.preview, 300))).size(12),
            actions,
        ]
        .spacing(SPACE_SM)
        .padding(Padding::from([SPACE_MD, SPACE_XL])),
    )
    .width(Length::Fill)
    .into()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}
