//! Select showcase — hanging menu in chrome and in a form row.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Border, Color, Element, Length, Padding};

use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{bevel_frame, stage_fill, CHROME_SURFACE, RADIUS_XL};
use sola_kit::components::text::{body, caption, heading, muted};
use sola_kit::fonts;

#[derive(Clone, Debug)]
pub enum Msg {
    Toggle,
    Dismiss,
    Pick(usize),
    ChromeToggle,
    ChromeDismiss,
    ChromePick(usize),
}

pub struct State {
    pub open: bool,
    pub active: usize,
    pub chrome_open: bool,
    pub chrome_active: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            active: 0,
            chrome_open: false,
            chrome_active: 0,
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
            Msg::ChromeToggle => self.chrome_open = !self.chrome_open,
            Msg::ChromeDismiss => self.chrome_open = false,
            Msg::ChromePick(i) => {
                self.chrome_active = i;
                self.chrome_open = false;
            }
        }
    }
}

const NAMES: [&str; 3] = ["Primary", "Alternate", "Work"];
const SEEDS: [&str; 3] = ["seed-primary", "seed-alternate", "seed-work"];

pub fn view(state: &State) -> Element<'_, Msg> {
    column![
        heading("Select"),
        body(
            "Identity select. Enamel plate from a stable seed. The menu hangs \
             under the trigger at the trigger's width — a raised popover, not \
             a darker inset card."
        )
        .style(muted),
        chrome_bar(state),
        form_panel(state),
    ]
    .spacing(20)
    .into()
}

fn chrome_bar(state: &State) -> Element<'_, Msg> {
    let options = NAMES.iter().zip(SEEDS).enumerate().map(|(i, (name, seed))| {
        SelectOption::new(*name, i == state.chrome_active, Msg::ChromePick(i)).mark(seed)
    });

    column![
        text("Chrome bar").font(fonts::ui_medium()).size(13),
        caption("Same grammar as the browser profile switcher.").style(muted),
        container(
            row![
                container(select(
                    NAMES[state.chrome_active],
                    options,
                    state.chrome_open,
                    Msg::ChromeToggle,
                    Msg::ChromeDismiss,
                ))
                .width(Length::Fixed(200.0)),
                Space::new().width(Length::Fill),
                caption("https://sola.computer").style(muted),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([8, 12]))
        .width(Length::Fill)
        .style(|_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(CHROME_SURFACE)),
            ..Default::default()
        }),
    ]
    .spacing(8)
    .into()
}

fn form_panel(state: &State) -> Element<'_, Msg> {
    let options = NAMES.iter().zip(SEEDS).enumerate().map(|(i, (name, seed))| {
        SelectOption::new(*name, i == state.active, Msg::Pick(i)).mark(seed)
    });

    let face = container(
        column![
            text("Theme")
                .font(fonts::display())
                .size(16),
            body("In a form the menu matches the field, start-aligned.")
                .style(muted),
            row![
                text("Preset")
                    .font(fonts::ui_medium())
                    .size(12)
                    .style(muted)
                    .width(Length::Fixed(92.0)),
                select(
                    NAMES[state.active],
                    options,
                    state.open,
                    Msg::Toggle,
                    Msg::Dismiss,
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(12),
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

    container(face)
        .padding(1)
        .width(Length::Fill)
        .style(|theme: &iced::Theme| {
            bevel_frame(theme.extended_palette().background.weaker.color, RADIUS_XL)
        })
        .into()
}
