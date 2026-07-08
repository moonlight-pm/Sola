//! Session sidebar: list persisted sessions, start a new one, or select an
//! existing one to resume. Borders/fills only — this iced stack does not
//! blur shadows.
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let header = row![
        text("Agent").font(sola_kit::fonts::ui_medium()).size(18),
        button(text("New"))
            .style(sola_kit::components::button::secondary)
            .on_press(Msg::NewSession),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let active_id = app.session.lock().ok().map(|s| s.id.clone());

    let mut list = column![header].spacing(6).padding(Padding::new(12.0));
    for summary in &app.sessions {
        let selected = active_id.as_deref() == Some(summary.id.as_str());
        list = list.push(
            button(text(summary.title.as_str()).size(13))
                .width(Length::Fill)
                .style(sola_kit::components::button::list_item(selected))
                .on_press(Msg::SelectSession(summary.path.clone())),
        );
    }

    container(scrollable(list))
        .width(Length::Fixed(260.0))
        .height(Length::Fill)
        .into()
}
