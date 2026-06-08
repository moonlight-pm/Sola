//! Shell page — editor for sola-shell's customizable chrome: the
//! shell-* color tokens (alpha-capable) and the switcher/launcher
//! spacing knobs. Edits route through the same preset machinery as the
//! Theme page (mutate active preset → broadcast Topic::Theme →
//! persist), so the running shell restyles as you drag.

use iced::widget::{column, container, mouse_area, row};
use iced::{Color, Element, Length, Padding};

use sola_kit::components::number_input;
use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted, subheading};
use sola_kit::components::{popover, popover_anchored};
use sola_kit::theme::{self, ShellStyle};

use crate::storybook::{Msg, ShellColorField, ShellSpaceField};

const SWATCH_SIZE: f32 = 56.0;
const GRID_GAP: f32 = 44.0;

pub fn view<'a>(
    shell: &'a ShellStyle,
    editable: bool,
    editing: Option<ShellColorField>,
    mut picker_view: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let intro: Element<'a, Msg> = if editable {
        body(
            "Live editor for sola-shell's chrome. Colors carry alpha — \
             the switcher backplate fill is translucent by design. Edits \
             re-emit Topic::Theme, so the running shell restyles \
             immediately.",
        )
        .style(muted)
        .into()
    } else {
        body(
            "Default theme — read-only. Click \"New Theme\" in the header \
             above to fork it under a new name; edits then route to that \
             copy.",
        )
        .style(muted)
        .into()
    };

    let colors: &[(&str, ShellColorField, &str)] = &[
        ("MENUBAR_BG", ShellColorField::MenubarBg, "shell-menubar-bg"),
        ("BACKDROP", ShellColorField::BackdropDim, "shell-backdrop-dim"),
        ("SWITCHER_BG", ShellColorField::SwitcherBg, "shell-switcher-bg"),
        ("SWITCHER_BORDER", ShellColorField::SwitcherBorder, "shell-switcher-border"),
    ];
    let mut color_row = row![].spacing(GRID_GAP);
    for (name, field, token) in colors {
        let picker = if editing == Some(*field) { picker_view.take() } else { None };
        color_row = color_row.push(swatch_tile(shell, name, *field, token, editable, editing, picker));
    }

    column![
        heading("Shell"),
        intro,

        subheading("Colors"),
        body(
            "Click a swatch to edit. The picker's alpha rail is live — \
             e.g. drag SWITCHER_BG's alpha to retune the backplate \
             translucency.",
        )
        .style(muted),
        color_row,

        subheading("Switcher"),
        space_row("Backplate padding", ShellSpaceField::SwitcherPad, shell.switcher_pad, 0.0..=64.0, 2.0, editable),
        space_row("Tile padding", ShellSpaceField::SwitcherTilePad, shell.switcher_tile_pad, 0.0..=48.0, 2.0, editable),

        subheading("Launcher"),
        space_row("Card width", ShellSpaceField::LauncherWidth, shell.launcher_width, 320.0..=1280.0, 20.0, editable),
        space_row("Row padding", ShellSpaceField::LauncherPad, shell.launcher_pad, 0.0..=32.0, 2.0, editable),
    ]
    .spacing(28)
    .into()
}

/// One color knob: swatch (click target + accent ring while editing,
/// anchored picker popover) over label / hex / token captions. Mirrors
/// the Theme page's `swatch_tile` with `ShellColorField` in place of
/// `AtomField`.
fn swatch_tile<'a>(
    shell: &ShellStyle,
    name: &'a str,
    field: ShellColorField,
    token: &'a str,
    editable: bool,
    editing: Option<ShellColorField>,
    picker: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let color = field.get(shell);
    let tile = swatch_sized::<Msg>(color, SWATCH_SIZE);
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
        let trigger = mouse_area(framed).on_press(Msg::EditShellColor(field));
        match picker {
            Some(view) => popover_anchored(trigger, popover(view), Msg::ClosePicker).into(),
            None => trigger.into(),
        }
    } else {
        tile
    };

    column![
        tile,
        body(name),
        code(theme::color_to_hex(color)).style(muted),
        caption(token).style(muted),
    ]
    .spacing(6)
    .width(Length::Fixed(SWATCH_SIZE + 36.0))
    .into()
}

/// One spacing knob: label + `[−] value px [+]` stepper (plain text on
/// the read-only Default preset).
fn space_row<'a>(
    label: &'a str,
    field: ShellSpaceField,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    editable: bool,
) -> Element<'a, Msg> {
    let control: Element<'a, Msg> = if editable {
        number_input(value, range, step, "px", move |v| Msg::SetShellSpace(field, v))
    } else {
        container(body(format!("{value:.0} px"))).into()
    };
    row![
        container(body(label)).width(Length::Fixed(160.0)),
        control,
    ]
    .spacing(16)
    .align_y(iced::Alignment::Center)
    .into()
}
