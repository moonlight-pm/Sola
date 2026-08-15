//! Toolbar — a monitor-style action row.

use iced::widget::{column, row};
use iced::Element;

use sola_kit::components::text::{body, muted};
use sola_kit::components::toolbar_button;

use crate::storybook::pages::chrome::{lede, panel};

#[derive(Clone, Debug)]
pub enum Msg {
    Clicked(&'static str),
}

#[derive(Default)]
pub struct State {
    last_clicked: Option<&'static str>,
    counts: std::collections::HashMap<&'static str, u32>,
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Clicked(label) => {
                self.last_clicked = Some(label);
                *self.counts.entry(label).or_insert(0) += 1;
            }
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let bar = ["Pause", "Clear", "Reset"]
        .iter()
        .fold(row![].spacing(6), |r, label| {
            r.push(toolbar_button(*label).on_press(Msg::Clicked(*label)))
        });

    let status = match state.last_clicked {
        Some(label) => format!(
            "{} · {}",
            label,
            state.counts.get(label).copied().unwrap_or(0)
        ),
        None => "Compact actions for a top strip.".to_string(),
    };

    column![
        lede(
            "Toolbar",
            "Condensed-bold labels. Same density as monitor Pause / Clear.",
        ),
        panel(
            column![bar, body(status).style(muted)].spacing(12),
        ),
    ]
    .spacing(16)
    .into()
}
