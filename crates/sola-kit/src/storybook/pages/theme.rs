//! Theme page — accent preset picker, per-role font selectors, and a
//! swatch grid of every atom in `sola_kit::theme::Atoms`. Edits emit
//! `Msg::SetAccent` or `Msg::SetFont`; the storybook update rebuilds
//! its iced theme, calls `sola_kit::fonts::install`, and re-emits
//! `Topic::Theme` so every kit consumer picks up the change.

use iced::widget::{column, container, mouse_area, pick_list, row};
use iced::{Color, Element, Length, Padding};

use sola_kit::components::ColorPicker;
use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted, subheading};
use sola_kit::theme::{self, Atoms, FontSelection};

use crate::storybook::{AtomField, FontRole, Msg};

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
    editing: Option<AtomField>,
    picker: Option<&'a ColorPicker>,
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
        fonts_grid(fonts, families, editable),

        subheading("Palette atoms"),
        body(if editable {
            "Click a swatch to edit its atom with the RGB picker. \
             Component style fns reach these via theme.extended_palette() \
             — the atom→slot bindings live in sola_kit::theme::build_theme."
        } else {
            "Read-only on the Default theme. Fork it (\"New Theme\") to \
             click a swatch and edit its atom. Component style fns reach \
             these via theme.extended_palette() — the atom→slot bindings \
             live in sola_kit::theme::build_theme."
        })
        .style(muted),
        atom_grid(atoms, editable, editing, picker),
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
        mouse_area(tile).on_press(Msg::SetAtom(AtomField::Accent, color)),
        caption(label).style(muted),
        code(hex_str).style(muted),
    ]
    .spacing(4)
    .width(Length::Fixed(PRESET_SIZE + 16.0))
    .into()
}
fn fonts_grid<'a>(
    fonts: &'a FontSelection,
    families: &'a [String],
    editable: bool,
) -> Element<'a, Msg> {
    column![
        font_row("ui",        FontRole::Ui,       &fonts.ui,        families, editable),
        font_row("ui_medium", FontRole::UiMedium, &fonts.ui_medium, families, editable),
        font_row("display",   FontRole::Display,  &fonts.display,   families, editable),
        font_row("chrome",    FontRole::Chrome,   &fonts.chrome,    families, editable),
        font_row("mono",      FontRole::Mono,     &fonts.mono,      families, editable),
    ]
    .spacing(12)
    .into()
}

fn font_row<'a>(
    role_label: &'a str,
    role: FontRole,
    current: &str,
    families: &'a [String],
    editable: bool,
) -> Element<'a, Msg> {
    // On the read-only Default theme the picker would fire a SetFont
    // that's dropped on the floor and snap back — show the family as
    // plain text instead, matching the read-only atom swatches.
    let control: Element<'a, Msg> = if editable {
        let selected = Some(current.to_string());
        pick_list(families, selected, move |family| Msg::SetFont(role, family))
            .width(Length::Fixed(220.0))
            .into()
    } else {
        container(body(current.to_string()))
            .width(Length::Fixed(220.0))
            .into()
    };

    row![
        container(body(role_label)).width(Length::Fixed(120.0)),
        control,
    ]
    .spacing(16)
    .align_y(iced::Alignment::Center)
    .into()
}

fn atom_grid<'a>(
    atoms: &'a Atoms,
    editable: bool,
    editing: Option<AtomField>,
    picker: Option<&'a ColorPicker>,
) -> Element<'a, Msg> {
    let rows: &[(&str, AtomField, &str)] = &[
        ("BG",        AtomField::Bg,       "background.base / weakest"),
        ("BG_RAISED", AtomField::BgRaised, "background.weaker / weak"),
        ("BG_HOVER",  AtomField::BgHover,  "background.neutral / strong"),
        ("BORDER",    AtomField::Border,   "background.stronger / strongest"),
        ("FG",        AtomField::Fg,       "palette.text"),
        ("FG_MUTED",  AtomField::FgMuted,  "secondary.base.text"),
        ("ACCENT",    AtomField::Accent,   "primary.base"),
        ("SUCCESS",   AtomField::Success,  "success.base"),
        ("WARNING",   AtomField::Warning,  "warning.base"),
        ("DANGER",    AtomField::Danger,   "danger.base"),
    ];

    let mut grid = rows.chunks(5).fold(column![].spacing(GRID_GAP), |col, chunk| {
        let r = chunk.iter().fold(row![].spacing(GRID_GAP), |r, (name, field, slot)| {
            r.push(swatch_tile(atoms, name, *field, slot, editable, editing))
        });
        col.push(r)
    });

    // Editing an atom opens its picker inline below the grid.
    if editable {
        if let Some(p) = picker {
            grid = grid.push(picker_panel(p));
        }
    }
    grid.into()
}

fn swatch_tile<'a>(
    atoms: &Atoms,
    name: &'a str,
    field: AtomField,
    slot: &'a str,
    editable: bool,
    editing: Option<AtomField>,
) -> Element<'a, Msg> {
    let color = field.get(atoms);
    let tile = swatch_sized::<Msg>(color, SWATCH_SIZE);
    // When editable, the swatch is a click target that opens its picker;
    // the open atom gets an accent ring so the picker's subject is clear.
    let tile: Element<'a, Msg> = if editable {
        let selected = editing == Some(field);
        let ring = if selected { 2.0 } else { 0.0 };
        let framed = container(tile)
            .padding(Padding::from(ring))
            .style(move |theme: &iced::Theme| {
                let p = theme.extended_palette();
                iced::widget::container::Style {
                    border: iced::Border {
                        color: if selected { p.primary.base.color } else { Color::TRANSPARENT },
                        width: ring,
                        radius: 8.0.into(),
                    },
                    ..iced::widget::container::Style::default()
                }
            });
        mouse_area(framed).on_press(Msg::EditAtom(field)).into()
    } else {
        tile
    };

    column![
        tile,
        body(name),
        code(theme::color_to_hex(color)).style(muted),
        caption(slot).style(muted),
    ]
    .spacing(6)
    .width(Length::Fixed(SWATCH_SIZE + 16.0))
    .into()
}

/// Inline HSV/hex picker for the atom currently being edited.
fn picker_panel(picker: &ColorPicker) -> Element<'_, Msg> {
    column![
        caption("Editing — drag a channel or type a hex; click the swatch again to close")
            .style(muted),
        picker.view().map(Msg::Picker),
    ]
    .spacing(8)
    .into()
}

fn colors_eq(a: Color, b: Color) -> bool {
    let eps = 1.0 / 255.0;
    (a.r - b.r).abs() < eps
        && (a.g - b.g).abs() < eps
        && (a.b - b.b).abs() < eps
}

