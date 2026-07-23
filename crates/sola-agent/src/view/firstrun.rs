//! First-run key-entry screen. Shown instead of the normal transcript UI
//! while `App.first_run` is true (no Sakana API key on disk or in env).
//! Borders/fills only — this iced stack does not blur shadows.
use iced::widget::{column, container};
use iced::{Alignment, Element, Length};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let field = text_input("Sakana API key", &app.key_draft)
        .on_input(Msg::KeyDraftChanged)
        .on_submit(Msg::KeySubmit)
        .secure(true)
        .size(13)
        .style(text_input::style)
        .width(Length::Fixed(360.0));

    let body = column![
        kit_text::heading("Welcome to Sola Agent"),
        kit_text::body("Paste your Sakana API key to begin. It is encrypted at rest.")
            .style(kit_text::muted),
        field,
        kit_btn::labeled("Save key", kit_btn::primary).on_press(Msg::KeySubmit),
    ]
    .spacing(SPACE_LG)
    .align_x(Alignment::Center);

    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(SPACE_XL)
        .into()
}
