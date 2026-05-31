//! NumberInput showcase — unit-aware steppers. Stateful so the values
//! actually change as you press.

use iced::widget::column;
use iced::Element;

use sola_kit::components::number_input;
use sola_kit::components::text::{body, caption, code, heading, muted};
use sola_kit::components::card;

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
        Self { radius: 8.0, opacity: 80.0 }
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
    let demo = card(
        column![
            caption("Corner radius").style(muted),
            number_input(state.radius, 0.0..=32.0, 1.0, "px", Msg::Radius),
            caption("Opacity").style(muted),
            number_input(state.opacity, 0.0..=100.0, 5.0, "%", Msg::Opacity),
        ]
        .spacing(8),
    );

    column![
        heading("NumberInput"),
        body("Unit-aware stepper for numeric tokens — radius, spacing, text size.")
            .style(muted),
        demo,
        code("number_input(value, 0.0..=32.0, 1.0, \"px\", Msg::Radius)").style(muted),
    ]
    .spacing(16)
    .into()
}
