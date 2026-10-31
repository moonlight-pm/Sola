//! Container, button and field styles for Vault's panes, drawn as the
//! canvas (`design/vault`): the list on the raised fill, the record on the
//! window's face with Settings' grouped rows, plates in enamel. Every
//! colour comes from the theme or a kit derivation.
//!
//! A tile's theme clears `background.base` (the frame paints the face),
//! so the face colour itself is `palette().background`.

use iced::widget::{button, container, text_editor};
use iced::{Background, Border, Color, Shadow, Theme, Vector};
use sola_kit::components::style::{
    HAIRLINE_A, RADIUS_LG, RADIUS_MD, RADIUS_SM, alpha, card_fill, hairline, mix, mix_white,
};
use sola_kit::components::text_input::{Status as InputStatus, Style as InputStyle};

/// The window's face (the record, sign-in).
pub fn face_color(theme: &Theme) -> Color {
    theme.palette().background
}

/// The raised fill (the list).
pub fn raised_color(theme: &Theme) -> Color {
    theme.extended_palette().background.weaker.color
}

/// Secondary ink: labels, tools.
pub fn ink2(theme: &Theme) -> Color {
    theme.extended_palette().secondary.base.text
}

/// Tertiary ink: marks after a name, the record's foot.
pub fn ink3(theme: &Theme) -> Color {
    alpha(ink2(theme), 0.78)
}

pub fn canvas(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.base.color,
        )),
        ..container::Style::default()
    }
}

pub fn raised(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(raised_color(theme))),
        ..container::Style::default()
    }
}

pub fn hairline_fill(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(mix_white(raised_color(theme), HAIRLINE_A))),
        ..container::Style::default()
    }
}

/// Settings' group: rows on one raised surface.
pub fn group(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(card_fill(p.background.weaker.color)),
        border: hairline(p, RADIUS_LG),
        ..container::Style::default()
    }
}

/// The rule between two rows of a group.
pub fn group_rule(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(mix_white(raised_color(theme), 0.06))),
        ..container::Style::default()
    }
}

/// A list row: the chosen one on the selection grey, hover a lift.
pub fn list_row(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        let fill = match (selected, status) {
            (true, _) => Some(sola_kit::theme::selection()),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                Some(mix_white(raised_color(theme), 0.05))
            }
            _ => None,
        };
        button::Style {
            background: fill.map(Background::Color),
            text_color: p.background.base.text,
            border: Border {
                radius: RADIUS_MD.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// A small tool: ink at rest, a quiet lift under the pointer.
pub fn tool(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let fill = match status {
        button::Status::Hovered => Some(alpha(p.background.strong.color, 0.7)),
        button::Status::Pressed => Some(p.background.strong.color),
        _ => None,
    };
    button::Style {
        background: fill.map(Background::Color),
        text_color: ink2(theme),
        border: Border {
            radius: RADIUS_SM.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// A tool that is on (a toggled reveal, an open menu).
pub fn tool_on(theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(sola_kit::theme::selection())),
        text_color: theme.extended_palette().background.base.text,
        border: Border {
            radius: RADIUS_SM.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// A link in running text ("Use another account").
pub fn link(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let text = match status {
        button::Status::Hovered | button::Status::Pressed => p.background.base.text,
        _ => mix(p.background.base.text, ink2(theme), 0.6),
    };
    button::Style {
        background: None,
        text_color: text,
        ..button::Style::default()
    }
}

/// An enamel plate with its letter in the face's ink.
pub fn plate(fill: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: Some(Background::Color(fill)),
        text_color: Some(face_color(theme)),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The grey plate under a kind's glyph.
pub fn kind_plate(radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let fill = theme.extended_palette().background.strong.color;
        container::Style {
            background: Some(Background::Color(fill)),
            text_color: Some(ink2(theme)),
            border: Border {
                color: mix_white(fill, HAIRLINE_A),
                width: 1.0,
                radius: radius.into(),
            },
            ..container::Style::default()
        }
    }
}

/// A message across the list (a failed sync): the warning's soft fill.
pub fn warn_box(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    let w = p.warning.base.color;
    container::Style {
        background: Some(Background::Color(mix(w, raised, 0.14))),
        border: Border {
            color: mix(w, raised, 0.28),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

/// A message in the danger's soft fill (a save that failed).
pub fn danger_box(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    let d = p.danger.base.color;
    container::Style {
        background: Some(Background::Color(mix(d, raised, 0.14))),
        border: Border {
            color: mix(d, raised, 0.28),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

/// The floating toast.
pub fn toast(theme: &Theme) -> container::Style {
    let fill = raised_color(theme);
    container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            color: mix_white(fill, HAIRLINE_A),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.28),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 10.0,
        },
        ..container::Style::default()
    }
}

/// The dim under a sheet.
pub fn dim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.48,
            ..Color::BLACK
        })),
        ..container::Style::default()
    }
}

/// The changed dot by an edited field's label.
pub fn changed_dot(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ink2(theme))),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The kit field with a danger edge (an error under it).
pub fn input_error(theme: &Theme, status: InputStatus) -> InputStyle {
    let mut s = sola_kit::components::text_input::style(theme, status);
    s.border.color = theme.extended_palette().danger.base.color;
    s
}

/// A box of the authenticator code: the field's well, the next box with
/// the focus edge.
pub fn code_box(theme: &Theme, next: bool, error: bool) -> container::Style {
    let status = if next {
        InputStatus::Focused { is_hovered: false }
    } else {
        InputStatus::Active
    };
    let s = if error {
        input_error(theme, status)
    } else {
        sola_kit::components::text_input::style(theme, status)
    };
    container::Style {
        background: Some(s.background),
        border: s.border,
        text_color: Some(s.value),
        ..container::Style::default()
    }
}

/// The field under the code's boxes: it takes the typing, the boxes show it.
pub fn invisible_input(_theme: &Theme, _status: InputStatus) -> InputStyle {
    InputStyle {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: Color::TRANSPARENT,
        placeholder: Color::TRANSPARENT,
        value: Color::TRANSPARENT,
        selection: Color::TRANSPARENT,
    }
}

/// The list's search: a well on the raised fill.
pub fn search_input(theme: &Theme, status: InputStatus) -> InputStyle {
    let p = theme.extended_palette();
    let raised = raised_color(theme);
    let edge = match status {
        InputStatus::Focused { .. } => mix(p.primary.base.color, raised, 0.55),
        _ => mix_white(raised, HAIRLINE_A),
    };
    InputStyle {
        background: Background::Color(mix_white(raised, 0.04)),
        border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        icon: ink2(theme),
        placeholder: alpha(ink2(theme), 0.75),
        value: p.background.base.text,
        selection: sola_kit::theme::selection(),
    }
}

/// Notes in the edit form: the kit field's well.
pub fn notes_editor(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let input = match status {
        text_editor::Status::Focused { is_hovered } => InputStatus::Focused { is_hovered },
        text_editor::Status::Hovered => InputStatus::Hovered,
        text_editor::Status::Disabled => InputStatus::Disabled,
        text_editor::Status::Active => InputStatus::Active,
    };
    let s = sola_kit::components::text_input::style(theme, input);
    text_editor::Style {
        background: s.background,
        border: s.border,
        placeholder: s.placeholder,
        value: s.value,
        selection: s.selection,
    }
}
