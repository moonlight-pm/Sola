//! Color picker — HSV sliders + a free-text hex field for editing one
//! `iced::Color`.
//!
//! **Stateful** (a `ColorPicker` value, not a bare fn): HSV is the
//! source of truth, so hue and saturation survive a trip through
//! value = 0 / greyscale — a picker that re-derived HSV from RGB every
//! frame would forget the hue the moment the color went black. The
//! caller holds a `ColorPicker`, routes its [`Message`] through
//! [`ColorPicker::update`], and reads [`ColorPicker::color`] to apply
//! the result:
//!
//! ```ignore
//! // open:
//! self.picker = Some(ColorPicker::new(current));
//! // in update, on Msg::Picker(m):
//! if let Some(p) = &mut self.picker { p.update(m); self.apply(p.color()); }
//! // in view:
//! self.picker.as_ref().map(|p| p.view().map(Msg::Picker))
//! ```

use std::ops::RangeInclusive;

use iced::widget::{column, row, slider, text_input};
use iced::{Color, Element, Length, Theme};

use crate::components::style::{SPACE_MD, SPACE_SM};
use crate::components::text::{caption, code, muted};
use crate::components::text_input as kit_text_input;
use crate::theme::{color_to_hex, try_parse};

/// RGB (0–1 channels) → HSV with hue in 0–360°, saturation/value in 0–1.
fn rgb_to_hsv(c: Color) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let v = max;
    let s = if max <= 0.0 { 0.0 } else { d / max };
    let h = if d <= 0.0 {
        0.0
    } else if max == c.r {
        60.0 * (((c.g - c.b) / d).rem_euclid(6.0))
    } else if max == c.g {
        60.0 * ((c.b - c.r) / d + 2.0)
    } else {
        60.0 * ((c.r - c.g) / d + 4.0)
    };
    (h.rem_euclid(360.0), s, v)
}

/// HSV (hue 0–360°, sat/val 0–1) + alpha → an `iced::Color`.
fn hsv_to_rgb(h: f32, s: f32, v: f32, a: f32) -> Color {
    let c = v * s;
    let h6 = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    Color { r: r + m, g: g + m, b: b + m, a }
}

/// Stateful HSV + hex color editor. HSV is canonical; the hex buffer is
/// a free-text staging area so partial input (`"#1a"`) doesn't snap.
#[derive(Debug, Clone)]
pub struct ColorPicker {
    h: f32,
    s: f32,
    v: f32,
    a: f32,
    hex: String,
}

/// Messages the picker emits; the caller wraps them in its own message
/// and feeds them back via [`ColorPicker::update`].
#[derive(Debug, Clone)]
pub enum Message {
    Hue(f32),
    Sat(f32),
    Val(f32),
    HexInput(String),
    HexSubmit,
}

impl ColorPicker {
    /// Open a picker seeded from `color`.
    pub fn new(color: Color) -> Self {
        let (h, s, v) = rgb_to_hsv(color);
        Self { h, s, v, a: color.a, hex: color_to_hex(color) }
    }

    /// The color the current HSV state resolves to.
    pub fn color(&self) -> Color {
        hsv_to_rgb(self.h, self.s, self.v, self.a)
    }

    /// Fold one message into the picker state.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Hue(h) => {
                self.h = h;
                self.sync_hex();
            }
            Message::Sat(s) => {
                self.s = (s / 100.0).clamp(0.0, 1.0);
                self.sync_hex();
            }
            Message::Val(v) => {
                self.v = (v / 100.0).clamp(0.0, 1.0);
                self.sync_hex();
            }
            Message::HexInput(buf) => {
                // Keep the raw buffer so partial input shows; only adopt
                // the HSV when it parses to a full #rrggbb.
                self.hex = buf;
                if let Some(c) = try_parse(&self.hex) {
                    let (h, s, v) = rgb_to_hsv(c);
                    self.h = h;
                    self.s = s;
                    self.v = v;
                }
            }
            Message::HexSubmit => {
                if let Some(c) = try_parse(&self.hex) {
                    let (h, s, v) = rgb_to_hsv(c);
                    self.h = h;
                    self.s = s;
                    self.v = v;
                }
                // Normalise the buffer back to canonical form.
                self.sync_hex();
            }
        }
    }

    fn sync_hex(&mut self) {
        self.hex = color_to_hex(self.color());
    }

    pub fn view(&self) -> Element<'_, Message, Theme> {
        column![
            channel("H", self.h, 0.0..=360.0, Message::Hue),
            channel("S", self.s * 100.0, 0.0..=100.0, Message::Sat),
            channel("V", self.v * 100.0, 0.0..=100.0, Message::Val),
            row![
                text_input("#rrggbb", &self.hex)
                    .on_input(Message::HexInput)
                    .on_submit(Message::HexSubmit)
                    .style(kit_text_input::style)
                    .width(Length::Fixed(120.0)),
                code(color_to_hex(self.color())).style(muted),
            ]
            .spacing(SPACE_MD)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(SPACE_SM)
        .into()
    }
}

