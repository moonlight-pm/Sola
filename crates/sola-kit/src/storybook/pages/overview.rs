//! Overview — design-system north star, tokens, type, density, control stage.
//!
//! Mirrors Open Design `sola-kit-ds.html` system page. Stateless layout
//! except the control-stage buttons, which reuse the Button page's
//! confirm-armed state via [`pages::button::State`].

use iced::widget::{column, container, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use sola_kit::components::badge::{self, Tone};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card;
use sola_kit::components::style::{RADIUS_MD, RADIUS_SM};
use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{
    body, caption, code, heading, muted, subheading,
};
use sola_kit::components::text_input as kit_text_input;
use sola_kit::theme::{self, Atoms};

use crate::storybook::pages::button::{self as button_page, Msg as ButtonMsg};

/// Overview content. `button_state` drives the control-stage confirm button.
pub fn view<'a>(
    atoms: &'a Atoms,
    button_state: &'a button_page::State,
) -> Element<'a, ButtonMsg> {
    column![
        heading("Sola Design System"),
        body(
            "A freer re-image of sola-kit for a Wayland shell — cool graphite \
             tool UI, sparse cyan signal, quiet selection. Seed atoms remain \
             editable on the Theme page; production greys no longer own the room."
        )
        .style(muted),
        north_star(),
        selection_compare(atoms.selection),
        color_foundation(atoms),
        type_roles(),
        spacing_radius(),
        control_stage(button_state),
    ]
    .spacing(22)
    .width(Length::Fill)
    .into()
}

fn north_star() -> Element<'static, ButtonMsg> {
    let hero = column![
        caption("NORTH STAR").style(muted),
        text("Dense chrome.")
            .font(sola_kit::fonts::display())
            .size(20),
        text("One decisive accent.")
            .font(sola_kit::fonts::display())
            .size(20),
        body(
            "Elevation from background steps and soft materials. Selection is \
             intent, not a grey slab. Controls live in product compositions — \
             never as a dump of naked widgets."
        )
        .style(muted),
    ]
    .spacing(6)
    .width(Length::FillPortion(3));

    let rules = column![
        rule_row("Tokens first", "All chrome resolves through kit / bus atoms. No snowflake hex in views."),
        rule_row("Materials", "Sidebar and header use soft raised fills; depth without heavy chrome borders."),
        rule_row("Density", "Quiet, compact chrome. Status over hierarchy theater."),
        rule_row("One primary", "At most one filled accent control per group. Ghost = muted lift only."),
    ]
    .spacing(10)
    .width(Length::FillPortion(2));

    card(row![hero, rules].spacing(20).width(Length::Fill))
        .padding(18)
        .width(Length::Fill)
        .into()
}

fn rule_row(title: &'static str, body_text: &'static str) -> Element<'static, ButtonMsg> {
    column![
        body(title),
        caption(body_text).style(muted),
    ]
    .spacing(2)
    .into()
}

fn selection_compare(selection: Color) -> Element<'static, ButtonMsg> {
    let flat_grey = Color::from_rgb(0.294, 0.294, 0.294); // #4b4b4b

    row![
        compare_card(
            "Flat grey · low structure",
            "drift",
            false,
            flat_grey,
            "#4b4b4b — flat grey, low structure",
        ),
        compare_card(
            "Seed selection · quiet intent",
            "bound",
            true,
            selection,
            "#163842 — teal-grey, quiet intent",
        ),
    ]
    .spacing(12)
    .width(Length::Fill)
    .into()
}

