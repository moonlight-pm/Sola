//! Select showcase — identity trigger + hanging menu.

use iced::widget::{column, container};
use iced::{Element, Length};

use sola_kit::components::card::style as card_style;
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::text::{body, code, heading, muted};

#[derive(Clone, Debug)]
pub enum Msg {
    Toggle,
    Dismiss,
    Pick(usize),
}

pub struct State {
    pub open: bool,
    pub active: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            active: 0,
        }
    }
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Toggle => self.open = !self.open,
            Msg::Dismiss => self.open = false,
            Msg::Pick(i) => {
                self.active = i;
                self.open = false;
            }
        }
    }
}

const NAMES: [&str; 3] = ["Primary", "Alternate", "Work"];
const SEEDS: [&str; 3] = ["seed-primary", "seed-alternate", "seed-work"];

pub fn view(state: &State) -> Element<'_, Msg> {
    let options = NAMES
        .iter()
        .zip(SEEDS)
        .enumerate()
        .map(|(i, (name, seed))| {
            SelectOption::new(*name, i == state.active, Msg::Pick(i)).mark(seed)
        });

    let demo = container(
        select(
            NAMES[state.active],
            options,
            state.open,
            Msg::Toggle,
            Msg::Dismiss,
        ),
    )
    .width(Length::Fixed(220.0))
    .style(card_style)
    .padding(16);

    column![
        heading("Select"),
        body(
            "Identity select. Each option carries an enamel plate from a \
             stable seed. The menu hangs under the trigger; the active row \
             uses the quiet selection wash and a lucide check."
        )
        .style(muted),
        demo,
        code(
            "select(label, options, open, Toggle, Dismiss)\n\
             SelectOption::new(name, selected, Pick(id)).mark(id)"
        )
        .style(muted),
    ]
    .spacing(16)
    .into()
}
