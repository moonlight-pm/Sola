//! sola-agent — minimal iced shell for a focused GUI AI agent.
//!
//! This is a fresh app scaffold. It deliberately ignores the retired
//! `apps/agent` WebView/Claude-CLI prototype and follows the current
//! `sola-kit`/iced app pattern used by `sola-terminal`, `sola-settings`,
//! and `sola-monitor`.

use std::sync::Arc;

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Padding, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::{MenuActionPayload, Topic, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, bus_subscription, startup, window_settings};
use sola_kit::fonts;
use sola_kit::theme::{default_theme, theme_from_bus};

const APP_ID: &str = "sola-agent";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(&[TopicKind::Theme, TopicKind::MenuAction, TopicKind::CloseApp])
        .app_menu("Agent", [("quit", "Quit Agent", KeyCode::Q.meta())])
        .install();

    let app = iced::application(App::default, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings(APP_ID));
    app.run()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    role: Role,
    text: String,
}

#[derive(Debug, Clone)]
struct Conversation {
    id: u64,
    title: String,
    messages: Vec<ChatMessage>,
}

struct App {
    theme: Theme,
    conversations: Vec<Conversation>,
    active: Option<u64>,
    draft: String,
    next_id: u64,
}

impl Default for App {
    fn default() -> Self {
        let welcome = Conversation {
            id: 1,
            title: "New conversation".into(),
            messages: vec![ChatMessage {
                role: Role::Assistant,
                text: "What would you like to work on?".into(),
            }],
        };
        Self {
            theme: default_theme(),
            conversations: vec![welcome],
            active: Some(1),
            draft: String::new(),
            next_id: 2,
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    BusMessage(Arc<Message>),
    NewConversation,
    SelectConversation(u64),
    DraftChanged(String),
    SendDraft,
}

impl App {
    fn title(&self) -> String {
        "Sola Agent".into()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Msg> {
        bus_subscription().map(Msg::BusMessage)
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::BusMessage(message) => {
                let parsed = Topic::parse(&message);

                if let Some(Topic::Theme(bus_theme)) = &parsed {
                    self.theme = theme_from_bus(bus_theme);
                    sola_kit::fonts::install(sola_kit::theme::fonts_from_bus_theme(bus_theme));
                }

                let our_quit = matches!(
                    &parsed,
                    Some(Topic::MenuAction(MenuActionPayload { app_id, action_id }))
                        if app_id == APP_ID && action_id == "quit"
                );
                let close_us = matches!(
                    &parsed,
                    Some(Topic::CloseApp(app_id)) if app_id == APP_ID
                );
                if our_quit || close_us {
                    return iced::exit();
                }
            }
            Msg::NewConversation => {
                let id = self.next_id;
                self.next_id += 1;
                self.conversations.insert(
                    0,
                    Conversation {
                        id,
                        title: "New conversation".into(),
                        messages: vec![ChatMessage {
                            role: Role::Assistant,
                            text: "Start with a goal, question, or task.".into(),
                        }],
                    },
                );
                self.active = Some(id);
                self.draft.clear();
            }
            Msg::SelectConversation(id) => {
                self.active = Some(id);
                self.draft.clear();
            }
            Msg::DraftChanged(value) => {
                self.draft = value;
            }
            Msg::SendDraft => {
                let text = self.draft.trim().to_string();
                if text.is_empty() {
                    return Task::none();
                }
                if let Some(conversation) = self.active_conversation_mut() {
                    if conversation.title == "New conversation" {
                        conversation.title = summarize_title(&text);
                    }
                    conversation.messages.push(ChatMessage {
                        role: Role::User,
                        text,
                    });
                    conversation.messages.push(ChatMessage {
                        role: Role::Assistant,
                        text: "Agent backend is not wired yet.".into(),
                    });
                }
                self.draft.clear();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        row![self.sidebar(), self.chat()]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn sidebar(&self) -> Element<'_, Msg> {
        let header = row![
            text("Agent").font(fonts::ui_medium()).size(18),
            button(text("+").size(18)).on_press(Msg::NewConversation),
        ]
        .align_y(Alignment::Center)
        .spacing(12);

        let mut list = column![header, Space::new().height(Length::Fixed(1.0))].spacing(10);
        for conversation in &self.conversations {
            let selected = self.active == Some(conversation.id);
            let label = if selected {
                format!("› {}", conversation.title)
            } else {
                conversation.title.clone()
            };
            list = list.push(
                button(text(label).size(14))
                    .width(Length::Fill)
                    .on_press(Msg::SelectConversation(conversation.id)),
            );
        }

        container(scrollable(list.padding(Padding::new(12.0))))
            .width(Length::Fixed(280.0))
            .height(Length::Fill)
            .into()
    }

    fn chat(&self) -> Element<'_, Msg> {
        let active = self.active_conversation();

        let title = active
            .map(|conversation| conversation.title.as_str())
            .unwrap_or("No conversation");

        let mut messages = column![].spacing(12).padding(Padding::new(20.0));
        if let Some(conversation) = active {
            for message in &conversation.messages {
                messages = messages.push(message_bubble(message));
            }
        } else {
            messages = messages.push(text("Create a conversation to begin."));
        }

        let input = row![
            text_input("Ask Sola Agent…", &self.draft)
                .on_input(Msg::DraftChanged)
                .on_submit(Msg::SendDraft)
                .padding(12)
                .size(15)
                .width(Length::Fill),
            button(text("Send")).on_press(Msg::SendDraft),
        ]
        .spacing(8)
        .padding(Padding::new(16.0));

        column![
            container(text(title).font(fonts::ui_medium()).size(20))
                .width(Length::Fill)
                .padding(Padding::new(16.0)),
            Space::new().height(Length::Fixed(1.0)),
            scrollable(messages).height(Length::Fill),
            Space::new().height(Length::Fixed(1.0)),
            input,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn active_conversation(&self) -> Option<&Conversation> {
        let active = self.active?;
        self.conversations.iter().find(|c| c.id == active)
    }

    fn active_conversation_mut(&mut self) -> Option<&mut Conversation> {
        let active = self.active?;
        self.conversations.iter_mut().find(|c| c.id == active)
    }
}

fn message_bubble(message: &ChatMessage) -> Element<'_, Msg> {
    let label = match message.role {
        Role::User => "You",
        Role::Assistant => "Agent",
    };
    let align = match message.role {
        Role::User => Alignment::End,
        Role::Assistant => Alignment::Start,
    };

    container(
        column![
            text(label).font(fonts::ui_medium()).size(12),
            text(&message.text).size(15),
        ]
        .spacing(4)
        .padding(Padding::new(12.0)),
    )
    .width(Length::Fill)
    .align_x(align)
    .into()
}

fn summarize_title(text: &str) -> String {
    const MAX_CHARS: usize = 40;
    let mut title: String = text.chars().take(MAX_CHARS).collect();
    if text.chars().count() > MAX_CHARS {
        title.push('…');
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_summarizer_keeps_short_text() {
        assert_eq!(summarize_title("hello"), "hello");
    }

    #[test]
    fn title_summarizer_truncates_long_text() {
        let title = summarize_title("abcdefghijklmnopqrstuvwxyz0123456789----tail");
        assert_eq!(title, "abcdefghijklmnopqrstuvwxyz0123456789----…");
    }
}
