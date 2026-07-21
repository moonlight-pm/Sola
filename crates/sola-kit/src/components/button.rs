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

use iced::widget::{button, text};
use iced::{Background, Border, Color, Theme};

use crate::components::style::{self, RADIUS_MD, RADIUS_SM};

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    style::filled(p.primary.base, p.primary.strong, p.primary.weak, status)
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: p.background.base.text,
        border: Border {
            color: p.background.stronger.color,
            width: 1.0,
            radius: RADIUS_MD.into(),
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
        button::Status::Disabled => style::dim(base),
        button::Status::Active => base,
    }
}

pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: p.background.base.text,
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_SM.into() },
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
        button::Status::Disabled => style::dim(base),
        button::Status::Active => base,
    }
}

pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    style::filled(p.danger.base, p.danger.strong, p.danger.weak, status)
}

/// Outlined danger — a restrained destructive affordance: transparent
/// fill with a danger-colored border and text at rest, filling in on
/// hover/press. The idle half of [`confirm_button`]; also usable alone
/// for a low-emphasis "Delete" that the caller gates behind a confirm.
pub fn danger_outline(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: p.danger.base.color,
        border: Border {
            color: p.danger.base.color,
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        shadow: Default::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(p.danger.base.color)),
            text_color: p.danger.base.text,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(p.danger.strong.color)),
            text_color: p.danger.strong.text,
            ..base
        },
        button::Status::Disabled => style::dim(base),
        button::Status::Active => base,
    }
}

/// Two-stage confirm button for destructive actions. The kit renders;
/// the **consumer owns the `armed` flag** (and decides when to disarm —
/// on a timeout, on navigation, or after the action commits), because
/// iced widgets hold no internal state.
///
/// - `armed == false`: shows `idle_label` in [`danger_outline`]; a press
///   sends `on_arm` (the consumer flips `armed` on).
/// - `armed == true`: shows `confirm_label` filled via [`danger`]; a
///   press sends `on_confirm` (the consumer performs the action and
///   flips `armed` back off).
///
/// Returns the configured `Button` so the caller can chain `.padding(..)`
/// / `.width(..)`. Pattern:
///
/// ```ignore
/// confirm_button(self.delete_armed, "Delete", "Confirm?",
///     Msg::ArmDelete, Msg::DeleteConfirmed)
///     .padding(Padding::from([6, 14]))
/// ```
pub fn confirm_button<'a, Message: Clone + 'a>(
    armed: bool,
    idle_label: &'a str,
    confirm_label: &'a str,
    on_arm: Message,
    on_confirm: Message,
) -> button::Button<'a, Message> {
    if armed {
        button(text(confirm_label)).style(danger).on_press(on_confirm)
    } else {
        button(text(idle_label)).style(danger_outline).on_press(on_arm)
    }
}

/// Style for a selectable list-row button.
///
/// `selected` is owned by the app (keyboard/MRU selection), independent of the
/// pointer `Status`. Selected → filled `primary` pill. Unselected → transparent,
/// lifting to `background.strong` on hover/press.
pub fn list_item(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        if selected {
            return button::Style {
                background: Some(Background::Color(p.primary.base.color)),
                text_color: p.primary.base.text,
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_MD.into() },
                shadow: Default::default(),
                snap: false,
            };
        }
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => p.background.strong.color,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.background.base.text,
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_MD.into() },
            shadow: Default::default(),
            snap: false,
        }
    }
}

/// Menu-row hover style — list_item calm without selection chrome.
///
/// Transparent at rest; `background.strong` on hover/press with a small
/// radius so the highlight reads as a compact macOS menu row, not a
/// fat list pill.
pub fn menu_item(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let bg = match status {
        button::Status::Hovered | button::Status::Pressed => p.background.strong.color,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.background.base.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_SM.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

/// Style for a menubar label button. `active` = its menu is open.
///
/// Transparent at rest; a translucent fg-tinted highlight on hover or while
/// active. Deriving the highlight from `background.base.text` keeps it legible
/// on the permanently-black menubar (light fg → light highlight) and adapts if
/// the bar colour ever changes.
pub fn menubar(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        let fg = p.background.base.text;
        let bg = if active {
            Color { a: 0.18, ..fg }
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => Color { a: 0.12, ..fg },
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_SM.into() },
            shadow: Default::default(),
            snap: false,
        }
    }
}


