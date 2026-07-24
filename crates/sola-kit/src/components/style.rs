//! Shared component style primitives — the bits that recurred across
//! the per-component style fns before they were factored out here.
//!
//! - [`filled`] builds the four-state `button::Style` shared by the
//!   `primary` and `danger` buttons (they differ only by palette tier).
//! - [`hairline`] / [`hairline_strong`] build soft white@α borders
//!   (card / popover / swatch / text_input).
//! - [`dim`] is the shared disabled treatment (halve every alpha).
//! - the `RADIUS_*` / `SPACE_*` constants name the radii and spacing
//!   steps the kit uses, so component code stops sprinkling bare
//!   `6.0.into()` / `padding(16)` literals.

use iced::theme::palette::{Extended, Pair};
use iced::widget::button;
use iced::{Background, Border, Color, Shadow, Vector};

/// Corner radii (sola-kit-ds graphite pass). `SM` = inputs / ghost chrome,
/// `MD` = buttons / swatches, `LG` = cards / popovers, `XL` = large
/// floating panels, `PILL` = fully-rounded badges.
pub const RADIUS_SM: f32 = 5.0;
pub const RADIUS_MD: f32 = 7.0;
pub const RADIUS_LG: f32 = 10.0;
/// `XL` = large floating panels (modal launcher, accent switcher frame).
pub const RADIUS_XL: f32 = 14.0;
pub const RADIUS_PILL: f32 = 999.0;

/// Spacing / padding steps. Kit layouts step through these rather than
/// scattering raw pixel counts; asymmetric paddings still pass explicit
/// `Padding::from([..])` since they don't fall on a single scale.
pub const SPACE_XS: f32 = 2.0;
pub const SPACE_SM: f32 = 4.0;
pub const SPACE_MD: f32 = 8.0;
pub const SPACE_LG: f32 = 12.0;
pub const SPACE_XL: f32 = 16.0;

/// Regular control content padding `[vertical, horizontal]` — buttons,
/// default actions. Prefer this over inventing pad literals in apps.
pub const PAD_CONTROL: [u16; 2] = [7, 14];
/// Compact control padding — toolbar, steppers, dense chrome.
pub const PAD_CONTROL_SM: [u16; 2] = [5, 11];

/// Soft hairline — white @ 7% alpha (OD `--hairline`). Prefer this over
/// the solid `border` atom for quiet chrome edges.
pub const HAIRLINE_A: f32 = 0.07;
/// Stronger hairline — white @ 12% (OD `--hairline-strong`).
pub const HAIRLINE_STRONG_A: f32 = 0.12;

/// Soft white@α hairline at the given corner radius.
pub fn hairline(_palette: &Extended, radius: f32) -> Border {
    Border {
        color: Color::from_rgba(1.0, 1.0, 1.0, HAIRLINE_A),
        width: 1.0,
        radius: radius.into(),
    }
}

/// Stronger white@α hairline (secondary buttons, focused fields at rest).
pub fn hairline_strong(radius: f32) -> Border {
    Border {
        color: Color::from_rgba(1.0, 1.0, 1.0, HAIRLINE_STRONG_A),
        width: 1.0,
        radius: radius.into(),
    }
}

/// Mix `color` toward transparent at `alpha` (0..1). Used for soft
/// badge / secondary fills that tint without opaque slabs.
pub fn alpha(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha,
        ..color
    }
}

/// Darken `color` toward black by `amount` (0..1) — stands in for
/// `color-mix(in srgb, #000 amount%, color)` (inset field wells).
pub fn inset_surface(color: Color, amount: f32) -> Color {
    let keep = 1.0 - amount;
    Color {
        r: color.r * keep,
        g: color.g * keep,
        b: color.b * keep,
        a: color.a,
    }
}

/// Dark label on bright filled primaries / dangers (`#041018` / near-black).
pub const ON_FILL_DARK: Color = Color {
    r: 0.016,
    g: 0.063,
    b: 0.094,
    a: 1.0,
};

