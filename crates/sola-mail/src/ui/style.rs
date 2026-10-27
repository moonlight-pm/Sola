//! Container / input styles for the mail panes, drawn as Next's mail
//! (`design/next/next.js` `mailBody`): the mailboxes and the list on the
//! raised fill, the letter on the window's face, hairlines between.
//!
//! A tile's theme clears `background.base` (the frame paints the face),
//! so the face here is `palette().background`; mixing into the cleared
//! base would come out near-black.

use iced::widget::{button, container, text_editor};
use iced::{Background, Border, Color, Shadow, Theme, Vector};
use sola_kit::components::style::{HAIRLINE_A, RADIUS_MD, mix, mix_white};

/// The window's face (the letter, compose).
pub fn face_color(theme: &Theme) -> Color {
    theme.palette().background
}

/// The raised fill (mailboxes, the list).
pub fn raised_color(theme: &Theme) -> Color {
    theme.extended_palette().background.weaker.color
}

/// The window's face: transparent in a tile, where the frame paints it.
pub fn canvas(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.base.color,
        )),
        ..container::Style::default()
    }
}

/// Mailboxes and the list.
pub fn raised(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(raised_color(theme))),
        ..container::Style::default()
    }
}

pub fn hairline(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(mix_white(
            raised_color(theme),
            HAIRLINE_A,
        ))),
        ..container::Style::default()
    }
}

/// A list row: the picked one lifted to the hover fill, full bleed.
pub fn list_row(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        let lift = p.background.strong.color;
        let fill = match (selected, status) {
            (true, _) => Some(lift),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                Some(mix(lift, raised_color(theme), 0.5))
            }
            _ => None,
        };
        button::Style {
            background: fill.map(Background::Color),
            text_color: p.background.base.text,
            ..button::Style::default()
        }
    }
}

/// The unread dot.
pub fn dot(theme: &Theme) -> container::Style {
    let c = theme.extended_palette().primary.base.color;
    container::Style {
        background: Some(Background::Color(c)),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Floating card (account status) with a soft drop shadow.
pub fn card(theme: &Theme) -> container::Style {
    let fill = theme.extended_palette().background.weaker.color;
    container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            color: mix_white(fill, HAIRLINE_A),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.22),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

/// Bottom toast / confirm strip.
pub fn strip(theme: &Theme) -> container::Style {
    let fill = theme.extended_palette().background.weaker.color;
    container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            color: mix_white(fill, HAIRLINE_A),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Attachment list well.
pub fn well(theme: &Theme) -> container::Style {
    let fill = mix_white(face_color(theme), 0.04);
    container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            color: mix_white(fill, HAIRLINE_A),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

/// Compose body field.
pub fn compose_field(theme: &Theme) -> container::Style {
    let fill = theme.extended_palette().background.weaker.color;
    container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            color: mix_white(fill, HAIRLINE_A),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

/// Compose header field (From / To / Cc / Subject): flat at rest, a soft
/// well on hover, a well with an accent edge while focused — so it reads
/// as a field, not a label.
pub fn compose_header_input(
    theme: &Theme,
    status: sola_kit::components::text_input::Status,
) -> sola_kit::components::text_input::Style {
    use sola_kit::components::text_input::Status;
    let p = theme.extended_palette();
    let face = face_color(theme);
    let (fill, edge) = match status {
        Status::Focused { .. } => (
            mix_white(face, 0.05),
            Color {
                a: 0.55,
                ..p.primary.base.color
            },
        ),
        Status::Hovered => (mix_white(face, 0.03), Color::TRANSPARENT),
        _ => (Color::TRANSPARENT, Color::TRANSPARENT),
    };
    sola_kit::components::text_input::Style {
        background: Background::Color(fill),
        border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        icon: p.secondary.base.text,
        placeholder: Color {
            a: 0.75,
            ..p.secondary.base.text
        },
        value: p.background.base.text,
        selection: mix_white(p.background.weaker.color, 0.16),
    }
}

/// The list's search field: a well on the raised fill.
pub fn search_input(
    theme: &Theme,
    status: sola_kit::components::text_input::Status,
) -> sola_kit::components::text_input::Style {
    use sola_kit::components::text_input::Status;
    let p = theme.extended_palette();
    let raised = raised_color(theme);
    let edge = match status {
        Status::Focused { .. } => Color {
            a: 0.55,
            ..p.primary.base.color
        },
        _ => mix_white(raised, HAIRLINE_A),
    };
    sola_kit::components::text_input::Style {
        background: Background::Color(mix_white(raised, 0.04)),
        border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        icon: p.secondary.base.text,
        placeholder: Color {
            a: 0.75,
            ..p.secondary.base.text
        },
        value: p.background.base.text,
        selection: mix_white(raised, 0.16),
    }
}

pub fn compose_editor(theme: &Theme, _status: text_editor::Status) -> text_editor::Style {
    let p = theme.extended_palette();
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        placeholder: Color {
            a: 0.75,
            ..p.secondary.base.color
        },
        value: p.background.base.text,
        selection: mix_white(p.background.weaker.color, 0.16),
    }
}
