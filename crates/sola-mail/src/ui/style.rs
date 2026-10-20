//! Container / input styles for the mail panes.

use iced::widget::{container, text_editor};
use iced::{Background, Border, Color, Shadow, Theme, Vector};
use sola_kit::components::style::{HAIRLINE_A, RADIUS_MD, mix_white};

/// Window canvas, list pane and toolbars.
pub fn canvas(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.extended_palette().background.base.color)),
        ..container::Style::default()
    }
}

/// Reading / compose pane: a slight raise over the list canvas.
pub fn read_pane(theme: &Theme) -> container::Style {
    let base = theme.extended_palette().background.base.color;
    container::Style {
        background: Some(Background::Color(mix_white(base, 0.02))),
        ..container::Style::default()
    }
}

pub fn hairline(theme: &Theme) -> container::Style {
    let base = theme.extended_palette().background.base.color;
    container::Style {
        background: Some(Background::Color(mix_white(base, HAIRLINE_A))),
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
    let fill = mix_white(theme.extended_palette().background.base.color, 0.04);
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

/// Borderless compose header field (From / To / Cc / Subject).
pub fn compose_header_input(
    theme: &Theme,
    _status: sola_kit::components::text_input::Status,
) -> sola_kit::components::text_input::Style {
    let p = theme.extended_palette();
    sola_kit::components::text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
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
