//! Field showcase — labelled form rows with the kit's text_input.
//! Stateful so the input actually reacts.

use iced::widget::column;
use iced::Element;

use sola_kit::components::card;
use sola_kit::components::field;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::text_input as kit_input;

#[derive(Clone, Debug)]
pub enum Msg {
    Username(String),
    Email(String),
}

#[derive(Default)]
pub struct State {
    pub username: String,
    pub email: String,
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Username(v) => self.username = v,
            Msg::Email(v) => self.email = v,
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let demo = card(
        column![
            field(
                "Username",
                kit_input::text_input("alice", &state.username)
                    .on_input(Msg::Username)
                    .style(kit_input::style),
                Some("3–20 characters"),
            ),
            field(
                "Email",
                kit_input::text_input("alice@example.com", &state.email)
                    .on_input(Msg::Email)
                    .style(kit_input::style),
                None,
            ),
        ]
        .spacing(16),
    );

    column![
        heading("Field"),
        body("Label is body 13 muted; help is caption 11; input pad [4, 8].").style(muted),
        demo,
        code("field(\"Label\", text_input(...).style(text_input::style), Some(\"help\"))").style(muted),
    ]
    .spacing(16)
    .into()
}
