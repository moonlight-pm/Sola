//! Form showcase — settings rows in a product panel.

use iced::widget::{checkbox, column, container, text, toggler};
use iced::{Border, Color, Element, Length};

use sola_kit::components::form::{checkbox_style, form_row, toggle_style};
use sola_kit::components::style::{RADIUS_XL, SPACE_MD, bevel_frame, stage_fill};
use sola_kit::components::text::{body, caption, heading, muted};
use sola_kit::fonts;

#[derive(Clone, Debug)]
pub enum Msg {
    Notifications(bool),
    LaunchAtLogin(bool),
    Analytics(bool),
    Wifi(bool),
    Bluetooth(bool),
}

#[derive(Default)]
pub struct State {
    pub notifications: bool,
    pub launch_at_login: bool,
    pub analytics: bool,
    pub wifi: bool,
    pub bluetooth: bool,
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Notifications(v) => self.notifications = v,
            Msg::LaunchAtLogin(v) => self.launch_at_login = v,
            Msg::Analytics(v) => self.analytics = v,
            Msg::Wifi(v) => self.wifi = v,
            Msg::Bluetooth(v) => self.bluetooth = v,
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let prefs = container(
        column![
            text("Preferences").font(fonts::display()).size(16),
            body("Label left, control right. Height 32. Parent owns state.").style(muted),
            form_row(
                "Wi‑Fi",
                toggler(state.wifi).on_toggle(Msg::Wifi).style(toggle_style),
            ),
            form_row(
                "Bluetooth",
                toggler(state.bluetooth)
                    .on_toggle(Msg::Bluetooth)
                    .style(toggle_style),
            ),
            form_row(
                "Launch at login",
                checkbox(state.launch_at_login)
                    .on_toggle(Msg::LaunchAtLogin)
                    .style(checkbox_style),
            ),
            form_row(
                "Notifications",
                checkbox(state.notifications)
                    .on_toggle(Msg::Notifications)
                    .style(checkbox_style),
            ),
            form_row(
                "Share anonymous analytics",
                checkbox(state.analytics)
                    .on_toggle(Msg::Analytics)
                    .style(checkbox_style),
            ),
            caption("Stacked label + input lives on Field.").style(muted),
        ]
        .spacing(SPACE_MD),
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
        heading("Form"),
        body("Settings-grade path. One panel, not two stacked sample cards.").style(muted),
        container(prefs)
            .padding(1)
            .width(Length::Fill)
            .style(|theme: &iced::Theme| {
                bevel_frame(theme.extended_palette().background.weaker.color, RADIUS_XL)
            }),
    ]
    .spacing(16)
    .into()
}
