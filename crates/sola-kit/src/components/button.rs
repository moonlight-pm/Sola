//! Named button style fns — `primary`, `secondary`, `ghost`, `danger`.
//!
//! Apps build buttons with iced's standard `iced::widget::button(label)`
//! and pass a kit style fn:
//!
//! ```ignore
//! use iced::widget::button;
//! use sola_kit::components::button as kit_btn;
//!
//! button("Save").style(kit_btn::primary).on_press(Msg::Save)
//! ```
//!
//! The kit's `toolbar` button (with its condensed-bold label and
//! padded shape) lives in [`crate::components::toolbar`] alongside its
//! style fn — keep visually-distinct chrome together with its widget.
//!
//! Each style fn is shaped the same as iced's own `button::primary`:
//! `fn(&Theme, Status) -> button::Style`. They cover the four
//! interaction states (Active / Hovered / Pressed / Disabled) by
//! deriving from kit palette tiers.

use iced::widget::button;
use iced::{Background, Border, Color, Theme};

/// Filled accent button — the "OK"/"Save"/"Apply" affordance. Bg is
/// `primary.base`, hover lifts to `primary.strong`, pressed sinks to
/// `primary.weak`. Disabled drops opacity instead of changing color
/// so the user reads it as the same action.
pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(p.primary.base.color)),
        text_color: p.primary.base.text,
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
        shadow: Default::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(p.primary.strong.color)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(p.primary.weak.color)),
            ..base
        },
        button::Status::Disabled => disabled(base),
        button::Status::Active => base,
    }
}

/// Outlined / chromeless secondary — for complementary actions like
/// "Cancel" next to a primary. Border is the kit hairline, fill is
/// transparent at rest and lifts to BG_HOVER on hover.
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: p.background.base.text,
        border: Border {
            color: p.background.stronger.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: Default::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(p.background.strong.color)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(p.background.weak.color)),
            ..base
        },
        button::Status::Disabled => disabled(base),
        button::Status::Active => base,
    }
}

/// Chromeless ghost — no fill, no border, just a text label. For
/// in-line links, breadcrumb segments, and tertiary actions. Hover
/// reveals a subtle background, otherwise reads as plain text.
pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: p.background.base.text,
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 4.0.into() },
        shadow: Default::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(p.background.strong.color)),
            text_color: p.primary.base.color,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(p.background.stronger.color)),
            text_color: p.primary.base.color,
            ..base
        },
        button::Status::Disabled => disabled(base),
        button::Status::Active => base,
    }
}

/// Destructive action — "Delete", "Reset". Same shape as `primary`
/// but on the danger atom so the affordance signals risk.
pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(p.danger.base.color)),
        text_color: p.danger.base.text,
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
        shadow: Default::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(p.danger.strong.color)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(p.danger.weak.color)),
            ..base
        },
        button::Status::Disabled => disabled(base),
        button::Status::Active => base,
    }
}

/// Disable shading shared across variants. Halves the alpha of every
/// color so disabled buttons read as "still this action, just not
/// available right now" rather than a different button entirely.
fn disabled(base: button::Style) -> button::Style {
    button::Style {
        background: base.background.map(|bg| match bg {
            Background::Color(c) => Background::Color(Color { a: c.a * 0.5, ..c }),
            other => other,
        }),
        text_color: Color { a: base.text_color.a * 0.5, ..base.text_color },
        ..base
    }
}
