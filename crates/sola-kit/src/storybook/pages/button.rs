//! Button showcase — composed groups, not a widget zoo.

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding};

use sola_kit::components::button as kit_btn;
use sola_kit::components::icon;
use sola_kit::components::style::{
    HAIRLINE_A, PAD_CONTROL, RADIUS_MD, RADIUS_XL, bevel_frame, mix, mix_white, stage_fill,
};
use sola_kit::components::text::{body, caption, heading, muted};
use sola_kit::fonts;

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
    column![
        heading("Button"),
        body(
            "Use labeled / labeled_sm. One filled accent per group. \
             Ghost stays muted until hover. Danger never competes with Save."
        )
        .style(muted),
        row![dialog(state), style_key(state)]
            .spacing(14)
            .width(Length::Fill),
        scenes(),
    ]
    .spacing(20)
    .width(Length::Fill)
    .into()
}

fn dialog(state: &State) -> Element<'_, Msg> {
    let face = container(
        column![
            text("Save theme changes").font(fonts::display()).size(16),
            body("One primary in the footer. Everything else is secondary or destructive.")
                .style(muted),
            hairline(),
            container(
                row![
                    kit_btn::labeled_sm("Delete", kit_btn::danger_outline).on_press(Msg::Noop),
                    Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Revert", kit_btn::ghost).on_press(Msg::Noop),
                    kit_btn::labeled_sm("Cancel", kit_btn::secondary).on_press(Msg::Noop),
                    kit_btn::labeled_sm("Save", kit_btn::primary).on_press(Msg::Noop),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding(Padding {
                top: 4.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            }),
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

    let _ = state;
    container(face)
        .padding(1)
        .width(Length::FillPortion(3))
        .style(|theme: &iced::Theme| {
            sola_kit::components::style::bevel_frame(
                theme.extended_palette().background.weaker.color,
                RADIUS_XL,
            )
        })
        .into()
}

fn style_key(state: &State) -> Element<'_, Msg> {
    let face = container(
        column![
            row![
                text("Style key").font(fonts::ui_medium()).size(13),
                Space::new().width(Length::Fill),
                caption("labeled + PAD_CONTROL").style(muted),
            ]
            .align_y(Alignment::Center),
            style_row(
                "PRIMARY",
                kit_btn::labeled("Save theme", kit_btn::primary)
                    .on_press(Msg::Noop)
                    .into(),
            ),
            style_row(
                "SECONDARY",
                kit_btn::labeled("Cancel", kit_btn::secondary)
                    .on_press(Msg::Noop)
                    .into(),
            ),
            style_row(
                "GHOST",
                kit_btn::labeled("Revert", kit_btn::ghost)
                    .on_press(Msg::Noop)
                    .into(),
            ),
            style_row(
                "DANGER",
                kit_btn::confirm_button(
                    state.confirm_armed,
                    "Delete",
                    "Confirm?",
                    Msg::ArmConfirm,
                    Msg::Confirm,
                )
                .padding(PAD_CONTROL)
                .into(),
            ),
            caption("Disabled is the same helper with no on_press.").style(muted),
            row![
                kit_btn::labeled_sm("Save", kit_btn::primary),
                kit_btn::labeled_sm("Cancel", kit_btn::secondary),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .padding(16)
    .width(Length::Fill)
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        let fill = mix(p.background.weaker.color, p.background.base.color, 0.90);
        iced::widget::container::Style {
            background: Some(Background::Color(fill)),
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
        .width(Length::FillPortion(2))
        .style(|theme: &iced::Theme| {
            let p = theme.extended_palette();
            let fill = mix(p.background.weaker.color, p.background.base.color, 0.90);
            bevel_frame(fill, RADIUS_XL)
        })
        .into()
}

fn scenes() -> Element<'static, Msg> {
    column![
        sub_label("Menubar"),
        container(
            row![
                {
                    use iced::widget::button;
                    button(icon("sola/flower", 14))
                        .style(kit_btn::menubar(false))
                        .padding([2, 9])
                        .on_press(Msg::Noop)
                },
                {
                    use iced::widget::button;
                    button(text("File").size(13))
                        .style(kit_btn::menubar(false))
                        .on_press(Msg::Noop)
                },
                {
                    use iced::widget::button;
                    button(text("Edit").size(13))
                        .style(kit_btn::menubar(true))
                        .on_press(Msg::Noop)
                },
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        )
        .padding(4)
        .width(Length::Fill)
        .style(|_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(Background::Color(Color::BLACK)),
            ..Default::default()
        }),
        sub_label("List rows"),
        column![
            {
                use iced::widget::button;
                button(text("Selected row").size(13))
                    .style(kit_btn::list_item(true))
                    .on_press(Msg::Noop)
                    .width(Length::Fill)
            },
            {
                use iced::widget::button;
                button(text("Unselected row").size(13))
                    .style(kit_btn::list_item(false))
                    .on_press(Msg::Noop)
                    .width(Length::Fill)
            },
        ]
        .spacing(2)
        .width(Length::Fill),
    ]
    .spacing(8)
    .into()
}

fn style_row<'a>(label: &'static str, sample: Element<'a, Msg>) -> Element<'a, Msg> {
    container(
        row![
            text(label)
                .font(fonts::ui_medium())
                .size(11)
                .style(muted)
                .width(Length::Fixed(88.0)),
            sample,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([8, 10]))
    .width(Length::Fill)
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        let fill = mix(p.background.base.color, p.background.weaker.color, 0.55);
        iced::widget::container::Style {
            background: Some(Background::Color(fill)),
            border: Border {
                color: mix_white(fill, HAIRLINE_A * 0.70),
                width: 1.0,
                radius: RADIUS_MD.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

fn sub_label(s: &'static str) -> Element<'static, Msg> {
    text(s).font(fonts::ui_medium()).size(13).into()
}

fn hairline() -> Element<'static, Msg> {
    container(Space::new().width(Length::Fill).height(1))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|theme: &iced::Theme| {
            let raised = theme.extended_palette().background.weaker.color;
            iced::widget::container::Style {
                background: Some(Background::Color(mix_white(raised, HAIRLINE_A))),
                ..Default::default()
            }
        })
        .into()
}