/// Soft accent glow under primary actions (OD `--glow-accent` approx).
pub fn accent_glow(accent: Color) -> Shadow {
    Shadow {
        color: alpha(accent, 0.35),
        offset: Vector::new(0.0, 4.0),
        blur_radius: 14.0,
    }
}

pub fn filled(
    base: Pair,
    strong: Pair,
    weak: Pair,
    status: button::Status,
) -> button::Style {
    filled_with(base, strong, weak, status, base.text, None)
}

/// Like [`filled`], with explicit label colour and optional resting glow.
pub fn filled_with(
    base: Pair,
    strong: Pair,
    weak: Pair,
    status: button::Status,
    text: Color,
    glow: Option<Shadow>,
) -> button::Style {
    let resting = button::Style {
        background: Some(Background::Color(base.color)),
        text_color: text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        shadow: glow.unwrap_or_default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(strong.color)),
            shadow: glow.unwrap_or_default(),
            ..resting
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(weak.color)),
            shadow: Default::default(),
            ..resting
        },
        button::Status::Disabled => dim(button::Style {
            shadow: Default::default(),
            ..resting
        }),
        button::Status::Active => resting,
    }
}

pub fn dim(base: button::Style) -> button::Style {
    button::Style {
        background: base.background.map(|bg| match bg {
            Background::Color(c) => Background::Color(Color { a: c.a * 0.5, ..c }),
            other => other,
        }),
        text_color: Color {
            a: base.text_color.a * 0.5,
            ..base.text_color
        },
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn pair(bg: f32, text: f32) -> Pair {
        Pair::new(Color::from_rgb(bg, bg, bg), Color::from_rgb(text, text, text))
    }

    #[test]
    fn filled_active_uses_base_color_and_text() {
        let base = pair(0.1, 0.9);
        let s = filled(base, pair(0.2, 0.9), pair(0.05, 0.9), button::Status::Active);
        assert_eq!(s.background, Some(Background::Color(base.color)));
        assert_eq!(s.text_color, base.text);
    }

    #[test]
    fn filled_hover_uses_strong_bg_but_keeps_base_text() {
        let base = pair(0.1, 0.9);
        let strong = pair(0.2, 0.3);
        let s = filled(base, strong, pair(0.05, 0.9), button::Status::Hovered);
        assert_eq!(s.background, Some(Background::Color(strong.color)));
        // Text stays the base pair's text — hover only lifts the fill.
        assert_eq!(s.text_color, base.text);
    }

    #[test]
    fn filled_pressed_uses_weak_bg() {
        let weak = pair(0.05, 0.9);
        let s = filled(pair(0.1, 0.9), pair(0.2, 0.9), weak, button::Status::Pressed);
        assert_eq!(s.background, Some(Background::Color(weak.color)));
    }

    #[test]
    fn filled_disabled_halves_alpha() {
        let base = Pair::new(
            Color::from_rgba(0.1, 0.1, 0.1, 1.0),
            Color::from_rgba(0.9, 0.9, 0.9, 1.0),
        );
        let s = filled(base, base, base, button::Status::Disabled);
        let Some(Background::Color(bg)) = s.background else {
            panic!("disabled style lost its background colour");
        };
        assert!((bg.a - 0.5).abs() < 1e-6, "bg alpha not halved: {}", bg.a);
        assert!(
            (s.text_color.a - 0.5).abs() < 1e-6,
            "text alpha not halved: {}",
            s.text_color.a
        );
    }

    #[test]
    fn hairline_uses_soft_white_alpha_and_unit_width() {
        let t = theme::default_theme();
        let ext = t.extended_palette();
        let b = hairline(ext, RADIUS_LG);
        assert!((b.color.a - HAIRLINE_A).abs() < 1e-6);
        assert!((b.color.r - 1.0).abs() < 1e-6);
        assert_eq!(b.width, 1.0);
        assert_eq!(b.radius, RADIUS_LG.into());
    }

    #[test]
    fn radii_match_graphite_ds() {
        assert_eq!(RADIUS_SM, 5.0);
        assert_eq!(RADIUS_MD, 7.0);
        assert_eq!(RADIUS_LG, 10.0);
    }
}
