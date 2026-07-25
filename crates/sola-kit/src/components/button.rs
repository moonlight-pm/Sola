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
//! // or density-baked:
//! kit_btn::labeled("Save", kit_btn::primary).on_press(Msg::Save)
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
use iced::widget::text;
use iced::widget::text::IntoFragment;
use iced::{Background, Border, Color, Shadow, Theme};

use crate::components::style::{
    self, HAIRLINE_A, HAIRLINE_STRONG_A, ON_FILL_DARK, PAD_CONTROL, PAD_CONTROL_SM, RADIUS_MD,
    RADIUS_SM, accent_glow, alpha, hairline_on, mix_white,
};
use crate::fonts;

/// Regular labeled button: 13px UI type + [`PAD_CONTROL`].
///
/// Prefer this over hand-rolled `button(text(...)).padding(...)` so
/// apps inherit kit density without inventing pads.
pub fn labeled<'a, Message: Clone + 'a>(
    label: impl IntoFragment<'a>,
    style_fn: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> button::Button<'a, Message> {
    button(text(label).font(fonts::ui()).size(13))
        .padding(PAD_CONTROL)
        .style(style_fn)
}

/// Compact labeled button: 12px UI type + [`PAD_CONTROL_SM`] (toolbar /
/// dense chrome density).
pub fn labeled_sm<'a, Message: Clone + 'a>(
    label: impl IntoFragment<'a>,
    style_fn: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> button::Button<'a, Message> {
    button(text(label).font(fonts::ui()).size(12))
        .padding(PAD_CONTROL_SM)
        .style(style_fn)
}

/// Filled accent action. Keep **one primary per group** — cyan is product
/// identity, but sparse (HIG: accent for the single default action).
/// Dark label + soft glow approximate the OD gradient primary.
pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    style::filled_with(
        p.primary.base,
        p.primary.strong,
        p.primary.weak,
        status,
        ON_FILL_DARK,
        Some(accent_glow(p.primary.base.color)),
    )
}

/// Soft secondary — quiet fill + thin hairline (not a bare outline).
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    // Opaque quiet fill (avoid translucent fill + translucent border stack).
    let fill = mix_white(p.background.weaker.color, 0.04);
    let fill_hover = mix_white(p.background.strong.color, 0.06);
    let base = button::Style {
        background: Some(Background::Color(fill)),
        text_color: p.background.base.text,
        border: hairline_on(fill, HAIRLINE_STRONG_A, RADIUS_MD),
        shadow: Default::default(),
        snap: true,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(fill_hover)),
            border: hairline_on(fill_hover, 0.14, RADIUS_MD),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(p.background.weak.color)),
            border: hairline_on(p.background.weak.color, HAIRLINE_A, RADIUS_MD),
            ..base
        },
        button::Status::Disabled => style::dim(base),
        button::Status::Active => base,
    }
}

/// Ghost — transparent at rest with **muted** text; hover lifts bg and
/// restores full fg so everyday chrome stays calm.
pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let muted = p.secondary.base.text;
    let fg = p.background.base.text;
    // ~35% toward primary label so ghost isn't read as disabled.
    let rest = Color {
        r: muted.r * 0.65 + fg.r * 0.35,
        g: muted.g * 0.65 + fg.g * 0.35,
        b: muted.b * 0.65 + fg.b * 0.35,
        a: 1.0,
    };
    let base = button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: rest,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_SM.into(),
        },
        shadow: Default::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(alpha(p.background.strong.color, 0.70))),
            text_color: fg,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(p.background.strong.color)),
            text_color: fg,
            ..base
        },
        button::Status::Disabled => style::dim(base),
        button::Status::Active => base,
    }
}

/// Filled danger — dark label + vertical gradient (same language as primary).
pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    // Reuse the glow path for gradient fill; shadow stays off (OD danger
    // has inset highlight only, no outer glow).
    let mut s = style::filled_with(
        p.danger.base,
        p.danger.strong,
        p.danger.weak,
        status,
        Color::from_rgb(0.102, 0.024, 0.031), // #1a0608
        Some(Shadow::default()),
    );
    s.shadow = Default::default();
    s
}

/// Soft danger outline — tinted fill + danger border (restrained destructive).
pub fn danger_outline(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let danger = p.danger.base.color;
    let raised = p.background.weaker.color;
    let soft = |amt: f32| Color {
        r: raised.r * (1.0 - amt) + danger.r * amt,
        g: raised.g * (1.0 - amt) + danger.g * amt,
        b: raised.b * (1.0 - amt) + danger.b * amt,
        a: 1.0,
    };
    let fill = soft(0.10);
    let edge = soft(0.42);
    let base = button::Style {
        background: Some(Background::Color(fill)),
        text_color: Color {
            r: danger.r * 0.88 + 0.12,
            g: danger.g * 0.88 + 0.12,
            b: danger.b * 0.88 + 0.12,
            a: 1.0,
        },
        border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        shadow: Default::default(),
        snap: true,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(soft(0.18))),
            border: Border {
                color: soft(0.55),
                width: 1.0,
                radius: RADIUS_MD.into(),
            },
            text_color: p.background.base.text,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(soft(0.28))),
            text_color: p.background.base.text,
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

/// Style for a selectable list-row button (launcher, pickers, …).
///
/// `selected` is owned by the app (keyboard/MRU selection), independent of the
/// pointer `Status`. Selected → quiet [`crate::theme::selection`] fill (not a
/// full accent pill — Spotlight restraint). Unselected → transparent, lifting
/// to `background.strong` on hover/press.
pub fn list_item(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        if selected {
            return button::Style {
                background: Some(Background::Color(crate::theme::selection())),
                text_color: p.background.base.text,
                border: Border {
                    color: alpha(p.primary.base.color, 0.18),
                    width: 1.0,
                    radius: RADIUS_SM.into(),
                },
                shadow: Default::default(),
                snap: false,
            };
        }
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => {
                alpha(p.background.strong.color, 0.70)
            }
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
}

/// Menu-row hover style — list_item calm without selection chrome.
///
/// Transparent at rest; soft hover lift with a small radius so the
/// highlight reads as a compact menu row, not a fat list pill.
pub fn menu_item(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let bg = match status {
        button::Status::Hovered | button::Status::Pressed => {
            alpha(p.background.strong.color, 0.75)
        }
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

/// Style for a menubar label button. `active` = its menu is open (or flash).
///
/// macOS-style: **no hover chrome while idle**. Highlight only when `active`
/// (menu open / shortcut flash). Hover-to-switch while a menu is already open
/// is driven by the shell making the hovered label `active`, not by button
/// Hovered status. Deriving the fill from `background.base.text` keeps it
/// legible on the permanently-black menubar.
pub fn menubar(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, _status| {
        let p = theme.extended_palette();
        let fg = p.background.base.text;
        // Square corners so a full-height active fill meets the top of the
        // bar cleanly (no radius dead band at y=0).
        let bg = if active {
            Color { a: 0.18, ..fg }
        } else {
            Color::TRANSPARENT
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 0.0.into(),
            },
            shadow: Default::default(),
            snap: false,
        }
    }
}
