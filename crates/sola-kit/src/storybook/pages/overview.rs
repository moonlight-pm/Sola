//! Overview — the kit as a desk, not a style-guide poster.
//!
//! Quiet heading, one composed identity panel, a compact style key, and
//! a thin seed footnote. Theme still owns the atom editor. Selection
//! compare / type stack / spacing chips live on their own pages.

use iced::widget::{Space, column, container, row, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding};

use sola_kit::components::badge::{self, Tone};
use sola_kit::components::button as kit_btn;
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{
    HAIRLINE_A, PAD_CONTROL, RADIUS_MD, RADIUS_XL, bevel_frame, mix, mix_white, stage_fill,
};
use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted, subheading};
use sola_kit::components::text_input as kit_text_input;
use sola_kit::fonts;
use sola_kit::theme::Atoms;

#[derive(Clone, Debug)]
pub enum Msg {
    Noop,
    ArmConfirm,
    Confirm,
    ThemeToggle,
    ThemeDismiss,
    ThemePick(usize),
}

pub struct State {
    pub theme_open: bool,
    pub theme_idx: usize,
    pub confirm_armed: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            theme_open: false,
            theme_idx: 0,
            confirm_armed: false,
        }
    }
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Noop => {}
            Msg::ArmConfirm => self.confirm_armed = true,
            Msg::Confirm => self.confirm_armed = false,
            Msg::ThemeToggle => self.theme_open = !self.theme_open,
            Msg::ThemeDismiss => self.theme_open = false,
            Msg::ThemePick(i) => {
                self.theme_idx = i;
                self.theme_open = false;
            }
        }
    }
}

const THEME_NAMES: [&str; 3] = ["Default", "Graphite", "Night"];
const THEME_SEEDS: [&str; 3] = ["seed-default", "seed-graphite", "seed-night"];

/// Overview content.
pub fn view<'a>(atoms: &'a Atoms, state: &'a State) -> Element<'a, Msg> {
    column![
        heading("Overview"),
        body(
            "Cool graphite tool UI. One filled accent per group. Selection \
             is a quiet well, not a slab. Edit seeds on Theme."
        )
        .style(muted),
        desk(state),
        seeds(atoms),
    ]
    .spacing(20)
    .width(Length::Fill)
    .into()
}

fn desk(state: &State) -> Element<'_, Msg> {
    // Identity is intrinsically taller. Iced `Fill` height collapses on a
    // shrink-height row, so stack: identity defines height; style key
    // fills that limit.
    stack![
        row![
            container(identity(state)).width(Length::FillPortion(3)),
            Space::new().width(Length::FillPortion(2)),
        ]
        .spacing(14)
        .width(Length::Fill),
        row![
            Space::new().width(Length::FillPortion(3)),
            container(style_key(state))
                .width(Length::FillPortion(2))
                .height(Length::Fill),
        ]
        .spacing(14)
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .width(Length::Fill)
    .into()
}

fn identity(state: &State) -> Element<'_, Msg> {
    let theme_options = THEME_NAMES
        .iter()
        .zip(THEME_SEEDS)
        .enumerate()
        .map(|(i, (name, seed))| {
            SelectOption::new(*name, i == state.theme_idx, Msg::ThemePick(i)).mark(seed)
        });

    let theme_select = select(
        THEME_NAMES[state.theme_idx],
        theme_options,
        state.theme_open,
        Msg::ThemeToggle,
        Msg::ThemeDismiss,
    );

    let face = container(
        column![
            text("How this machine names you")
                .font(fonts::display())
                .size(16),
            body(
                "One primary in the footer. Everything else is secondary \
                 or destructive."
            )
            .style(muted),
            stage_field("Username", "naturalethic"),
            stage_field("Display", "Joshua"),
            row![
                text("Theme")
                    .font(fonts::ui_medium())
                    .size(12)
                    .style(muted)
                    .width(Length::Fixed(92.0)),
                theme_select,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            row![
                text("Status")
                    .font(fonts::ui_medium())
                    .size(12)
                    .style(muted)
                    .width(Length::Fixed(92.0)),
                badge::badge("DEFAULT", Tone::Accent),
                badge::badge("CLEAN", Tone::Success),
                caption("Theme dirty only after atom edits").style(muted),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            hairline(),
            container(
                row![
                    kit_btn::labeled_sm("Reset", kit_btn::danger_outline).on_press(Msg::Noop),
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

    container(face)
        .padding(1)
        .width(Length::Fill)
        .style(|theme: &iced::Theme| {
            bevel_frame(theme.extended_palette().background.weaker.color, RADIUS_XL)
        })
        .into()
}

fn style_key(state: &State) -> Element<'_, Msg> {
    let face = container(
        column![
            row![
                text("Style key").font(fonts::ui_medium()).size(13),
                Space::new().width(Length::Fill),
                caption("One primary per group").style(muted),
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
            caption(
                "Primary carries soft gradient + glow. Secondary is a quiet fill, not a bare \
                 outline. Ghost stays muted until hover. Danger outline never competes with the \
                 primary."
            )
            .style(muted),
        ]
        .spacing(10),
    )
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
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
        .height(Length::Fill)
        .style(|theme: &iced::Theme| {
            bevel_frame(theme.extended_palette().background.weaker.color, RADIUS_XL)
        })
        .into()
}

fn seeds(atoms: &Atoms) -> Element<'_, Msg> {
    let chips = [
        ("BG", atoms.bg),
        ("RAISED", atoms.bg_raised),
        ("FG", atoms.fg),
        ("ACCENT", atoms.accent),
        ("SELECT", atoms.selection),
        ("OK", atoms.success),
        ("DANGER", atoms.danger),
    ];

    let row_chips = chips.into_iter().fold(
        row![].spacing(16).align_y(Alignment::Start),
        |r, (name, color)| r.push(seed_chip(name, color)),
    );

    column![
        subheading("Seeds"),
        caption("Live atoms. Full editor and type roles live on Theme.").style(muted),
        row_chips,
    ]
    .spacing(8)
    .into()
}

fn seed_chip(name: &'static str, color: Color) -> Element<'static, Msg> {
    column![
        swatch_sized(color, 28.0),
        text(name).font(fonts::ui_medium()).size(10),
        code(hex_of(color)).style(muted),
    ]
    .spacing(5)
    .width(Length::Fixed(56.0))
    .into()
}

fn hex_of(color: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (color.r * 255.0).round() as u8,
        (color.g * 255.0).round() as u8,
        (color.b * 255.0).round() as u8,
    )
}

fn stage_field(label: &'static str, value: &'static str) -> Element<'static, Msg> {
    row![
        text(label)
            .font(fonts::ui_medium())
            .size(12)
            .style(muted)
            .width(Length::Fixed(92.0)),
        kit_text_input::text_input("", value)
            .style(kit_text_input::style)
            .on_input(|_| Msg::Noop)
            .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
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
