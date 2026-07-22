//! Form showcase — horizontal settings rows, checkbox, and toggler
//! with kit styles (P7d).

use iced::widget::{checkbox, column, toggler};
use iced::Element;

use sola_kit::components::card;
use sola_kit::components::form::{checkbox_style, form_row, toggle_style};
use sola_kit::components::style::SPACE_MD;
use sola_kit::components::text::{body, code, heading, muted, subheading};

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
    let rows = card(
        column![
            form_row(
                "Wi‑Fi",
                toggler(state.wifi)
                    .on_toggle(Msg::Wifi)
                    .style(toggle_style),
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
        ]
        .spacing(SPACE_MD),
    );

    let checks = card(
        column![
            checkbox(state.notifications)
                .label("Enable notifications")
                .on_toggle(Msg::Notifications)
                .style(checkbox_style),
            checkbox(state.analytics)
                .label("Share anonymous analytics")
                .on_toggle(Msg::Analytics)
                .style(checkbox_style),
        ]
        .spacing(SPACE_MD),
    );

    column![
        heading("Form"),
        body(
            "Settings-grade path: form_row (label left / control right), \
             checkbox_style, toggle_style. Parent owns state."
        )
        .style(muted),
        subheading("form_row"),
        body("Height 32, body label at full contrast, SPACE_MD vertical rhythm.")
            .style(muted),
        rows,
        subheading("checkbox / toggler"),
        body("Selected = accent; unselected = raised + hairline (checkbox) or grey track (toggle).")
            .style(muted),
        checks,
        code("form_row(\"Wi‑Fi\", toggler(on).on_toggle(Msg::Wifi).style(toggle_style))")
            .style(muted),
    ]
    .spacing(16)
    .into()
}
