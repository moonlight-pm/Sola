//! Field showcase — stacked label + input in a product panel.

use iced::widget::{column, container, text};
use iced::{Border, Color, Element, Length};

use sola_kit::components::field;
use sola_kit::components::style::{bevel_frame, stage_fill, RADIUS_XL};
use sola_kit::components::text::{body, caption, heading, muted};
use sola_kit::components::text_input as kit_input;
use sola_kit::fonts;

#[derive(Clone, Debug)]
pub enum Msg {
    Username(String),
    Email(String),
    Broken(String),
}

#[derive(Default)]
pub struct State {
    pub username: String,
    pub email: String,
    pub broken: String,
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Username(v) => self.username = v,
            Msg::Email(v) => self.email = v,
            Msg::Broken(v) => self.broken = v,
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let form = container(
        column![
            text("Account")
                .font(fonts::display())
                .size(16),
            body("Stacked label + control. Error replaces help in the same slot.")
                .style(muted),
            field(
                "Username",
                kit_input::text_input("naturalethic", &state.username)
                    .on_input(Msg::Username)
                    .style(kit_input::style),
                Some("3–20 characters"),
                None,
            ),
            field(
                "Email",
                kit_input::text_input("joshua@sola.computer", &state.email)
                    .on_input(Msg::Email)
                    .style(kit_input::style),
                None,
                None,
            ),
            field(
                "Display name",
                kit_input::text_input("must-not-be-empty", &state.broken)
                    .on_input(Msg::Broken)
                    .style(kit_input::style),
                Some("Shown in the menubar"),
                Some("This field is required"),
            ),
            caption("Horizontal settings rows live on Form (form_row).").style(muted),
        ]
        .spacing(14),
    )
    .padding(18)
    .width(Length::Fill)
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        iced::widget::container::Style {
            background: Some(stage_fill(
                p.background.base.color,
                p.background.weaker.color,
                p.primary.base.color,
            )),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_XL.into(),
            },
            ..Default::default()
        }
    });

    column![
        heading("Field"),
        body("The stacked form used in dialogs and account panels. Not a catalog of inputs.")
            .style(muted),
        container(form)
            .padding(1)
            .width(Length::Fill)
            .style(|theme: &iced::Theme| {
                bevel_frame(theme.extended_palette().background.weaker.color, RADIUS_XL)
            }),
    ]
    .spacing(16)
    .into()
}
