//! Toolbar — a monitor-style action row.

use iced::Element;
use iced::widget::{column, row};

use sola_kit::components::icon::icon_handle;
use sola_kit::components::text::{body, muted};
use sola_kit::components::toolbar::{toolbar_button, toolbar_icon_tip};

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
            "Condensed-bold labels, or icons with a delayed tooltip. Same density as monitor Pause / Clear.",
        ),
        panel(
            column![
                bar,
                row![
                    toolbar_icon_tip(
                        icon_handle("lucide/square-pen"),
                        "Compose",
                        Some(Msg::Clicked("Compose")),
                    ),
                    toolbar_icon_tip(
                        icon_handle("lucide/reply"),
                        "Reply",
                        Some(Msg::Clicked("Reply")),
                    ),
                    toolbar_icon_tip(icon_handle("lucide/archive"), "Archive", None),
                    // None → muted, no tooltip (unavailable).
                ]
                .spacing(4),
                body(status).style(muted),
            ]
            .spacing(12),
        ),
    ]
    .spacing(16)
    .into()
}
