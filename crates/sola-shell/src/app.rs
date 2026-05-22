//! Shell — central state for the iced shell. This is the skeleton;
//! per-window state and bus dispatch land in subsequent tasks.

use std::sync::Arc;

use iced::widget::{container, text};
use iced::{Element, Length, Subscription};

use sola_kit::theme;

#[derive(Clone, Debug)]
pub enum Msg {
    Bus(Arc<sola_bus::Message>),
    Noop,
}

pub struct Shell {
    theme: iced::Theme,
}

impl Shell {
    pub fn default() -> Self {
        Self { theme: theme::default_theme() }
    }

    pub fn title(&self) -> String {
        "sola-shell".to_string()
    }

    pub fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        sola_kit::app::bus_subscription().map(Msg::Bus)
    }

    pub fn update(&mut self, _msg: Msg) {}

    pub fn view(&self) -> Element<'_, Msg> {
        container(text("sola-shell (iced) — skeleton"))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }
}
