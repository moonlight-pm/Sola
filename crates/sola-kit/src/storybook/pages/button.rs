//! Button showcase — every kit-named style fn shown side-by-side so
//! the visual hierarchy reads at a glance.

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};

use sola_kit::components::button as kit_btn;
use sola_kit::components::card::style as card_style;
use sola_kit::components::icon;
use sola_kit::components::text::{body, caption, code, heading, muted};

#[derive(Clone, Debug)]
pub enum Msg {
    ArmConfirm,
    Confirm,
    Noop,
}

#[derive(Default)]
pub struct State {
    pub confirm_armed: bool,
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::ArmConfirm => self.confirm_armed = true,
            Msg::Confirm => self.confirm_armed = false,
            Msg::Noop => {}
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    // One filled primary in this row — secondary/ghost/danger for the rest.
    let buttons = row![
        button(text("Primary")).style(kit_btn::primary).on_press(Msg::Noop),
        button(text("Secondary")).style(kit_btn::secondary).on_press(Msg::Noop),
        button(text("Ghost")).style(kit_btn::ghost).on_press(Msg::Noop),
        button(text("Danger")).style(kit_btn::danger).on_press(Msg::Noop),
    ]
    .spacing(8);

    // Density-baked helpers: 13 + PAD_CONTROL / 12 + PAD_CONTROL_SM.
    let labeled = row![
        kit_btn::labeled("Labeled primary", kit_btn::primary).on_press(Msg::Noop),
        kit_btn::labeled("Labeled secondary", kit_btn::secondary).on_press(Msg::Noop),
        kit_btn::labeled_sm("Labeled sm", kit_btn::ghost).on_press(Msg::Noop),
    ]
    .spacing(8);

    let disabled = row![
        button(text("Primary")).style(kit_btn::primary),
        button(text("Secondary")).style(kit_btn::secondary),
        button(text("Ghost")).style(kit_btn::ghost),
        button(text("Danger")).style(kit_btn::danger),
    ]
    .spacing(8);

    // Destructive matrix: outline alone + two-stage confirm.
    let destructive = row![
        kit_btn::labeled("Danger outline", kit_btn::danger_outline).on_press(Msg::Noop),
        kit_btn::confirm_button(
            state.confirm_armed,
            "Delete",
            "Confirm?",
            Msg::ArmConfirm,
            Msg::Confirm,
        )
        .padding(sola_kit::components::style::PAD_CONTROL),
    ]
    .spacing(8);

    let list_items = row![
        button(text("Selected row")).style(kit_btn::list_item(true)).on_press(Msg::Noop).width(200),
        button(text("Unselected row")).style(kit_btn::list_item(false)).on_press(Msg::Noop).width(200),
    ]
    .spacing(8);

    let menu_items = row![
        button(text("Copy")).style(kit_btn::menu_item).on_press(Msg::Noop).width(180),
        button(text("Paste")).style(kit_btn::menu_item).on_press(Msg::Noop).width(180),
    ]
    .spacing(8);

    // Menubar demo: dark bar container with system flower + rest / active labels.
    let menubar_bar = container(
        row![
            button(icon("sola/flower", 14))
                .style(kit_btn::menubar(false))
                .padding([2, 9])
                .on_press(Msg::Noop),
            button(text("File")).style(kit_btn::menubar(false)).on_press(Msg::Noop),
            button(text("Edit")).style(kit_btn::menubar(true)).on_press(Msg::Noop),
        ]
        .spacing(2),
    )
    .style(|_theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::BLACK)),
        ..Default::default()
    })
    .padding(4)
    .width(Length::Fill);

    // Product moment — one primary per group, as the OD control stage shows.
    let product = container(
        column![
            caption("Product moment — session identity footer").style(muted),
            row![
                kit_btn::labeled("Save theme", kit_btn::primary).on_press(Msg::Noop),
                kit_btn::labeled("Cancel", kit_btn::secondary).on_press(Msg::Noop),
                kit_btn::labeled("Revert", kit_btn::ghost).on_press(Msg::Noop),
                kit_btn::confirm_button(
                    state.confirm_armed,
                    "Delete",
                    "Confirm?",
                    Msg::ArmConfirm,
                    Msg::Confirm,
                )
                .padding(sola_kit::components::style::PAD_CONTROL),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .style(card_style)
    .padding(18)
    .width(Length::Fill);

    let demo = container(
        column![
            caption("Interactive — one primary per group; ghost is muted until hover").style(muted),
            buttons,
            caption("labeled / labeled_sm — PAD_CONTROL [7,14] / PAD_CONTROL_SM [5,11]").style(muted),
            labeled,
            caption("Disabled (no on_press)").style(muted),
            disabled,
            caption("Destructive — danger_outline + confirm_button (armed state)").style(muted),
            destructive,
            caption("List item — quiet selection / unselected").style(muted),
            list_items,
            caption("Menu item — compact hover (shell menus)").style(muted),
            menu_items,
            caption("Menubar — rest / active (\"Edit\" is open)").style(muted),
            menubar_bar,
        ]
        .spacing(12),
    )
    .style(card_style)
    .padding(18)
    .width(Length::Fill);

    column![
        heading("Button"),
        body(
            "Primary carries soft glow + dark label. Secondary is a quiet fill, \
             not a bare outline. Ghost stays muted until hover. One primary per group."
        )
        .style(muted),
        product,
        demo,
        code("button::labeled(\"Save\", button::primary) · confirm_button(armed, …)").style(muted),
    ]
    .spacing(16)
    .into()
}
