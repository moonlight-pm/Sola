//! Kit-styled text input — slightly chunkier border-radius and palette
//! mapping that biases toward the kit's BG/BG_RAISED hierarchy.
//!
//! Apps build inputs with iced's standard `iced::widget::text_input`
//! and pass `.style(sola_kit::components::text_input::style)`.

use iced::widget::text_input;
use iced::{Background, Border, Theme};

/// Single style fn covering all four `text_input::Status` variants.
/// Resting state has the canvas BG; focused lifts the border to the
/// accent; disabled drops to the muted-text + weak-bg pair.
pub fn style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = theme.extended_palette();
    let active = text_input::Style {
        background: Background::Color(p.background.base.color),
        border: Border {
            radius: 6.0.into(),
            width: 1.0,
            color: p.background.stronger.color,
        },
        icon: p.secondary.base.text,
        placeholder: p.secondary.base.text,
        value: p.background.base.text,
        selection: p.primary.weak.color,
    };
    match status {
        text_input::Status::Active => active,
        text_input::Status::Hovered => text_input::Style {
            border: Border {
                color: p.background.strongest.color,
                ..active.border
            },
            ..active
        },
        text_input::Status::Focused { .. } => text_input::Style {
            border: Border {
                color: p.primary.base.color,
                width: 1.0,
                ..active.border
            },
            ..active
        },
        text_input::Status::Disabled => text_input::Style {
            background: Background::Color(p.background.weak.color),
            value: active.placeholder,
            ..active
        },
    }
}
