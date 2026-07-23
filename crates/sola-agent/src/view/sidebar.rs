//! Session sidebar: list persisted sessions, start a new one, or select an
//! existing one to resume. Borders/fills only — this iced stack does not
//! blur shadows.
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    // Session switching is unsafe while a turn is streaming (the worker would
    // append into the wrong session), so the controls go visibly disabled by
    // withholding their `on_press` — mirrored by the gate in `App::update`.
    let streaming = app.streaming.is_some();

    let mut new_btn = kit_btn::labeled("New", kit_btn::secondary);
    if !streaming {
        new_btn = new_btn.on_press(Msg::NewSession);
    }
    let header = row![kit_text::subheading("Agent"), new_btn,]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center);

    let active_id = app.session.lock().ok().map(|s| s.id.clone());

    let mut list = column![header]
        .spacing(SPACE_SM)
        .padding(Padding::new(SPACE_LG));
    for summary in &app.sessions {
        let selected = active_id.as_deref() == Some(summary.id.as_str());
        let mut item = button(text(summary.title.as_str()).size(13))
            .width(Length::Fill)
            .style(kit_btn::list_item(selected));
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
