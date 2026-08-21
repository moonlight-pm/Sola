//! NumberInput — token steppers in a settings panel.

use iced::Element;
use iced::widget::column;

use sola_kit::components::form::form_row;
use sola_kit::components::number_input;
use sola_kit::components::style::SPACE_MD;

use crate::storybook::pages::chrome::{lede, panel};

#[derive(Clone, Debug)]
pub enum Msg {
    Radius(f32),
    Opacity(f32),
}

pub struct State {
    pub radius: f32,
    pub opacity: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            radius: 8.0,
            opacity: 80.0,
        }
    }
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Radius(v) => self.radius = v,
            Msg::Opacity(v) => self.opacity = v,
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    column![
        lede(
            "NumberInput",
            "Unit-aware steppers for tokens — radius, spacing, opacity.",
        ),
        panel(
            column![
                form_row(
                    "Corner radius",
                    number_input(state.radius, 0.0..=32.0, 1.0, "px", Msg::Radius),
                ),
                form_row(
                    "Opacity",
                    number_input(state.opacity, 0.0..=100.0, 5.0, "%", Msg::Opacity),
                ),
            ]
            .spacing(SPACE_MD),
        ),
    ]
    .spacing(16)
    .into()
}
