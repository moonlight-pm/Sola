//! Status bar — mono pills + context meter (graphite agent DS).

use iced::widget::{container, row, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::style::{RADIUS_PILL, SPACE_MD, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::App;

pub(crate) fn view(app: &App) -> Element<'_, crate::Msg> {
    let model = pill(format!(
        "model  {}",
        if app.backend_label.is_empty() {
            "grok".into()
        } else {
            app.backend_label.to_ascii_lowercase()
        }
    ));
    let mode = pill(format!("{}  {}", "mode", app.connection_mode.as_str()));

    let turn = if app.pending.is_some() {
        pill("awaiting approval".into())
    } else if app.streaming {
        pill("streaming".into())
    } else if app.connected {
        pill("idle".into())
    } else {
        pill("disconnected".into())
    };

    let mut left = row![model, mode, sep(), turn]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center);

    left = left.push(Space::new().width(Length::Fill));

    if let Some(ctx) = format_usage_parts(app.usage_used, app.usage_size) {
        left = left.push(context_meter(&ctx));
    }

    container(left.padding(Padding::from([7.0, SPACE_XL + 4.0])))
        .width(Length::Fill)
        .style(footer_style)
        .into()
}

struct UsageParts {
    pct: u64,
    used_k: u64,
    size_k: u64,
    frac: f32,
}

fn format_usage_parts(used: Option<u64>, size: Option<u64>) -> Option<UsageParts> {
    let used = used?;
    let size = size.unwrap_or(500_000).max(1);
    let frac = (used as f32 / size as f32).clamp(0.0, 1.0);
    let pct = (frac * 100.0).round() as u64;
    Some(UsageParts {
        pct,
        used_k: tokens_k(used),
        size_k: tokens_k(size),
        frac,
    })
}

fn context_meter(u: &UsageParts) -> Element<'static, crate::Msg> {
    let bar_w = 72.0;
    let fill_w = (bar_w * u.frac).max(2.0);
    let track = container(
        container(Space::new().width(Length::Fixed(fill_w)).height(Length::Fixed(4.0)))
            .width(Length::Fixed(fill_w))
            .height(Length::Fixed(4.0))
            .style(ctx_fill_style),
    )
    .width(Length::Fixed(bar_w))
    .height(Length::Fixed(4.0))
    .style(ctx_track_style);

    row![
        text(format!("context {}%", u.pct))
            .font(fonts::mono())
            .size(11)
            .style(kit_text::muted),
        track,
        text(format!("{}k / {}k", u.used_k, u.size_k))
            .font(fonts::mono())
            .size(11)
            .style(kit_text::muted),
    ]
    .spacing(8.0)
    .align_y(Alignment::Center)
    .into()
}

fn pill(label: String) -> Element<'static, crate::Msg> {
    container(
        text(label)
            .font(fonts::mono())
            .size(11)
            .style(kit_text::muted),
    )
    .padding(Padding::from([2.0, 8.0]))
    .style(pill_style)
    .into()
}

fn sep() -> Element<'static, crate::Msg> {
    container(Space::new().width(1.0).height(12.0))
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            container::Style {
                background: Some(Background::Color(Color {
                    a: 0.7,
                    ..p.background.stronger.color
                })),
                ..container::Style::default()
            }
        })
        .into()
}

fn tokens_k(n: u64) -> u64 {
    (n + 500) / 1000
}

fn footer_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        border: Border {
            color: Color {
                a: 0.45,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn pill_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.80,
            ..p.background.weaker.color
        })),
        border: Border {
            color: Color {
                a: 0.55,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: RADIUS_PILL.into(),
        },
        ..container::Style::default()
    }
}

fn ctx_track_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.90,
            ..p.background.strong.color
        })),
        border: Border {
            radius: RADIUS_PILL.into(),
            ..Default::default()
        },
        ..container::Style::default()
    }
}

fn ctx_fill_style(theme: &Theme) -> container::Style {
    let c = theme.extended_palette().primary.base.color;
    container::Style {
        background: Some(Background::Color(c)),
        border: Border {
            radius: RADIUS_PILL.into(),
            ..Default::default()
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_parts() {
        let u = format_usage_parts(Some(258_000), Some(500_000)).unwrap();
        assert_eq!(u.pct, 52);
        assert_eq!(u.used_k, 258);
        assert_eq!(u.size_k, 500);
    }
}
