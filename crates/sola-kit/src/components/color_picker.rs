//! Color picker — RGB sliders + a live hex readout for editing one
//! `iced::Color`. Stateless: the caller owns the color and receives a
//! new one via `on_change` whenever a channel moves. Drop it next to a
//! [`crate::components::swatch`] showing the current value.
//!
//! v0 edits the three RGB channels (lossless, no internal state).
//! Free-text hex *entry* needs an in-progress buffer the caller holds,
//! so it's deferred; the hex value is shown read-only for now. Alpha is
//! preserved but not edited (the kit's atoms are all opaque).

use iced::widget::{column, row, slider};
use iced::{Color, Element, Length, Theme};

use crate::components::style::{SPACE_MD, SPACE_SM};
use crate::components::text::{caption, code, muted};
use crate::theme::color_to_hex;

/// 0.0–1.0 channel → 0–255 byte, rounded and clamped.
fn channel_u8(v: f32) -> u8 {
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// 0–255 byte → 0.0–1.0 channel.
fn u8_channel(v: u8) -> f32 {
    v as f32 / 255.0
}

/// Replace the red channel, preserving green / blue / alpha.
fn with_r(c: Color, v: u8) -> Color {
    Color { r: u8_channel(v), ..c }
}

/// Replace the green channel, preserving red / blue / alpha.
fn with_g(c: Color, v: u8) -> Color {
    Color { g: u8_channel(v), ..c }
}

/// Replace the blue channel, preserving red / green / alpha.
fn with_b(c: Color, v: u8) -> Color {
    Color { b: u8_channel(v), ..c }
}

/// Build the picker for `color`. `on_change` fires with the edited
/// color on every channel move.
pub fn color_picker<'a, Message: Clone + 'a>(
    color: Color,
    on_change: impl Fn(Color) -> Message + Clone + 'a,
) -> Element<'a, Message, Theme> {
    let on_r = on_change.clone();
    let on_g = on_change.clone();
    let on_b = on_change;
    column![
        channel_row("R", channel_u8(color.r), move |v| on_r(with_r(color, v))),
        channel_row("G", channel_u8(color.g), move |v| on_g(with_g(color, v))),
        channel_row("B", channel_u8(color.b), move |v| on_b(with_b(color, v))),
        code(color_to_hex(color)).style(muted),
    ]
    .spacing(SPACE_SM)
    .into()
}

/// One labelled `0..=255` slider with its current byte value shown.
fn channel_row<'a, Message: Clone + 'a>(
    label: &'a str,
    value: u8,
    on_change: impl Fn(u8) -> Message + 'a,
) -> Element<'a, Message, Theme> {
    row![
        caption(label).style(muted).width(Length::Fixed(14.0)),
        slider(0..=255u8, value, on_change).width(Length::Fixed(160.0)),
        caption(value.to_string()).style(muted).width(Length::Fixed(28.0)),
    ]
    .spacing(SPACE_MD)
    .align_y(iced::Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_u8_maps_endpoints_and_midpoint() {
        assert_eq!(channel_u8(0.0), 0);
        assert_eq!(channel_u8(1.0), 255);
        // 0.5 * 255 = 127.5, rounds to 128.
        assert_eq!(channel_u8(0.5), 128);
    }

    #[test]
    fn channel_u8_clamps_out_of_range() {
        assert_eq!(channel_u8(-0.3), 0);
        assert_eq!(channel_u8(2.0), 255);
    }

    #[test]
    fn byte_channel_round_trips() {
        for v in [0u8, 1, 64, 127, 128, 200, 255] {
            assert_eq!(channel_u8(u8_channel(v)), v, "round-trip failed at {v}");
        }
    }

    #[test]
    fn with_r_sets_red_and_preserves_others() {
        let c = Color::from_rgba(0.1, 0.2, 0.3, 0.4);
        let out = with_r(c, 255);
        assert_eq!(out.r, 1.0);
        assert_eq!(out.g, c.g);
        assert_eq!(out.b, c.b);
        assert_eq!(out.a, c.a);
    }

    #[test]
    fn with_g_and_b_preserve_alpha_and_siblings() {
        let c = Color::from_rgba(0.1, 0.2, 0.3, 0.4);
        let g = with_g(c, 0);
        assert_eq!(g.g, 0.0);
        assert_eq!(g.r, c.r);
        assert_eq!(g.b, c.b);
        assert_eq!(g.a, c.a);
        let b = with_b(c, 128);
        assert_eq!(channel_u8(b.b), 128);
        assert_eq!(b.r, c.r);
        assert_eq!(b.g, c.g);
        assert_eq!(b.a, c.a);
    }
}
