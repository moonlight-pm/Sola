//! First-run key-entry screen. Shown instead of the normal transcript UI
//! while `App.first_run` is true (no Sakana API key on disk or in env).
//! Borders/fills only — this iced stack does not blur shadows.
use iced::widget::{button, column, container, text, text_input};
use iced::{Alignment, Element, Length};

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let field = text_input("Sakana API key", &app.key_draft)
        .on_input(Msg::KeyDraftChanged)
        .on_submit(Msg::KeySubmit)
        .secure(true)
        .padding(12)
        .size(15)
        .width(Length::Fixed(360.0));

    let body = column![
        text("Welcome to Sola Agent").font(sola_kit::fonts::ui_medium()).size(20),
        text("Paste your Sakana API key to begin. It is encrypted at rest.")
            .size(13)
            .style(sola_kit::components::text::muted),
        field,
        button(text("Save key"))
            .style(sola_kit::components::button::primary)
            .on_press(Msg::KeySubmit),
    ]
    .spacing(14)
    .align_x(Alignment::Center);

    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
