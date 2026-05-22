//! Toolbar button showcase — stateful so we can demonstrate the
//! pressed-count side effect, which mirrors how a real consumer
//! (pause/clear in sola-monitor) wires it up.

use iced::widget::{column, container, row};
use iced::{Element, Length};

use sola_kit::components::card::style as card_style;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::toolbar_button;

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
    let buttons = ["Pause", "Clear", "Reset"];
    let bar = buttons.iter().fold(row![].spacing(6), |r, label| {
        r.push(toolbar_button(label).on_press(Msg::Clicked(label)))
    });

    let demo = container(bar)
        .style(card_style)
        .padding(12)
        .width(Length::Fill);

    let status = match state.last_clicked {
        Some(label) => format!(
            "Last clicked: {} (count: {})",
            label,
            state.counts.get(label).copied().unwrap_or(0),
        ),
        None => "Click a button to see the active state".to_string(),
    };

    column![
        heading("Toolbar"),
        body("Compact buttons with condensed-bold labels — for top-of-window action rows.")
            .style(muted),
        demo,
        body(status).style(muted),
        code("toolbar_button(label).on_press(msg)").style(muted),
    ]
    .spacing(16)
    .into()
}
