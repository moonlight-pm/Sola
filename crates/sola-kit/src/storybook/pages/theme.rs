//! Theme page — per-role font selectors and a swatch grid of every atom
//! in `sola_kit::theme::Atoms`. Clicking a swatch opens the colour
//! picker (floated as a popover by `Storybook::view`). Edits emit
//! `Msg::EditAtom` / `Msg::Picker` or `Msg::SetFont`; the storybook update rebuilds
//! its iced theme, calls `sola_kit::fonts::install`, and re-emits
//! `Topic::Theme` so every kit consumer picks up the change.

use iced::widget::{column, container, mouse_area, pick_list, row};
use iced::{Color, Element, Length, Padding};

use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted, subheading};
use sola_kit::components::{popover, popover_anchored};
use sola_kit::theme::{self, Atoms, FontSelection};

use crate::storybook::{AtomField, FontRole, Msg};

const SWATCH_SIZE: f32 = 56.0;
const GRID_GAP: f32 = 44.0;

pub fn view<'a>(
    atoms: &'a Atoms,
    fonts: &'a FontSelection,
    families: &'a [String],
    editable: bool,
    editing: Option<AtomField>,
    picker_view: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let intro: Element<'a, Msg> = if editable {
        body("Fonts and seed atoms. Edits ride the bus to every kit app.")
            .style(muted)
            .into()
    } else {
        body("Default is read-only. New Theme in the header forks it.")
            .style(muted)
            .into()
    };

    column![
        heading("Theme"),
        intro,
        subheading("Fonts"),
        fonts_grid(fonts, families, editable),
        subheading("Palette"),
        body(if editable {
            "Click a swatch. Neon stays neon — don't mix it toward black."
        } else {
            "Read-only on Default. Fork to edit."
        })
        .style(muted),
        atom_grid(atoms, editable, editing, picker_view),
    ]
    .spacing(28)
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

/// Every atom, in the order the Theme page's full grid renders them.
const ALL_ATOM_FIELDS: &[AtomField] = &[
    AtomField::Bg,
    AtomField::BgRaised,
    AtomField::BgHover,
    AtomField::Border,
    AtomField::Fg,
    AtomField::FgMuted,
    AtomField::Accent,
    AtomField::Success,
    AtomField::Warning,
    AtomField::Danger,
    AtomField::Selection,
];

/// Display metadata for one atom: its `(LABEL, palette-slot)` caption.
/// `selection` has no iced slot — it's delivered process-wide — so its
/// "slot" names the thing it actually drives.
fn atom_meta(field: AtomField) -> (&'static str, &'static str) {
    match field {
        AtomField::Bg => ("BG", "background.base / weakest"),
        AtomField::BgRaised => ("BG_RAISED", "background.weaker / weak"),
        AtomField::BgHover => ("BG_HOVER", "background.neutral / strong"),
        AtomField::Border => ("BORDER", "background.stronger / strongest"),
        AtomField::Fg => ("FG", "palette.text"),
        AtomField::FgMuted => ("FG_MUTED", "secondary.base.text"),
        AtomField::Accent => ("ACCENT", "primary.base"),
        AtomField::Success => ("SUCCESS", "success.base"),
        AtomField::Warning => ("WARNING", "warning.base"),
        AtomField::Danger => ("DANGER", "danger.base"),
        AtomField::Selection => ("SELECTION", "selected-row highlight"),
    }
}

/// Lay out the swatches for an arbitrary set of atoms, five per row,
/// handing the single open picker to whichever tile is being edited.
/// Shared by the Theme page's full grid and the per-component panels.
fn swatch_flow<'a>(
    atoms: &'a Atoms,
    fields: &[AtomField],
    editable: bool,
    editing: Option<AtomField>,
    mut picker_view: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let mut col = column![].spacing(GRID_GAP);
    for chunk in fields.chunks(5) {
        let mut r = row![].spacing(GRID_GAP);
        for field in chunk {
            let (name, slot) = atom_meta(*field);
            let picker = if editing == Some(*field) { picker_view.take() } else { None };
            r = r.push(swatch_tile(atoms, name, *field, slot, editable, editing, picker));
        }
        col = col.push(r);
    }
    col.into()
}

/// Contextual atom editor for a component page — the swatches for just
/// the atoms that component uses ([`crate::storybook::Page::atoms`]),
/// rendered below its demo. Same swatch/picker mechanics as the Theme
/// page's full grid; the global Save/Revert in the header commit or
/// discard the shared working set. Renders nothing for an empty set.
pub fn atom_panel<'a>(
    atoms: &'a Atoms,
    fields: &'a [AtomField],
    editable: bool,
    editing: Option<AtomField>,
    picker_view: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    if fields.is_empty() {
        return column![].into();
    }
    let note = if editable {
        "This page's atoms. Click a swatch. Save lives in the header."
    } else {
        "This page's atoms. Default is read-only — New Theme to edit."
    };
    column![
        subheading("Atoms"),
        body(note).style(muted),
        swatch_flow(atoms, fields, editable, editing, picker_view),
    ]
    .spacing(12)
    .into()
}

fn atom_grid<'a>(
    atoms: &'a Atoms,
    editable: bool,
    editing: Option<AtomField>,
    picker_view: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    swatch_flow(atoms, ALL_ATOM_FIELDS, editable, editing, picker_view)
}

fn swatch_tile<'a>(
    atoms: &Atoms,
    name: &'a str,
    field: AtomField,
    slot: &'a str,
    editable: bool,
    editing: Option<AtomField>,
    picker: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let color = field.get(atoms);
    let tile = swatch_sized::<Msg>(color, SWATCH_SIZE);
    // When editable, the swatch is a click target that opens its picker;
    // the open atom gets an accent ring so the picker's subject is clear.
    let tile: Element<'a, Msg> = if editable {
        let selected = editing == Some(field);
        // Always reserve the ring's footprint (constant padding + border
        // width); selecting toggles only the ring COLOR, never the tile's
        // layout size — otherwise the selected swatch grows and shoves the
        // grid below it down.
        const RING: f32 = 2.0;
        let framed = container(tile)
            .padding(Padding::from(RING))
            .style(move |theme: &iced::Theme| {
                let p = theme.extended_palette();
                iced::widget::container::Style {
                    border: iced::Border {
                        color: if selected { p.primary.base.color } else { Color::TRANSPARENT },
                        width: RING,
                        radius: 8.0.into(),
                    },
                    ..iced::widget::container::Style::default()
                }
            });
        let trigger = mouse_area(framed).on_press(Msg::EditAtom(field));
        // While this atom is the one being edited, anchor its colour
        // picker to the swatch as a popover; a click outside dismisses it.
        match picker {
            Some(view) => popover_anchored(trigger, popover(view), Msg::ClosePicker).into(),
            None => trigger.into(),
        }
    } else {
        tile
    };

    // The hex line carries a "reset" link when this atom has drifted from
    // its compile-time default — a surgical, single-atom revert. Hidden
    // on the read-only Default and whenever the value already is default.
    let hex = code(theme::color_to_hex(color)).style(muted);
    let hex_line: Element<'a, Msg> = if editable && color != field.default_color() {
        row![
            hex,
            mouse_area(
                caption("reset").style(|t: &iced::Theme| iced::widget::text::Style {
                    color: Some(t.extended_palette().primary.base.color),
                })
            )
            .on_press(Msg::ResetAtom(field)),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        hex.into()
    };

    column![tile, body(name), hex_line, caption(slot).style(muted)]
        .spacing(6)
        .width(Length::Fixed(SWATCH_SIZE + 16.0))
        .into()
}