fn compare_card(
    header: &'static str,
    tag: &'static str,
    bound: bool,
    active_fill: Color,
    caption_text: &'static str,
) -> Element<'static, ButtonMsg> {
    let tag_el = container(caption(tag).style(if bound {
        sola_kit::components::text::accent
    } else {
        muted
    }))
    .padding(Padding::from([2, 7]))
    .style(move |theme: &iced::Theme| {
        let p = theme.extended_palette();
        if bound {
            iced::widget::container::Style {
                background: Some(Background::Color(Color {
                    a: 0.10,
                    ..p.primary.base.color
                })),
                border: Border {
                    color: Color {
                        a: 0.35,
                        ..p.primary.base.color
                    },
                    width: 1.0,
                    radius: 999.0.into(),
                },
                ..Default::default()
            }
        } else {
            iced::widget::container::Style {
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.07),
                    width: 1.0,
                    radius: 999.0.into(),
                },
                ..Default::default()
            }
        }
    });

    let hdr = container(
        row![
            caption(header).style(muted),
            Space::new().width(Length::Fill),
            tag_el,
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding::from([8, 12]))
    .width(Length::Fill)
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        iced::widget::container::Style {
            background: Some(Background::Color(Color {
                a: 0.80,
                ..p.background.weaker.color
            })),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.07),
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    });

    let rows = column![
        sel_row("Theme", false, active_fill),
        sel_row("Field", true, active_fill),
        sel_row("Button", false, active_fill),
        caption(caption_text).style(muted),
    ]
    .spacing(4)
    .padding(14);

    container(column![hdr, rows])
        .style(card::style)
        .width(Length::Fill)
        .into()
}

fn sel_row(label: &'static str, active: bool, fill: Color) -> Element<'static, ButtonMsg> {
    container(body(label))
        .padding(Padding::from([7, 10]))
        .width(Length::Fill)
        .style(move |_theme: &iced::Theme| iced::widget::container::Style {
            background: active.then_some(Background::Color(fill)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_MD.into(),
            },
            ..Default::default()
        })
        .into()
}

fn color_foundation(atoms: &Atoms) -> Element<'_, ButtonMsg> {
    let rows = column![
        token_row("BG", theme::hex::BG, "Window / canvas", atoms.bg),
        token_row("BG_RAISED", theme::hex::BG_RAISED, "Sidebar · card · menu", atoms.bg_raised),
        token_row("BG_HOVER", theme::hex::BG_HOVER, "Hover lift", atoms.bg_hover),
        token_row("BORDER", theme::hex::BORDER, "Hard edges", atoms.border),
        token_row("FG", theme::hex::FG, "Primary label", atoms.fg),
        token_row("FG_MUTED", theme::hex::FG_MUTED, "Secondary label", atoms.fg_muted),
        token_row("ACCENT", theme::hex::ACCENT, "Focus · primary action", atoms.accent),
        token_row("SUCCESS", theme::hex::SUCCESS, "Semantic success", atoms.success),
        token_row("WARNING", theme::hex::WARNING, "Semantic warning", atoms.warning),
        token_row("DANGER", theme::hex::DANGER, "Semantic danger", atoms.danger),
        token_row("SELECTION", theme::hex::SELECTION, "Selected row (quiet)", atoms.selection),
    ]
    .spacing(6);

    card(
        column![
            subheading("Color foundation"),
            body(
                "Redesign baseline (editable on Theme). Live atoms above; seed \
                 hex strings are the compile-time defaults."
            )
            .style(muted),
            rows,
        ]
        .spacing(10),
    )
    .padding(18)
    .width(Length::Fill)
    .into()
}

fn token_row<'a>(
    name: &'static str,
    seed_hex: &'static str,
    role: &'static str,
    color: Color,
) -> Element<'a, ButtonMsg> {
    row![
        swatch_sized(color, 18.0),
        code(name).width(Length::Fixed(100.0)),
        code(seed_hex).style(muted).width(Length::Fixed(90.0)),
        caption(role).style(muted),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center)
    .into()
}

fn type_roles() -> Element<'static, ButtonMsg> {
    card(
        column![
            subheading("Type roles"),
            heading("Heading · 22 display"),
            subheading("Subheading · 15 display"),
            body("Body UI · 13 regular — settings rows, dialogs, lists"),
            caption("Caption · 11 — help, secondary labels").style(muted),
            code("Code · 12 mono — IDs, detail panels, hex"),
        ]
        .spacing(8),
    )
    .padding(18)
    .width(Length::Fill)
    .into()
}

fn spacing_radius() -> Element<'static, ButtonMsg> {
    let chips = row![
        scale_chip("space-1", "4px"),
        scale_chip("space-2", "8px"),
        scale_chip("space-3", "12px"),
        scale_chip("space-4", "16px"),
        scale_chip("r-sm", "5px"),
        scale_chip("r-md", "7px"),
        scale_chip("r-lg", "10px"),
        scale_chip("pad", "7×14"),
    ]
    .spacing(8)
    .wrap();

    column![subheading("Spacing & radius"), chips]
        .spacing(10)
        .into()
}

