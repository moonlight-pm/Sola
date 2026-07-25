//! Overview — design-system north star, tokens, type, density, control stage.
//!
//! Mirrors Open Design `sola-kit-ds.html` system page. Stateless layout
//! except the control-stage confirm button, which reuses the Button page's
//! armed state via [`pages::button::State`].

use iced::widget::{column, container, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use sola_kit::components::badge::{self, Tone};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card;
use sola_kit::components::style::{
    hairline, hairline_on, hero_fill, mix, mix_white, stage_fill, HAIRLINE_A, HAIRLINE_STRONG_A,
    RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XL,
};
use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted, subheading};
use sola_kit::components::text_input as kit_text_input;
use sola_kit::fonts;
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
            "Cool graphite tool UI for a Wayland shell — sparse cyan signal, \
             quiet selection. Edit seed atoms on the Theme page."
        )
        .style(muted),
        north_star(),
        selection_compare(atoms.selection),
        color_foundation(atoms),
        type_roles(),
        spacing_radius(),
        control_stage(button_state),
    ]
    .spacing(20)
    .width(Length::Fill)
    .into()
}

/// OD `.ds-banner`: hero panel left + four stacked rule cards right
/// (not a single combined card).
fn north_star() -> Element<'static, ButtonMsg> {
    let hero = container(
        column![
            text("NORTH STAR")
                .font(fonts::ui_medium())
                .size(10)
                .style(|theme: &iced::Theme| {
                    let p = theme.extended_palette();
                    // OD: color-mix(accent 80%, fg)
                    iced::widget::text::Style {
                        color: Some(mix(p.primary.base.color, p.background.base.text, 0.80)),
                    }
                }),
            text("Dense chrome.")
                .font(fonts::display())
                .size(20),
            text("One decisive accent.")
                .font(fonts::display())
                .size(20),
            body(
                "Elevation from background steps and soft materials. Selection is \
                 intent, not a grey slab. Controls live in product compositions."
            )
            .style(muted),
        ]
        .spacing(6),
    )
    .padding(Padding::from([22, 22]))
    .width(Length::FillPortion(6))
    .height(Length::Fill)
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        let bg = p.background.base.color;
        let raised = p.background.weaker.color;
        let selection = sola_kit::theme::selection();
        iced::widget::container::Style {
            background: Some(hero_fill(bg, raised, selection)),
            border: hairline_on(raised, HAIRLINE_STRONG_A, RADIUS_XL),
            ..Default::default()
        }
    });

    let rules = column![
        rule_card(
            "Tokens first",
            "All chrome resolves through kit / bus atoms. No snowflake hex in views.",
        ),
        rule_card(
            "Materials",
            "Sidebar and header use soft raised fills; depth without heavy chrome borders.",
        ),
        rule_card(
            "Density",
            "Quiet, compact chrome. Status over hierarchy theater.",
        ),
        rule_card(
            "One primary",
            "At most one filled accent control per group. Ghost = muted lift only.",
        ),
    ]
    .spacing(8)
    .width(Length::FillPortion(5));

    row![hero, rules].spacing(16).width(Length::Fill).into()
}

fn rule_card(title: &'static str, body_text: &'static str) -> Element<'static, ButtonMsg> {
    container(
        column![
            text(title).font(fonts::ui_medium()).size(12),
            caption(body_text).style(muted),
        ]
        .spacing(3),
    )
    .padding(Padding::from([12, 14]))
    .width(Length::Fill)
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        let raised = p.background.weaker.color;
        let bg = p.background.base.color;
        // OD: color-mix(raised 75%, transparent) over canvas → mix with base.
        let fill = mix(raised, bg, 0.75);
        iced::widget::container::Style {
            background: Some(Background::Color(fill)),
            border: hairline_on(fill, HAIRLINE_A, RADIUS_LG),
            ..Default::default()
        }
    })
    .into()
}

