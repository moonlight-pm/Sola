//! Session sidebar: list persisted sessions, start a new one, or select an
//! existing one to resume. Borders/fills only — this iced stack does not
//! blur shadows.
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    // Session switching is unsafe while a turn is streaming (the worker would
    // append into the wrong session), so the controls go visibly disabled by
    // withholding their `on_press` — mirrored by the gate in `App::update`.
    let streaming = app.streaming.is_some();

    let mut new_btn =
        button(text("New")).style(sola_kit::components::button::secondary);
    if !streaming {
        new_btn = new_btn.on_press(Msg::NewSession);
    }
    let header = row![
        text("Agent").font(sola_kit::fonts::ui_medium()).size(18),
        new_btn,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let active_id = app.session.lock().ok().map(|s| s.id.clone());

    let mut list = column![header].spacing(6).padding(Padding::new(12.0));
    for summary in &app.sessions {
        let selected = active_id.as_deref() == Some(summary.id.as_str());
        let mut item = button(text(summary.title.as_str()).size(13))
            .width(Length::Fill)
            .style(sola_kit::components::button::list_item(selected));
        if !streaming {
            item = item.on_press(Msg::SelectSession(summary.path.clone()));
        }
        list = list.push(item);
    }

    container(scrollable(list))
        .width(Length::Fixed(260.0))
        .height(Length::Fill)
        .into()
}