/// One labelled slider with its current value shown to the right.
fn channel<'a>(
    label: &'a str,
    value: f32,
    range: RangeInclusive<f32>,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message, Theme> {
    row![
        caption(label).style(muted).width(Length::Fixed(14.0)),
        slider(range, value, on_change).step(1.0).width(Length::Fixed(180.0)),
        caption(format!("{value:.0}")).style(muted).width(Length::Fixed(34.0)),
    ]
    .spacing(SPACE_MD)
    .align_y(iced::Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn color_close(a: Color, b: Color) -> bool {
        close(a.r, b.r) && close(a.g, b.g) && close(a.b, b.b) && close(a.a, b.a)
    }

    #[test]
    fn primaries_map_to_known_hsv() {
        let (h, s, v) = rgb_to_hsv(Color::from_rgb(1.0, 0.0, 0.0));
        assert!(close(h, 0.0) && close(s, 1.0) && close(v, 1.0), "red: {h},{s},{v}");
        let (h, _, _) = rgb_to_hsv(Color::from_rgb(0.0, 1.0, 0.0));
        assert!(close(h, 120.0), "green hue: {h}");
        let (h, _, _) = rgb_to_hsv(Color::from_rgb(0.0, 0.0, 1.0));
        assert!(close(h, 240.0), "blue hue: {h}");
    }

    #[test]
    fn greys_have_zero_saturation() {
        for g in [0.0_f32, 0.25, 0.5, 1.0] {
            let (_, s, v) = rgb_to_hsv(Color::from_rgb(g, g, g));
            assert!(close(s, 0.0), "grey {g} sat: {s}");
            assert!(close(v, g), "grey {g} val: {v}");
        }
    }

    #[test]
    fn hsv_to_rgb_hits_known_corners() {
        assert!(color_close(hsv_to_rgb(0.0, 1.0, 1.0, 1.0), Color::from_rgb(1.0, 0.0, 0.0)));
        assert!(color_close(hsv_to_rgb(120.0, 1.0, 1.0, 1.0), Color::from_rgb(0.0, 1.0, 0.0)));
        assert!(color_close(hsv_to_rgb(240.0, 1.0, 1.0, 1.0), Color::from_rgb(0.0, 0.0, 1.0)));
        // Hue wraps at 360.
        assert!(color_close(hsv_to_rgb(360.0, 1.0, 1.0, 1.0), Color::from_rgb(1.0, 0.0, 0.0)));
    }

    #[test]
    fn round_trips_through_hsv() {
        let samples = [
            Color::from_rgb(0.1, 0.2, 0.3),
            Color::from_rgb(0.9, 0.4, 0.05),
            Color::from_rgb(0.0, 0.5, 0.5),
            Color::from_rgba(0.33, 0.66, 0.99, 0.5),
        ];
        for c in samples {
            let (h, s, v) = rgb_to_hsv(c);
            assert!(color_close(hsv_to_rgb(h, s, v, c.a), c), "round-trip failed for {c:?}");
        }
    }

    #[test]
    fn picker_preserves_hue_across_zero_value() {
        // Seed a saturated orange, drag value to 0, then back up: hue
        // must survive (the whole point of storing HSV).
        let mut p = ColorPicker::new(Color::from_rgb(0.9, 0.5, 0.1));
        let (h0, s0) = (p.h, p.s);
        p.update(Message::Val(0.0));
        p.update(Message::Val(100.0));
        assert!(close(p.h, h0), "hue drifted: {} vs {h0}", p.h);
        assert!(close(p.s, s0), "sat drifted: {} vs {s0}", p.s);
    }

    #[test]
    fn hex_input_adopts_valid_and_ignores_partial() {
        let mut p = ColorPicker::new(Color::from_rgb(0.0, 0.0, 0.0));
        p.update(Message::HexInput("#1a".to_string()));
        // Partial: buffer holds it, color unchanged (still black).
        assert!(color_close(p.color(), Color::from_rgb(0.0, 0.0, 0.0)));
        p.update(Message::HexInput("#ff8800".to_string()));
        assert!(color_close(p.color(), Color::from_rgb8(0xff, 0x88, 0x00)));
    }
}