fn scale_chip(name: &'static str, value: &'static str) -> Element<'static, ButtonMsg> {
    container(
        column![
            caption(name).style(muted),
            body(value),
        ]
        .spacing(2)
        .align_x(iced::Alignment::Center),
    )
    .padding(Padding::from([8, 12]))
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        iced::widget::container::Style {
            background: Some(Background::Color(p.background.weaker.color)),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.07),
                width: 1.0,
                radius: RADIUS_SM.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

fn control_stage(button_state: &button_page::State) -> Element<'_, ButtonMsg> {
    let product = card(
        column![
            caption("PRODUCT MOMENT").style(muted),
            subheading("Session identity"),
            body(
                "How this kit host names itself to solactl and the switcher. \
                 One primary action in the footer — everything else is secondary \
                 or destructive."
            )
            .style(muted),
            stage_field("Username", "alice"),
            stage_field("Display", "Alice · kit"),
            row![
                caption("Status").style(muted).width(Length::Fixed(92.0)),
                badge::badge("SEED", Tone::Accent),
                badge::badge("BOUND", Tone::Success),
                caption("Theme dirty only after atom edits").style(muted),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            row![
                kit_btn::labeled_sm("Delete", kit_btn::danger_outline)
                    .on_press(ButtonMsg::Noop),
                Space::new().width(Length::Fill),
                kit_btn::labeled_sm("Revert", kit_btn::ghost).on_press(ButtonMsg::Noop),
                kit_btn::labeled_sm("Cancel", kit_btn::secondary).on_press(ButtonMsg::Noop),
                kit_btn::labeled_sm("Save", kit_btn::primary).on_press(ButtonMsg::Noop),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(12),
    )
    .padding(18)
    .width(Length::FillPortion(3));

    let style_key = card(
        column![
            row![
                subheading("Style key"),
                Space::new().width(Length::Fill),
                caption("One primary per group").style(muted),
            ]
            .align_y(iced::Alignment::Center),
            style_row(
                "Primary",
                kit_btn::labeled("Save theme", kit_btn::primary)
                    .on_press(ButtonMsg::Noop)
                    .into(),
            ),
            style_row(
                "Secondary",
                kit_btn::labeled("Cancel", kit_btn::secondary)
                    .on_press(ButtonMsg::Noop)
                    .into(),
            ),
            style_row(
                "Ghost",
                kit_btn::labeled("Revert", kit_btn::ghost)
                    .on_press(ButtonMsg::Noop)
                    .into(),
            ),
            style_row(
                "Danger",
                row![
                    kit_btn::labeled("Delete", kit_btn::danger_outline).on_press(ButtonMsg::Noop),
                    kit_btn::confirm_button(
                        button_state.confirm_armed,
                        "Delete",
                        "Confirm?",
                        ButtonMsg::ArmConfirm,
                        ButtonMsg::Confirm,
                    )
                    .padding(sola_kit::components::style::PAD_CONTROL),
                ]
                .spacing(8)
                .into(),
            ),
            caption(
                "Primary carries soft glow + dark label. Secondary is a quiet fill, \
                 not a bare outline. Ghost stays muted until hover. Danger outline \
                 never competes with the primary."
            )
            .style(muted),
        ]
        .spacing(12),
    )
    .padding(18)
    .width(Length::FillPortion(2));

    column![
        subheading("Control stage"),
        body(
            "How buttons and fields should actually appear — in a composed product \
             surface, not a junk drawer of naked widgets."
        )
        .style(muted),
        row![product, style_key].spacing(12).width(Length::Fill),
    ]
    .spacing(10)
    .into()
}

fn stage_field(label: &'static str, value: &'static str) -> Element<'static, ButtonMsg> {
    row![
        caption(label).style(muted).width(Length::Fixed(92.0)),
        kit_text_input::text_input("", value)
            .style(kit_text_input::style)
            .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}

fn style_row<'a>(
    label: &'static str,
    sample: Element<'a, ButtonMsg>,
) -> Element<'a, ButtonMsg> {
    row![
        caption(label).style(muted).width(Length::Fixed(72.0)),
        sample,
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}
