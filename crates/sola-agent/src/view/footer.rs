//! Status bar: backend, connection mode, context usage, turn state.

use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::style::{SPACE_MD, SPACE_XL};
use sola_kit::components::text as kit_text;

use crate::App;

pub(crate) fn view(app: &App) -> Element<'_, crate::Msg> {
    let backend = app.backend_label.as_str();
    let mode = app.connection_mode.as_str();
    let ctx = match (app.usage_used, app.usage_size) {
        (Some(used), Some(size)) if size > 0 => {
            let pct = (used as f64 / size as f64 * 100.0).round() as u64;
            format!("context {used}/{size} ({pct}%)")
        }
        (Some(used), _) => format!("tokens ~{used}"),
        _ => "context —".into(),
    };
    let state = if app.pending.is_some() {
        "waiting for approval"
    } else if app.streaming {
        "streaming"
    } else if app.connected {
        "idle"
    } else {
        "disconnected"
    };
    let session = app
        .session_id
        .as_deref()
        .map(|s| {
            if s.len() > 8 {
                format!("session {}…", &s[..8])
            } else {
                format!("session {s}")
            }
        })
        .unwrap_or_else(|| "no session".into());

    container(
        row![
            kit_text::caption(backend),
            kit_text::caption(mode),
            kit_text::caption(session),
            kit_text::caption(ctx),
            text(state).size(11),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center)
        .padding(Padding::from([SPACE_MD, SPACE_XL])),
    )
    .width(Length::Fill)
    .into()
}
