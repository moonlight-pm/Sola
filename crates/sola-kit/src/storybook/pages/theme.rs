//! Theme page — accent preset picker, per-role font selectors, and a
//! swatch grid of every atom in `sola_kit::theme::Atoms`. Edits emit
//! `Msg::SetAccent` or `Msg::SetFont`; the storybook update rebuilds
//! its iced theme, calls `sola_kit::fonts::install`, and re-emits
//! `Topic::Theme` so every kit consumer picks up the change.

use iced::widget::{column, container, mouse_area, pick_list, row};
use iced::{Color, Element, Length, Padding};

use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted, subheading};
use sola_kit::theme::{self, Atoms, FontSelection};

use crate::storybook::{FontRole, Msg};

const SWATCH_SIZE: f32 = 56.0;
const PRESET_SIZE: f32 = 40.0;
const GRID_GAP: f32 = 44.0;

const ACCENT_PRESETS: &[(&str, &str)] = &[
    ("Sky",     "#58a6ff"),
    ("Cyan",    "#00d4ff"),
    ("Mint",    "#3fb950"),
    ("Amber",   "#d29922"),
    ("Magenta", "#bc8cff"),
];

pub fn view<'a>(
    atoms: &'a Atoms,
    fonts: &'a FontSelection,
    families: &'a [String],
    editable: bool,
) -> Element<'a, Msg> {
    let intro: Element<'a, Msg> = if editable {
        body(
            "Live editor for the kit's atoms and font roles. Edits rebuild \
             the iced theme, reinstall the fonts table, and re-emit \
             Topic::Theme so other kit apps — sola-monitor today — update \
             on their next render."
        )
        .style(muted)
        .into()
    } else {
        body(
            "Default theme — read-only. Click \"New Theme\" in the header \
             above to fork it under a new name; edits then route to that \
             copy."
        )
        .style(muted)
        .into()
    };

    column![
        heading("Theme"),
        intro,

        subheading("Accent"),
        presets_row(atoms.accent),

        subheading("Fonts"),
        body(
            "Pick a family per role. Selections route through the bus, so \
             sola-monitor's body / chrome / mono text swaps in real time."
        )
        .style(muted),
        fonts_grid(fonts, families),

        subheading("Palette atoms"),
        body(
            "Read-only for now. Component style fns reach these via \
             theme.extended_palette() — the atom→slot bindings live in \
             sola_kit::theme::build_theme."
        )
        .style(muted),
        atom_grid(atoms),
    ]
    .spacing(28)
    .into()
}

fn presets_row(current: Color) -> Element<'static, Msg> {
    let mut r = row![].spacing(16);
    for (label, hex_str) in ACCENT_PRESETS {
        let color = theme::parse(hex_str);
        let selected = colors_eq(color, current);
        r = r.push(preset_tile(label, hex_str, color, selected));
    }
    r.into()
}

fn preset_tile<'a>(
    label: &'a str,
    hex_str: &'a str,
    color: Color,
    selected: bool,
) -> Element<'a, Msg> {
    let ring = if selected { 2.0 } else { 0.0 };
    let tile = container(swatch_sized::<Msg>(color, PRESET_SIZE))
        .padding(Padding::from(ring))
        .style(move |theme: &iced::Theme| {
            let p = theme.extended_palette();
            iced::widget::container::Style {
                border: iced::Border {
                    color: if ring > 0.0 { p.primary.base.color } else { Color::TRANSPARENT },
                    width: ring,
                    radius: 8.0.into(),
                },
                ..iced::widget::container::Style::default()
            }
        });

    column![
        mouse_area(tile).on_press(Msg::SetAccent(color)),
        caption(label).style(muted),
        code(hex_str).style(muted),
    ]
    .spacing(4)
    .width(Length::Fixed(PRESET_SIZE + 16.0))
    .into()
}

fn fonts_grid<'a>(fonts: &'a FontSelection, families: &'a [String]) -> Element<'a, Msg> {
    column![
        font_row("ui",        FontRole::Ui,       &fonts.ui,        families),
        font_row("ui_medium", FontRole::UiMedium, &fonts.ui_medium, families),
        font_row("display",   FontRole::Display,  &fonts.display,   families),
        font_row("chrome",    FontRole::Chrome,   &fonts.chrome,    families),
        font_row("mono",      FontRole::Mono,     &fonts.mono,      families),
    ]
    .spacing(12)
    .into()
}

fn font_row<'a>(
    role_label: &'a str,
    role: FontRole,
    current: &str,
    families: &'a [String],
) -> Element<'a, Msg> {
    let selected = Some(current.to_string());
    let picker = pick_list(families, selected, move |family| Msg::SetFont(role, family))
        .width(Length::Fixed(220.0));

    row![
        container(body(role_label)).width(Length::Fixed(120.0)),
        picker,
    ]
    .spacing(16)
    .align_y(iced::Alignment::Center)
    .into()
}

fn atom_grid(atoms: &Atoms) -> Element<'_, Msg> {
    let rows: &[(&str, Color, &str)] = &[
        ("BG",        atoms.bg,        "background.base / weakest"),
        ("BG_RAISED", atoms.bg_raised, "background.weaker / weak"),
        ("BG_HOVER",  atoms.bg_hover,  "background.neutral / strong"),
        ("BORDER",    atoms.border,    "background.stronger / strongest"),
        ("FG",        atoms.fg,        "palette.text"),
        ("FG_MUTED",  atoms.fg_muted,  "secondary.base.text"),
        ("ACCENT",    atoms.accent,    "primary.base"),
        ("SUCCESS",   atoms.success,   "success.base"),
        ("WARNING",   atoms.warning,   "warning.base"),
        ("DANGER",    atoms.danger,    "danger.base"),
    ];

    rows.chunks(5).fold(column![].spacing(GRID_GAP), |col, chunk| {
        let r = chunk.iter().fold(row![].spacing(GRID_GAP), |r, (name, c, slot)| {
            r.push(swatch_tile(name, *c, slot))
        });
        col.push(r)
    })
    .into()
}

fn swatch_tile<'a>(name: &'a str, color: Color, slot: &'a str) -> Element<'a, Msg> {
    column![
        swatch_sized::<Msg>(color, SWATCH_SIZE),
        body(name),
        code(color_hex(color)).style(muted),
        caption(slot).style(muted),
    ]
    .spacing(6)
    .width(Length::Fixed(SWATCH_SIZE + 16.0))
    .into()
}

fn colors_eq(a: Color, b: Color) -> bool {
    let eps = 1.0 / 255.0;
    (a.r - b.r).abs() < eps
        && (a.g - b.g).abs() < eps
        && (a.b - b.b).abs() < eps
}

fn color_hex(c: Color) -> String {
    let r = (c.r * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (c.g * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (c.b * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{r:02x}{g:02x}{b:02x}")
}
