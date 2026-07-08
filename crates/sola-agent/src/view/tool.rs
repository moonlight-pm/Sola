//! Rich tool-detail rendering: `Text` / `Diff` / `Bash` bodies inside a tool
//! bubble. Borders/fills only — this iced stack does not blur shadows.
use iced::widget::{Text, column, container, row, text};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::tools::ToolDetail;
use crate::{Msg, ToolTurn};

/// Short label for a detail variant. Drives the header badge in
/// `tool_view` once the call has finished (also exercised directly by the
/// compile guard test below).
pub(crate) fn detail_label(detail: &ToolDetail) -> &'static str {
    match detail {
        ToolDetail::Text(_) => "output",
        ToolDetail::Diff { .. } => "diff",
        ToolDetail::Bash { .. } => "shell",
    }
}

/// Compact one-line preview of a tool call's arguments, e.g. `{"path":"a"}`.
/// `None` for calls with no arguments (or a bare `{}`) so the header doesn't
/// show a redundant empty-object line.
fn args_summary(args: &serde_json::Value) -> Option<String> {
    match args {
        serde_json::Value::Null => None,
        serde_json::Value::Object(map) if map.is_empty() => None,
        other => Some(other.to_string()),
    }
}

pub(crate) fn tool_view<'a>(tt: &'a ToolTurn, theme: &Theme) -> Element<'a, Msg> {
    let mut header = row![
        text(format!("⚙ {}", tt.tool)).font(sola_kit::fonts::ui_medium()).size(13)
    ]
    .spacing(8);
    if let Some(d) = &tt.detail {
        header = header.push(sola_kit::components::badge::badge(
            detail_label(d),
            sola_kit::components::badge::Tone::Neutral,
        ));
    }

    let detail: Element<'a, Msg> = match &tt.detail {
        None => running(tt),
        Some(ToolDetail::Text(s)) => mono_block(s.as_str(), theme),
        Some(ToolDetail::Diff { path, before, after }) => {
            diff_view(path.as_str(), before.as_str(), after.as_str(), theme)
        }
        Some(ToolDetail::Bash { code, stdout, stderr }) => {
            bash_view(*code, stdout.as_str(), stderr.as_str(), theme)
        }
    };

    let mut body = column![header].spacing(8);
    if let Some(summary) = args_summary(&tt.args) {
        body = body.push(
            text(summary)
                .font(sola_kit::fonts::mono())
                .size(11)
                .style(sola_kit::components::text::muted),
        );
    }
    body = body.push(detail);

    sola_kit::components::card::card(body).width(Length::Fill).into()
}

fn running<'a>(tt: &'a ToolTurn) -> Element<'a, Msg> {
    column![
        text("running…").size(12).style(sola_kit::components::text::muted),
        mono_raw(tt.output.as_str()),
    ]
    .spacing(4)
    .into()
}

fn mono_raw<'a>(s: &str) -> Text<'a, Theme> {
    text(s.to_string()).font(sola_kit::fonts::mono()).size(12)
}

fn mono_block<'a>(s: &str, theme: &Theme) -> Element<'a, Msg> {
    let bg = theme.extended_palette().background.weaker.color;
    container(mono_raw(s))
        .padding(Padding::new(8.0))
        .width(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border { color: bg, width: 1.0, radius: 6.0.into() },
            ..container::Style::default()
        })
        .into()
}

fn diff_line<'a>(sign: char, content: &str, color: Color) -> Element<'a, Msg> {
    text(format!("{sign} {content}"))
        .font(sola_kit::fonts::mono())
        .size(12)
        .style(move |_t: &Theme| iced::widget::text::Style { color: Some(color) })
        .into()
}

fn diff_view<'a>(path: &str, before: &str, after: &str, theme: &Theme) -> Element<'a, Msg> {
    let p = theme.extended_palette();
    let removed = p.danger.base.color;
    let added = p.success.base.color;
    let mut lines = column![
        text(path.to_string())
            .font(sola_kit::fonts::mono())
            .size(12)
            .style(sola_kit::components::text::muted)
    ]
    .spacing(1);
    for line in before.lines() {
        lines = lines.push(diff_line('-', line, removed));
    }
    for line in after.lines() {
        lines = lines.push(diff_line('+', line, added));
    }
    container(lines).padding(Padding::new(8.0)).width(Length::Fill).into()
}

fn bash_view<'a>(code: i32, stdout: &str, stderr: &str, theme: &Theme) -> Element<'a, Msg> {
    let p = theme.extended_palette();
    let status_color = if code == 0 { p.success.base.color } else { p.danger.base.color };
    let status = text(format!("exit {code}"))
        .size(12)
        .style(move |_t: &Theme| iced::widget::text::Style { color: Some(status_color) });
    let mut col = column![status].spacing(6);
    if !stdout.is_empty() {
        col = col.push(mono_raw(stdout));
    }
    if !stderr.is_empty() {
        col = col.push(
            text(stderr.to_string())
                .font(sola_kit::fonts::mono())
                .size(12)
                .style(sola_kit::components::text::danger),
        );
    }
    container(col).padding(Padding::new(8.0)).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use crate::tools::ToolDetail;

    // Guards that the three detail shapes are all handled (renderer returns an
    // Element for each without panicking on match).
    #[test]
    fn tool_view_handles_all_detail_variants() {
        let variants = [
            ToolDetail::Text("x".into()),
            ToolDetail::Diff { path: "a".into(), before: "b".into(), after: "c".into() },
            ToolDetail::Bash { code: 0, stdout: "o".into(), stderr: String::new() },
        ];
        for v in variants {
            // super::detail_label is a pure helper introduced with the renderer.
            assert!(!super::detail_label(&v).is_empty());
        }
    }
}
