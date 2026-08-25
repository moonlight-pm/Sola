//! Permission approval strip — graphite product moment (sola-agent-ds).
//!
//! Compact warning card: kind caption, tool title, mono preview, action row.
//! Prefer short labels and clear hierarchy over a wall of raw JSON.

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{RADIUS_LG, RADIUS_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::{Msg, PendingApproval};

pub(crate) fn strip(pending: &PendingApproval) -> Element<'_, Msg> {
    let actions = action_row(pending);

    let icon = container(
        text("!")
            .font(fonts::ui_medium())
            .size(13)
            .style(|t: &Theme| iced::widget::text::Style {
                color: Some(t.extended_palette().warning.base.color),
            }),
    )
    .width(Length::Fixed(28.0))
    .height(Length::Fixed(28.0))
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(icon_style);

    let title = text(format!("Allow {}?", display_tool(&pending.tool)))
        .font(fonts::ui_medium())
        .size(14)
        .style(title_style);

    let preview = format_preview(&pending.preview);
    let preview_el: Element<'_, Msg> = if preview.is_empty() {
        Space::new().height(0).into()
    } else {
        container(
            text(preview)
                .font(fonts::mono())
                .size(12)
                .style(kit_text::muted)
                .wrapping(iced::widget::text::Wrapping::Word)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .padding(Padding::from([8.0, 10.0]))
        .style(preview_well_style)
        .into()
    };

    let head = row![
        icon,
        column![
            text("Permission required")
                .font(fonts::ui_medium())
                .size(10)
                .style(caption_style),
            title,
        ]
        .spacing(3.0)
        .width(Length::Fill),
    ]
    .spacing(10.0)
    .align_y(Alignment::Start);

    let body = column![head, preview_el, actions]
        .spacing(10.0)
        .width(Length::Fill);

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

fn action_row(pending: &PendingApproval) -> Element<'_, Msg> {
    let mut actions = row![].spacing(SPACE_SM).align_y(Alignment::Center);

    if pending.options.is_empty() {
        actions = actions
            .push(
                kit_btn::labeled_sm("Allow", kit_btn::primary).on_press(Msg::PermissionAllowFirst),
            )
            .push(
                kit_btn::labeled_sm("Deny", kit_btn::danger_outline).on_press(Msg::PermissionDeny),
            );
    } else {
        // Stable order: allows first (once → always), then rejects.
        let mut opts: Vec<_> = pending.options.iter().collect();
        opts.sort_by_key(|o| option_sort_key(&o.kind));

        for opt in opts {
            let kind = opt.kind.to_lowercase();
            let is_deny = kind.contains("reject") || kind.contains("deny");
            let is_always = kind.contains("allow_always")
                || (kind.contains("always") && kind.contains("allow"));
            let style = if is_deny {
                kit_btn::danger_outline
            } else if is_always {
                kit_btn::secondary
            } else {
                kit_btn::primary
            };
            let label = friendly_option_label(&opt.name, &opt.kind);
            actions = actions.push(
                kit_btn::labeled_sm(label, style)
                    .on_press(Msg::PermissionPick(opt.option_id.clone())),
            );
        }
    }

    actions.push(Space::new().width(Length::Fill)).into()
}

fn option_sort_key(kind: &str) -> u8 {
    let k = kind.to_lowercase();
    if k.contains("reject") || k.contains("deny") {
        3
    } else if k.contains("allow_always") || (k.contains("always") && k.contains("allow")) {
        2
    } else if k.contains("allow") {
        1
    } else {
        2
    }
}

fn friendly_option_label(name: &str, kind: &str) -> String {
    let k = kind.to_lowercase();
    let n = name.trim();
    // Prefer short, product-y labels when the wire name is noisy.
    if k.contains("allow_always") || n.eq_ignore_ascii_case("allow always") {
        return "Always allow".into();
    }
    if k.contains("allow_once")
        || n.eq_ignore_ascii_case("allow once")
        || n.eq_ignore_ascii_case("allow")
    {
        return "Allow".into();
    }
    if k.contains("reject_always") || n.eq_ignore_ascii_case("reject always") {
        return "Always deny".into();
    }
    if k.contains("reject") || k.contains("deny") {
        return "Deny".into();
    }
    if n.is_empty() {
        "Allow".into()
    } else {
        n.to_string()
    }
}

fn display_tool(tool: &str) -> String {
    let t = tool.trim();
    if t.is_empty() {
        return "tool".into();
    }
    // Strip noisy prefixes; keep the human-readable title.
    t.trim_start_matches("tool:")
        .trim_start_matches("Tool:")
        .to_string()
}

/// Pretty-print JSON when possible; otherwise soft-truncate plain text.
fn format_preview(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() || s == "null" {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        return pretty_json_preview(&v, 320);
    }
    // Raw might be a JSON string with quotes from serde to_string.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&format!("\"{s}\"")) {
        if let Some(inner) = v.as_str() {
            if let Ok(nested) = serde_json::from_str::<serde_json::Value>(inner) {
                return pretty_json_preview(&nested, 320);
            }
        }
    }
    truncate(s, 320)
}

fn pretty_json_preview(v: &serde_json::Value, max: usize) -> String {
    // Prefer a few key fields when present.
    if let Some(obj) = v.as_object() {
        let keys = [
            "command",
            "path",
            "file",
            "filePath",
            "filepath",
            "url",
            "query",
            "description",
            "content",
            "args",
        ];
        let mut lines = Vec::new();
        for k in keys {
            if let Some(val) = obj.get(k) {
                let rendered = match val {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                lines.push(format!("{k}: {}", truncate(&rendered, 120)));
            }
        }
        if !lines.is_empty() {
            return truncate(&lines.join("\n"), max);
        }
    }
    let pretty = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    truncate(&pretty, max)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn caption_style(theme: &Theme) -> iced::widget::text::Style {
    let m = theme.extended_palette().secondary.base.color;
    iced::widget::text::Style {
        color: Some(Color { a: 0.85, ..m }),
    }
}

fn title_style(theme: &Theme) -> iced::widget::text::Style {
    let w = theme.extended_palette().warning.base.color;
    iced::widget::text::Style {
        color: Some(Color {
            r: (w.r * 0.75 + 1.0 * 0.25).min(1.0),
            g: (w.g * 0.75 + 1.0 * 0.25).min(1.0),
            b: (w.b * 0.75 + 1.0 * 0.25).min(1.0),
            a: 1.0,
        }),
    }
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

fn preview_well_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.55,
            ..p.background.strong.color
        })),
        border: Border {
            color: Color {
                a: 0.35,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

fn approval_card_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let warn = p.warning.base.color;
    let raised = p.background.weaker.color;
    let bg = Color {
        r: warn.r * 0.07 + raised.r * 0.93,
        g: warn.g * 0.07 + raised.g * 0.93,
        b: warn.b * 0.07 + raised.b * 0.93,
        a: 1.0,
    };
    container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: Color {
                a: 0.40,
                r: warn.r * 0.40 + p.background.stronger.color.r * 0.60,
                g: warn.g * 0.40 + p.background.stronger.color.g * 0.60,
                b: warn.b * 0.40 + p.background.stronger.color.b * 0.60,
            },
            width: 1.0,
            radius: RADIUS_LG.into(),
        },
        ..container::Style::default()
    }
}
