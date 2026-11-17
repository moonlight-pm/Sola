//! The conversations, newest first, with a field that filters them and
//! finds people to write to.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::badge::count_mark;
use sola_kit::components::scroll;
use sola_kit::components::style::{BAR_H, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use super::{App, Msg, search_id, style, when};
use crate::store::chats::{Chat, Id, Recipient};

const LIST_W: f32 = 300.0;
const AVATAR: f32 = 38.0;

pub fn v_hairline() -> Element<'static, Msg> {
    container(Space::new().width(1).height(Length::Fill)).width(1).height(Length::Fill).style(style::hairline).into()
}

pub fn h_hairline() -> Element<'static, Msg> {
    container(Space::new().width(Length::Fill).height(1)).width(Length::Fill).height(1).style(style::hairline).into()
}

/// A disc with someone's initial, in a colour of their own.
pub fn avatar<'a>(seed: Id, name: &str, size: f32) -> Element<'a, Msg> {
    let initial: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "#".into());
    container(text(initial).size(size * 0.45).font(fonts::ui_medium()))
        .width(size)
        .height(size)
        .center_x(size)
        .center_y(size)
        .style(style::avatar(seed))
        .into()
}

impl App {
    pub(super) fn view_list(&self) -> Element<'_, Msg> {
        let field = text_input("Search or start a conversation", &self.search)
            .id(search_id())
            .on_input(Msg::Search)
            .size(13)
            .padding(Padding::from([5, 9]))
            .style(style::search);
        let bar = container(field).height(BAR_H).padding(Padding::from([0.0, SPACE_MD])).center_y(BAR_H);

        let query = self.search.trim().to_lowercase();
        let mut rows = column![];
        let shown: Vec<&Chat> = self.chats.iter().filter(|chat| query.is_empty() || chat.title.to_lowercase().contains(&query)).collect();
        for chat in &shown {
            rows = rows.push(self.chat_row(chat));
        }
        if !query.is_empty() {
            let people: Vec<&Recipient> = self
                .people
                .iter()
                .filter(|person| {
                    person.id != self.me
                        && !person.blocked
                        && (person.contact_name.is_some() || person.profile_name.is_some())
                        && person.name().to_lowercase().contains(&query)
                        && !shown.iter().any(|chat| chat.recipient == Some(person.id))
                })
                .take(30)
                .collect();
            if !people.is_empty() {
                rows = rows.push(
                    container(kit_text::caption("People").style(kit_text::muted))
                        .padding(Padding::from([SPACE_SM, SPACE_LG])),
                );
            }
            for person in people {
                rows = rows.push(person_row(person));
            }
        }
        if shown.is_empty() && query.is_empty() {
            rows = rows.push(
                container(
                    kit_text::caption("No conversations yet. Messages show here as they arrive; search for someone to write to them.")
                        .style(kit_text::muted),
                )
                .padding(SPACE_LG),
            );
        }

        container(column![bar, h_hairline(), scroll::scrollable(rows).height(Length::Fill)])
            .width(LIST_W)
            .height(Length::Fill)
            .style(style::raised)
            .into()
    }

    fn chat_row<'a>(&'a self, chat: &'a Chat) -> Element<'a, Msg> {
        let unread = chat.unread > 0;
        let title = text(chat.title.as_str())
            .size(14)
            .font(if unread { fonts::ui_medium() } else { fonts::ui() })
            .wrapping(text::Wrapping::None);
        let preview = if !chat.draft.is_empty() && self.open != Some(chat.id) {
            format!("Draft: {}", chat.draft.lines().next().unwrap_or_default())
        } else if chat.preview_outgoing && !chat.preview.is_empty() {
            format!("You: {}", chat.preview)
        } else {
            chat.preview.clone()
        };
        let mut second = row![
            container(kit_text::caption(preview).style(kit_text::muted).wrapping(text::Wrapping::None))
                .width(Length::Fill)
                .clip(true)
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
        if unread {
            second = second.push(count_mark(chat.unread));
        }
        let words = column![
            row![
                container(title).width(Length::Fill).clip(true),
                kit_text::caption(when::brief(chat.last_activity)).style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
            second,
        ]
        .spacing(SPACE_XS)
        .width(Length::Fill);
        button(row![avatar(chat.id, &chat.title, AVATAR), words].spacing(SPACE_MD).align_y(Alignment::Center))
            .on_press(Msg::Open(chat.id))
            .padding(Padding::from([SPACE_MD, SPACE_LG]))
            .width(Length::Fill)
            .style(style::list_row(self.open == Some(chat.id)))
            .into()
    }
}

fn person_row(person: &Recipient) -> Element<'_, Msg> {
    let name = person.name();
    let mut words = column![text(name.clone()).size(14)].spacing(SPACE_XS);
    if let Some(number) = person.number.as_deref().filter(|number| *number != name) {
        words = words.push(kit_text::caption(number).style(kit_text::muted));
    }
    button(row![avatar(person.id, &name, AVATAR), words].spacing(SPACE_MD).align_y(Alignment::Center))
        .on_press(Msg::OpenPerson(person.id))
        .padding(Padding::from([SPACE_MD, SPACE_LG]))
        .width(Length::Fill)
        .style(style::list_row(false))
        .into()
}
