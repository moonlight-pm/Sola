//! Agent UI composition. Borders/fills only — this iced stack does not blur
//! shadows.
pub(crate) mod bubble;
pub(crate) mod footer;

use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::Element;
use iced::{Length, Padding};

use crate::{App, Msg};

pub(crate) fn screen(app: &App) -> Element<'_, Msg> {
    let bubbles: Vec<Element<'_, Msg>> = app
        .turns
        .iter()
        .map(|t| bubble::turn_view(t, &app.theme))
        .collect();
    let transcript = scrollable(
        Column::with_children(bubbles)
            .spacing(12)
            .padding(Padding::new(20.0))
            .width(Length::Fill),
    )
    .height(Length::Fill);

    column![transcript, input_row(app), footer::view(app)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn input_row(app: &App) -> Element<'_, Msg> {
    let field = text_input("Ask Sola Agent…", &app.draft)
        .on_input(Msg::DraftChanged)
        .on_submit(Msg::Send)
        .padding(12)
        .size(15)
        .width(Length::Fill);

    let action: Element<'_, Msg> = if app.streaming.is_some() {
        button(text("Stop"))
            .style(sola_kit::components::button::danger)
            .on_press(Msg::Abort)
            .into()
    } else {
        button(text("Send"))
            .style(sola_kit::components::button::primary)
            .on_press(Msg::Send)
            .into()
    };

    container(row![field, action].spacing(8))
        .padding(Padding::new(16.0))
        .width(Length::Fill)
        .into()
}
