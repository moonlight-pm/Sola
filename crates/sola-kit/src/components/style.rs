//! Shared component style primitives — the bits that recurred across
//! the per-component style fns before they were factored out here.
//!
//! - [`filled`] builds the four-state `button::Style` shared by the
//!   `primary` and `danger` buttons (they differ only by palette tier).
//! - [`hairline`] builds the 1px border that card / popover / swatch /
//!   text_input all draw.
//! - [`dim`] is the shared disabled treatment (halve every alpha).
//! - the `RADIUS_*` / `SPACE_*` constants name the radii and spacing
//!   steps the kit uses, so component code stops sprinkling bare
//!   `6.0.into()` / `padding(16)` literals.

use iced::theme::palette::{Extended, Pair};
use iced::widget::button;
use iced::{Background, Border, Color};

/// Corner radii. `SM` = inputs / ghost chrome, `MD` = buttons /
/// swatches, `LG` = cards / popovers, `XL` = large floating panels,
/// `PILL` = fully-rounded badges.
///
/// Graphite pass: 5 / 7 / 10 (was 4 / 6 / 8).
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
/// Roomier outer padding used by agent-style chat panes.
pub const SPACE_2XL: f32 = 20.0;
pub const SPACE_3XL: f32 = 28.0;

/// Regular control content padding `[vertical, horizontal]` — buttons,
/// default actions. Prefer this over inventing pad literals in apps.
pub const PAD_CONTROL: [u16; 2] = [6, 12];
/// Compact control padding — toolbar, steppers, dense chrome.
pub const PAD_CONTROL_SM: [u16; 2] = [5, 10];

pub fn hairline(palette: &Extended, radius: f32) -> Border {
    Border {
        color: palette.background.stronger.color,
        width: 1.0,
        radius: radius.into(),
    }
}

pub fn filled(
    base: Pair,
    strong: Pair,
    weak: Pair,
    status: button::Status,
) -> button::Style {
    let resting = button::Style {
        background: Some(Background::Color(base.color)),
        text_color: base.text,
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: RADIUS_MD.into() },
        shadow: Default::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(strong.color)),
            ..resting
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(weak.color)),
            ..resting
        },
        button::Status::Disabled => dim(resting),
        button::Status::Active => resting,
    }
}

pub fn dim(base: button::Style) -> button::Style {
    button::Style {
        background: base.background.map(|bg| match bg {
            Background::Color(c) => Background::Color(Color { a: c.a * 0.5, ..c }),
            other => other,
        }),
        text_color: Color { a: base.text_color.a * 0.5, ..base.text_color },
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
    fn hairline_uses_stronger_colour_and_unit_width() {
        let t = theme::default_theme();
        let ext = t.extended_palette();
        let b = hairline(ext, RADIUS_LG);
        assert_eq!(b.color, ext.background.stronger.color);
        assert_eq!(b.width, 1.0);
    }
}
