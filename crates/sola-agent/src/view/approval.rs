//! Permission approval strip — warning card (graphite agent DS).

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{RADIUS_LG, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::{Msg, PendingApproval};

pub(crate) fn strip(pending: &PendingApproval) -> Element<'_, Msg> {
    let mut actions = row![].spacing(SPACE_SM).align_y(Alignment::Center);
    let mut has_deny = false;
    for opt in &pending.options {
        let kind = opt.kind.to_lowercase();
        let is_deny = kind.contains("reject") || kind.contains("deny");
        let style = if is_deny {
            has_deny = true;
            kit_btn::danger_outline
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
                kit_btn::labeled_sm("Approve", kit_btn::primary)
                    .on_press(Msg::PermissionAllowFirst),
            )
            .push(
                kit_btn::labeled_sm("Deny", kit_btn::danger_soft).on_press(Msg::PermissionDeny),
            );
        has_deny = true;
    }
    if !has_deny {
        // keep spacer + deny affordance consistent with design
    }
    actions = actions.push(Space::new().width(Length::Fill));

    let icon = container(text("!").font(fonts::ui_medium()).size(14).style(|t: &Theme| {
        iced::widget::text::Style {
            color: Some(t.extended_palette().warning.base.color),
        }
    }))
    .width(Length::Fixed(28.0))
    .height(Length::Fixed(28.0))
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(icon_style);

    let head = row![
        icon,
        column![
            text(format!("Allow {}?", pending.tool))
                .font(fonts::ui_medium())
                .size(14)
                .style(|t: &Theme| {
                    let w = t.extended_palette().warning.base.color;
                    iced::widget::text::Style {
                        color: Some(Color {
                            r: (w.r * 0.8 + 1.0 * 0.2).min(1.0),
                            g: (w.g * 0.8 + 1.0 * 0.2).min(1.0),
                            b: (w.b * 0.8 + 1.0 * 0.2).min(1.0),
                            a: 1.0,
                        }),
                    }
                }),
            text(truncate(&pending.preview, 360))
                .font(fonts::mono())
                .size(12)
                .style(kit_text::muted)
                .wrapping(iced::widget::text::Wrapping::Word),
        ]
        .spacing(3.0)
        .width(Length::Fill),
    ]
    .spacing(10.0)
    .align_y(Alignment::Start);

    let body = column![head, actions].spacing(10.0);

    container(
        container(body.padding(Padding::from([12.0, 14.0])))
            .width(Length::Fill)
            .max_width(720.0)
            .style(approval_card_style),
    )
    .width(Length::Fill)
    .center_x(Length::Fill)
    .padding(Padding {
        top: 0.0,
        right: SPACE_XL + 4.0,
        bottom: 10.0,
        left: SPACE_XL + 4.0,
    })
    .into()
}

fn icon_style(theme: &Theme) -> container::Style {
    let w = theme.extended_palette().warning.base.color;
    container::Style {
        background: Some(Background::Color(Color { a: 0.14, ..w })),
        border: Border {
            color: Color { a: 0.28, ..w },
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

fn approval_card_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let warn = p.warning.base.color;
    let raised = p.background.weaker.color;
    let bg = Color {
        r: warn.r * 0.08 + raised.r * 0.92,
        g: warn.g * 0.08 + raised.g * 0.92,
        b: warn.b * 0.08 + raised.b * 0.92,
        a: 1.0,
    };
    container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: Color {
                a: 0.35,
                r: warn.r * 0.35 + p.background.stronger.color.r * 0.65,
                g: warn.g * 0.35 + p.background.stronger.color.g * 0.65,
                b: warn.b * 0.35 + p.background.stronger.color.b * 0.65,
            },
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
