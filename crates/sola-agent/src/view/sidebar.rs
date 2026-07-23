//! Session sidebar: list Grok sessions for cwd, new, pin, select.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let busy = app.streaming || app.pending.is_some();

    let mut new_btn = kit_btn::labeled("New", kit_btn::secondary);
    if !busy {
        new_btn = new_btn.on_press(Msg::NewSession);
    }
    let header = row![kit_text::subheading("Agent"), new_btn]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center);

    let mut list = column![header]
        .spacing(SPACE_SM)
        .padding(Padding::new(SPACE_LG));

    for summary in &app.sessions {
        let selected = app.session_id.as_deref() == Some(summary.id.as_str());
        let pin = if summary.pinned { "★ " } else { "" };
        let label = format!("{pin}{}", summary.title);
        let mut item = button(text(label).size(13))
            .width(Length::Fill)
            .style(kit_btn::list_item(selected));
        if !busy {
            item = item.on_press(Msg::SelectSession(summary.id.clone()));
        }
        let pin_btn = {
            let mut b = kit_btn::labeled(
                if summary.pinned { "Unpin" } else { "Pin" },
                kit_btn::ghost,
            );
            b = b.on_press(Msg::TogglePin(summary.id.clone()));
            b
        };
        list = list.push(
            row![item, pin_btn]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center),
        );
    }

    container(scrollable(list))
        .width(Length::Fixed(280.0))
        .height(Length::Fill)
        .into()
}
