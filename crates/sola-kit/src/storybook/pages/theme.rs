//! Theme page — accent preset picker plus a swatch grid of every
//! atom in `sola_kit::theme::Atoms`. Clicking a preset emits
//! [`Msg::SetAccent`]; the storybook update rebuilds its iced theme,
//! emits `Topic::Theme` on the bus, and every kit consumer (sola-monitor,
//! the storybook itself) picks up the new accent on its next render.

use iced::widget::{column, container, mouse_area, row};
use iced::{Color, Element, Length, Padding};

use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted, subheading};
use sola_kit::theme::{self, Atoms};

use crate::storybook::Msg;

const SWATCH_SIZE: f32 = 56.0;
const PRESET_SIZE: f32 = 40.0;

/// Five preset accents — the bus-side `from_bus_theme` lookup keys
/// off `"accent"`, so any sufficiently bright colour roundtrips.
const ACCENT_PRESETS: &[(&str, &str)] = &[
    ("Sky",     "#58a6ff"),
    ("Cyan",    "#00d4ff"),
    ("Mint",    "#3fb950"),
    ("Amber",   "#d29922"),
    ("Magenta", "#bc8cff"),
];

pub fn view(atoms: &Atoms) -> Element<'_, Msg> {
    column![
        heading("Theme"),
        body(
            "Click a preset to set the accent atom. The storybook rebuilds \
             its iced theme from the editable Atoms struct and re-emits \
             Topic::Theme so every other kit app — sola-monitor included \
             — picks up the change on its next render."
        )
        .style(muted),

        subheading("Accent"),
        presets_row(atoms.accent),

        subheading("Palette atoms"),
        body(
            "Read-only for now. Component style fns reach these via \
             theme.extended_palette() — the atom→slot bindings live in \
             sola_kit::theme::iced_theme_from_atoms."
        )
        .style(muted),
        atom_grid(atoms),
    ]
    .spacing(24)
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
    // Wrap the swatch in a thin selection ring when it matches the
    // current accent; mouse_area routes the click without painting
    // a button surface over the colour.
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

    let stack = column![
        mouse_area(tile).on_press(Msg::SetAccent(color)),
        caption(label).style(muted),
        code(hex_str).style(muted),
    ]
    .spacing(4)
    .width(Length::Fixed(PRESET_SIZE + 16.0));
    stack.into()
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

    rows.chunks(5).fold(column![].spacing(28), |col, chunk| {
        let r = chunk.iter().fold(row![].spacing(28), |r, (name, c, slot)| {
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
