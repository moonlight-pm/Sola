//! Styles for the panes, drawn as Mail's: the list on the raised fill, the
//! thread on the window's face, hairlines between.

use iced::widget::{button, container, text_editor};
use iced::{Background, Border, Color, Theme};
use sola_kit::components::style::{HAIRLINE_A, RADIUS_LG, RADIUS_MD, RADIUS_PILL, mix, mix_white, on_color};
use sola_kit::components::text_input;

/// How much of a bubble's own fill a quote inside it keeps (the rest is black).
const QUOTE_SHADE: f32 = 0.8;
/// How much of someone's colour their name keeps (the rest is white).
const NAME_TINT: f32 = 0.6;

pub fn raised_color(theme: &Theme) -> Color {
    theme.extended_palette().background.weaker.color
}

/// The window's face: transparent in a tile, where the frame paints it.
pub fn canvas(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.extended_palette().background.base.color)),
        ..container::Style::default()
    }
}

pub fn raised(theme: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(raised_color(theme))), ..container::Style::default() }
}

pub fn hairline(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(mix_white(raised_color(theme), HAIRLINE_A))),
        ..container::Style::default()
    }
}

/// A row of the list: the open one lifted, full bleed.
pub fn list_row(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        let lift = p.background.strong.color;
        let fill = match (selected, status) {
            (true, _) => Some(lift),
            (false, button::Status::Hovered | button::Status::Pressed) => Some(mix(lift, raised_color(theme), 0.5)),
            _ => None,
        };
        button::Style {
            background: fill.map(Background::Color),
            text_color: p.background.base.text,
            ..button::Style::default()
        }
    }
}

/// What a bubble is filled with: ours in the accent, theirs lifted.
pub fn bubble_color(theme: &Theme, outgoing: bool) -> Color {
    let p = theme.extended_palette();
    if outgoing { p.primary.base.color } else { p.background.strong.color }
}

pub fn bubble(outgoing: bool, picked: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let fill = bubble_color(theme, outgoing);
        container::Style {
            background: Some(Background::Color(fill)),
            text_color: Some(on_color(fill)),
            border: Border {
                radius: RADIUS_LG.into(),
                width: if picked { 1.5 } else { 0.0 },
                color: on_color(fill),
            },
            ..container::Style::default()
        }
    }
}

/// A bubble is a button so it can be picked; it looks like its container.
pub fn bare(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style::default()
}

/// The quoted message inside a reply: a darker inset with an edge.
pub fn quote(outgoing: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let fill = bubble_color(theme, outgoing);
        container::Style {
            background: Some(Background::Color(mix(fill, Color::BLACK, QUOTE_SHADE))),
            text_color: Some(on_color(fill)),
            border: Border { radius: RADIUS_MD.into(), ..Border::default() },
            ..container::Style::default()
        }
    }
}

/// A reaction under a bubble; ours is edged in the accent.
pub fn reaction(mine: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = theme.extended_palette();
        container::Style {
            background: Some(Background::Color(p.background.strong.color)),
            text_color: Some(p.background.base.text),
            border: Border {
                radius: RADIUS_PILL.into(),
                width: 1.0,
                color: if mine { p.primary.base.color } else { mix_white(raised_color(theme), HAIRLINE_A) },
            },
            ..container::Style::default()
        }
    }
}

/// A line of the client's own in the thread (a timer change, a call).
pub fn note(theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(theme.extended_palette().secondary.base.text),
        ..container::Style::default()
    }
}

/// The picture of someone with none: a disc in a colour of their own.
pub fn avatar(seed: i64) -> impl Fn(&Theme) -> container::Style {
    const HUES: [Color; 8] = [
        Color::from_rgb(0.24, 0.47, 0.85),
        Color::from_rgb(0.80, 0.36, 0.36),
        Color::from_rgb(0.30, 0.62, 0.45),
        Color::from_rgb(0.72, 0.52, 0.22),
        Color::from_rgb(0.56, 0.42, 0.80),
        Color::from_rgb(0.25, 0.60, 0.66),
        Color::from_rgb(0.78, 0.42, 0.62),
        Color::from_rgb(0.46, 0.52, 0.60),
    ];
    move |_theme| container::Style {
        background: Some(Background::Color(HUES[seed.rem_euclid(HUES.len() as i64) as usize])),
        text_color: Some(Color::WHITE),
        border: Border { radius: RADIUS_PILL.into(), ..Border::default() },
        ..container::Style::default()
    }
}

/// The colour of a name over a bubble in a group, the avatar's.
pub fn name_color(seed: i64) -> Color {
    avatar(seed)(&Theme::Dark).background.map_or(Color::WHITE, |fill| match fill {
        Background::Color(color) => mix(color, Color::WHITE, NAME_TINT),
        _ => Color::WHITE,
    })
}

/// A card over the panes (the code to scan, a banner over the composer).
pub fn card(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.strong.color)),
        text_color: Some(p.background.base.text),
        border: Border { radius: RADIUS_LG.into(), width: 1.0, color: mix_white(raised_color(theme), HAIRLINE_A) },
        ..container::Style::default()
    }
}

/// The paper the code to scan is drawn on: white whatever the theme, so a
/// phone reads it.
pub fn paper(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::WHITE)),
        border: Border { radius: RADIUS_MD.into(), ..Border::default() },
        ..container::Style::default()
    }
}

/// The frame around the composer's field.
pub fn field(theme: &Theme) -> container::Style {
    let raised = raised_color(theme);
    container::Style {
        background: Some(Background::Color(mix_white(raised, 0.04))),
        border: Border { radius: RADIUS_LG.into(), width: 1.0, color: mix_white(raised, HAIRLINE_A) },
        ..container::Style::default()
    }
}

pub fn editor(theme: &Theme, _status: text_editor::Status) -> text_editor::Style {
    let p = theme.extended_palette();
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_MD.into() },
        placeholder: Color { a: 0.75, ..p.secondary.base.color },
        value: p.background.base.text,
        selection: mix_white(p.background.weaker.color, 0.16),
    }
}

pub fn search(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = theme.extended_palette();
    let raised = raised_color(theme);
    let edge = match status {
        text_input::Status::Focused { .. } => Color { a: 0.55, ..p.primary.base.color },
        _ => mix_white(raised, HAIRLINE_A),
    };
    text_input::Style {
        background: Background::Color(mix_white(raised, 0.04)),
        border: Border { color: edge, width: 1.0, radius: RADIUS_MD.into() },
        icon: p.secondary.base.text,
        placeholder: Color { a: 0.75, ..p.secondary.base.color },
        value: p.background.base.text,
        selection: mix_white(raised, 0.16),
    }
}
