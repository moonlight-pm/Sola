//! Status bar — quiet chrome with badges for connection and turn state.

use iced::widget::{container, row};
use iced::{Alignment, Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::badge::{self, Tone};
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;

use crate::App;

pub(crate) fn view(app: &App) -> Element<'_, crate::Msg> {
    let conn = if app.connected {
        badge::badge(format!("{} · {}", app.backend_label, app.connection_mode.as_str()), Tone::Success)
    } else {
        badge::badge("disconnected", Tone::Warning)
    };

    let turn = if app.pending.is_some() {
        badge::badge("awaiting approval", Tone::Warning)
    } else if app.streaming {
        badge::badge("streaming", Tone::Accent)
    } else {
        badge::badge("idle", Tone::Neutral)
    };

    let ctx = match (app.usage_used, app.usage_size) {
        (Some(used), Some(size)) if size > 0 => {
            let pct = (used as f64 / size as f64 * 100.0).clamp(0.0, 100.0).round() as u64;
            format!("{pct}% context")
        }
        (Some(used), _) => format!("~{used} tokens"),
        _ => String::new(),
    };

    let session = app
        .session_title
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            app.session_id.as_ref().map(|id| {
                if id.len() > 10 {
                    format!("{}…", &id[..8])
                } else {
                    id.clone()
                }
            })
        })
        .unwrap_or_else(|| "No session".into());

    let mut status = row![conn, turn, kit_text::caption(session).style(kit_text::muted)]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center);
    status = status.push(iced::widget::Space::new().width(Length::Fill));
    if !ctx.is_empty() {
        status = status.push(kit_text::caption(ctx).style(kit_text::muted));
    }

    container(
        status.padding(Padding::from([SPACE_SM + 2.0, SPACE_XL])),
    )
    .width(Length::Fill)
    .style(footer_style)
    .into()
}

fn footer_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: Border {
            color: p.background.stronger.color,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}