fn selection_compare(selection: Color) -> Element<'static, ButtonMsg> {
    let flat_grey = Color::from_rgb(0.294, 0.294, 0.294); // #4b4b4b

    row![
        compare_card(
            "Live custom · SELECTION",
            "drift",
            false,
            flat_grey,
            "#4b4b4b — flat grey, low structure",
        ),
        compare_card(
            "Seed system · SELECTION",
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
    // OD `.tag` — compact uppercase pill. Bound gets soft accent wash;
    // drift is outline-only muted. Opaque mixes so iced doesn't inflate alpha.
    let tag_el = container(
        text(tag)
            .font(fonts::ui_medium())
            .size(10)
            .style(move |theme: &iced::Theme| {
                let p = theme.extended_palette();
                iced::widget::text::Style {
                    color: Some(if bound {
                        p.primary.base.color
                    } else {
                        p.secondary.base.text
                    }),
                }
            }),
    )
    .padding(Padding::from([2, 7]))
    .style(move |theme: &iced::Theme| {
        let p = theme.extended_palette();
        let raised = p.background.weaker.color;
        if bound {
            let accent = p.primary.base.color;
            iced::widget::container::Style {
                background: Some(Background::Color(mix(accent, raised, 0.10))),
                border: Border {
                    color: mix(accent, raised, 0.35),
                    width: 1.0,
                    radius: 999.0.into(),
                },
                ..Default::default()
            }
        } else {
            iced::widget::container::Style {
                background: None,
                border: hairline_on(raised, HAIRLINE_A, 999.0),
                ..Default::default()
            }
        }
    });

    let hdr = container(
        row![
            text(header)
                .font(fonts::ui_medium())
                .size(11)
                .style(muted),
            Space::new().width(Length::Fill),
            tag_el,
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding::from([8, 12]))
    .width(Length::Fill)
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        let raised = p.background.weaker.color;
        let bg = p.background.base.color;
        iced::widget::container::Style {
            // OD: color-mix(raised 80%, transparent) over canvas.
            background: Some(Background::Color(mix(raised, bg, 0.80))),
            border: Border {
                color: mix_white(raised, HAIRLINE_A),
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    });

    let body_block = container(
        column![
            sel_row("Theme", false, active_fill),
            sel_row("Field", true, active_fill),
            sel_row("Button", false, active_fill),
            container(caption(caption_text).style(muted)).padding(Padding {
                top: 8.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            }),
        ]
        .spacing(4),
    )
    .padding(14)
    .width(Length::Fill)
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        iced::widget::container::Style {
            // OD `.compare-card .body { background: var(--bg) }`
            background: Some(Background::Color(p.background.base.color)),
            ..Default::default()
        }
    });

    // Hairline under header between the two sections.
    let rule = container(Space::new().width(Length::Fill).height(1))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|theme: &iced::Theme| {
            let raised = theme.extended_palette().background.weaker.color;
            iced::widget::container::Style {
                background: Some(Background::Color(mix_white(raised, HAIRLINE_A))),
                ..Default::default()
            }
        });

    container(column![hdr, rule, body_block])
        .style(|theme: &iced::Theme| {
            let p = theme.extended_palette();
            iced::widget::container::Style {
                background: None,
                border: hairline(p, RADIUS_LG),
                ..Default::default()
            }
        })
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
    .spacing(0);

    card(
        column![
            subheading("Color foundation"),
            body("Live atoms · seed hex is the compile-time default. Edit on Theme.")
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
    column![
        container(
            row![
                swatch_sized(color, 22.0),
                code(name).width(Length::Fixed(100.0)),
                code(seed_hex).style(muted).width(Length::Fixed(90.0)),
                caption(role).style(muted),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding::from([8, 0]))
        .width(Length::Fill),
        container(Space::new().width(Length::Fill).height(1))
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|theme: &iced::Theme| {
                let raised = theme.extended_palette().background.weaker.color;
                iced::widget::container::Style {
                    // Opaque sRGB mix — translucent white separators look thick.
                    background: Some(Background::Color(mix_white(raised, 0.06))),
                    ..Default::default()
                }
            }),
    ]
    .width(Length::Fill)
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
        let fill = p.background.weaker.color;
        iced::widget::container::Style {
            background: Some(Background::Color(fill)),
            border: Border {
                color: mix_white(fill, 0.07),
                width: 1.0,
                radius: RADIUS_SM.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

fn control_stage(button_state: &button_page::State) -> Element<'_, ButtonMsg> {
    let product = container(
        column![
            caption("PRODUCT MOMENT").style(muted),
            subheading("Session identity"),
            body(
                "How this kit host names itself. One primary in the footer — \
                 everything else is secondary or destructive."
            )
            .style(muted),
            stage_field("Username", "alice"),
            stage_field("Display", "Alice · kit"),
            row![
                caption("Status").style(muted).width(Length::Fixed(80.0)),
                badge::badge("SEED", Tone::Accent),
                badge::badge("BOUND", Tone::Success),
                caption("Dirty only after atom edits").style(muted),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            container(
                row![
                    kit_btn::labeled_sm("Delete", kit_btn::danger_outline)
                        .on_press(ButtonMsg::Noop),
                    Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Revert", kit_btn::ghost).on_press(ButtonMsg::Noop),
                    kit_btn::labeled_sm("Cancel", kit_btn::secondary).on_press(ButtonMsg::Noop),
                    kit_btn::labeled_sm("Save", kit_btn::primary).on_press(ButtonMsg::Noop),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding {
                top: 14.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            }),
        ]
        .spacing(10),
    )
    .padding(18)
    .width(Length::FillPortion(3))
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        let bg = p.background.base.color;
        let raised = p.background.weaker.color;
        let accent = p.primary.base.color;
        iced::widget::container::Style {
            background: Some(stage_fill(bg, raised, accent)),
            border: hairline_on(raised, HAIRLINE_STRONG_A, RADIUS_XL),
            ..Default::default()
        }
    });

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
            // Single confirm control: idle = outline Delete, armed = filled Confirm?
            style_row(
                "Danger",
                kit_btn::confirm_button(
                    button_state.confirm_armed,
                    "Delete",
                    "Confirm?",
                    ButtonMsg::ArmConfirm,
                    ButtonMsg::Confirm,
                )
                .padding(sola_kit::components::style::PAD_CONTROL)
                .into(),
            ),
            caption("Glow primary · soft secondary · muted ghost · outline danger.")
                .style(muted),
        ]
        .spacing(10),
    )
    .padding(16)
    .width(Length::FillPortion(2));

    column![
        subheading("Control stage"),
        body("Composed product surface — not a junk drawer of naked widgets.").style(muted),
        row![product, style_key].spacing(12).width(Length::Fill),
    ]
    .spacing(8)
    .into()
}

fn stage_field(label: &'static str, value: &'static str) -> Element<'static, ButtonMsg> {
    row![
        caption(label).style(muted).width(Length::Fixed(80.0)),
        kit_text_input::text_input("", value)
            .style(kit_text_input::style)
            .width(Length::Fill),
    ]
    .spacing(10)
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
