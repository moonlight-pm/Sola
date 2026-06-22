//! Color picker — a real picker control: a 2D saturation/value field, a
//! hue rail, an alpha rail, and Hex / RGB / HSL text inputs, all editing
//! one `iced::Color`.
//!
//! **Stateful** (a `ColorPicker` value, not a bare fn): HSV is the source
//! of truth, so hue and saturation survive a trip through value = 0 /
//! greyscale — a picker that re-derived HSV from RGB every frame would
//! forget the hue the moment the colour went black. The text fields are
//! free-text staging buffers (so partial input like `"#1a"` doesn't
//! snap); they're rebuilt from the canonical HSV whenever a drag or a
//! different field moves the colour. The caller holds a `ColorPicker`,
//! routes its [`Message`] through [`ColorPicker::update`], and reads
//! [`ColorPicker::color`] to apply the result:
//!
//! ```ignore
//! self.picker = Some(ColorPicker::new(current));         // open
//! if let Some(p) = &mut self.picker { p.update(m); self.apply(p.color()); }
//! self.picker.as_ref().map(|p| p.view().map(Msg::Picker)); // view
//! ```

use iced::widget::{column, row};
use iced::{Color, Element, Length, Theme};

use crate::components::spectrum::{alpha_strip, hue_strip, sv_square};
use crate::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use crate::components::swatch::swatch_sized;
use crate::components::text::{caption, muted};
use crate::components::text_input as kit_text_input;
use crate::theme::{color_to_hex, try_parse};

/// RGB (0–1 channels) → HSV with hue in 0–360°, saturation/value in 0–1.
fn rgb_to_hsv(c: Color) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let v = max;
    let s = if max <= 0.0 { 0.0 } else { d / max };
    let h = hue_of(c, max, d);
    (h, s, v)
}

/// HSV (hue 0–360°, sat/val 0–1) + alpha → an `iced::Color`.
fn hsv_to_rgb(h: f32, s: f32, v: f32, a: f32) -> Color {
    let c = v * s;
    let m = v - c;
    from_chroma(h, c, m, a)
}

/// RGB → HSL with hue in 0–360°, saturation/lightness in 0–1.
fn rgb_to_hsl(c: Color) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let l = (max + min) / 2.0;
    let s = if d <= 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let h = hue_of(c, max, d);
    (h, s, l)
}

/// HSL (hue 0–360°, sat/light 0–1) + alpha → an `iced::Color`.
fn hsl_to_rgb(h: f32, s: f32, l: f32, a: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let m = l - c / 2.0;
    from_chroma(h, c, m, a)
}

/// Shared hue computation (degrees) for the RGB→HS{V,L} paths.
fn hue_of(c: Color, max: f32, d: f32) -> f32 {
    let h = if d <= 0.0 {
        0.0
    } else if max == c.r {
        60.0 * (((c.g - c.b) / d).rem_euclid(6.0))
    } else if max == c.g {
        60.0 * ((c.b - c.r) / d + 2.0)
    } else {
        60.0 * ((c.r - c.g) / d + 4.0)
    };
    h.rem_euclid(360.0)
}

/// Shared chroma→RGB reconstruction for the HS{V,L}→RGB paths. `c` is
/// chroma, `m` the per-channel lightness offset.
fn from_chroma(h: f32, c: f32, m: f32, a: f32) -> Color {
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
    Color { r: r + m, g: g + m, b: b + m, a }
}

/// Which slot in a 3-channel text model (RGB or HSL) an edit targets.
#[derive(Debug, Clone, Copy)]
pub enum Channel {
    One,
    Two,
    Three,
}

impl Channel {
    fn idx(self) -> usize {
        match self {
            Channel::One => 0,
            Channel::Two => 1,
            Channel::Three => 2,
        }
    }
}

/// Stateful colour editor. HSV (+ alpha) is canonical; the text buffers
/// are free-text staging that mirror it.
#[derive(Debug, Clone)]
pub struct ColorPicker {
    h: f32,
    s: f32,
    v: f32,
    a: f32,
    hex: String,
    /// r/g/b as 0–255 strings.
    rgb: [String; 3],
    /// h(0–360)/s(0–100)/l(0–100) as strings.
    hsl: [String; 3],
}

/// Messages the picker emits; the caller wraps them in its own message
/// and feeds them back via [`ColorPicker::update`].
#[derive(Debug, Clone)]
pub enum Message {
    /// Saturation, value from the 2D field (each 0–1).
    Sv(f32, f32),
    /// Hue from the rail (0–360).
    Hue(f32),
    /// Alpha from the rail (0–1).
    Alpha(f32),
    /// Free-text hex field changed.
    Hex(String),
    /// Hex field committed (Enter) — canonicalise the buffer.
    HexSubmit,
    /// One RGB channel's text field changed.
    Rgb(Channel, String),
    /// One HSL channel's text field changed.
    Hsl(Channel, String),
}

impl ColorPicker {
    /// Open a picker seeded from `color`.
    pub fn new(color: Color) -> Self {
        let (h, s, v) = rgb_to_hsv(color);
        let mut me = Self {
            h,
            s,
            v,
            a: color.a,
            hex: String::new(),
            rgb: Default::default(),
            hsl: Default::default(),
        };
        me.sync_all();
        me
    }

    /// The colour the current HSV state resolves to.
    pub fn color(&self) -> Color {
        hsv_to_rgb(self.h, self.s, self.v, self.a)
    }

    /// Fold one message into the picker state.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Sv(s, v) => {
                self.s = s.clamp(0.0, 1.0);
                self.v = v.clamp(0.0, 1.0);
                self.sync_all();
            }
            Message::Hue(h) => {
                self.h = h.clamp(0.0, 360.0);
                self.sync_all();
            }
            Message::Alpha(a) => {
                self.a = a.clamp(0.0, 1.0);
                self.sync_all();
            }
            Message::Hex(buf) => {
                self.hex = buf;
                if let Some(c) = try_parse(&self.hex) {
                    self.adopt(c);
                    // Leave the hex buffer as typed; refresh the others.
                    self.sync_rgb();
                    self.sync_hsl();
                }
            }
            Message::HexSubmit => {
                if let Some(c) = try_parse(&self.hex) {
                    self.adopt(c);
                }
                self.sync_all();
            }
            Message::Rgb(ch, buf) => {
                self.rgb[ch.idx()] = buf;
                if let Some(c) = self.parse_rgb() {
                    self.adopt(c);
                    self.sync_hex();
                    self.sync_hsl();
                }
            }
            Message::Hsl(ch, buf) => {
                self.hsl[ch.idx()] = buf;
                if let Some(c) = self.parse_hsl() {
                    self.adopt(c);
                    self.sync_hex();
                    self.sync_rgb();
                }
            }
        }
    }

    /// Adopt an RGB colour as the new canonical HSV (alpha kept).
    fn adopt(&mut self, c: Color) {
        let (h, s, v) = rgb_to_hsv(c);
        self.h = h;
        self.s = s;
        self.v = v;
        self.a = c.a;
    }

    fn parse_rgb(&self) -> Option<Color> {
        let r = self.rgb[0].trim().parse::<u8>().ok()?;
        let g = self.rgb[1].trim().parse::<u8>().ok()?;
        let b = self.rgb[2].trim().parse::<u8>().ok()?;
        Some(Color::from_rgba8(r, g, b, self.a))
    }

    fn parse_hsl(&self) -> Option<Color> {
        let h = self.hsl[0].trim().parse::<f32>().ok()?;
        let s = self.hsl[1].trim().parse::<f32>().ok()?;
        let l = self.hsl[2].trim().parse::<f32>().ok()?;
        if !(0.0..=360.0).contains(&h) || !(0.0..=100.0).contains(&s) || !(0.0..=100.0).contains(&l)
        {
            return None;
        }
        Some(hsl_to_rgb(h, s / 100.0, l / 100.0, self.a))
    }

    fn sync_all(&mut self) {
        self.sync_hex();
        self.sync_rgb();
        self.sync_hsl();
    }

    fn sync_hex(&mut self) {
        self.hex = color_to_hex(self.color());
    }

    fn sync_rgb(&mut self) {
        let c = self.color();
        let to = |x: f32| ((x * 255.0).round() as u8).to_string();
        self.rgb = [to(c.r), to(c.g), to(c.b)];
    }

    fn sync_hsl(&mut self) {
        let (h, s, l) = rgb_to_hsl(self.color());
        self.hsl = [
            format!("{h:.0}"),
            format!("{:.0}", s * 100.0),
            format!("{:.0}", l * 100.0),
        ];
    }

    pub fn view(&self) -> Element<'_, Message, Theme> {
        let hue_color = hsv_to_rgb(self.h, 1.0, 1.0, 1.0);
        let surface = column![
            sv_square(hue_color, self.s, self.v, Message::Sv),
            hue_strip(self.h, Message::Hue),
            alpha_strip(self.color(), self.a, Message::Alpha),
            swatch_sized::<Message>(self.color(), 28.0),
        ]
        .spacing(SPACE_MD);

        let inputs = column![
            hex_row(&self.hex),
            triple_row("RGB", &self.rgb, Message::Rgb),
            triple_row("HSL", &self.hsl, Message::Hsl),
        ]
        .spacing(SPACE_MD);

        row![surface, inputs].spacing(SPACE_LG).into()
    }
}

/// The hex field: a wider input that adopts on a full `#rrggbb` and
/// canonicalises on Enter.
fn hex_row(hex: &str) -> Element<'_, Message, Theme> {
    row![
        caption("HEX").style(muted).width(Length::Fixed(34.0)),
        kit_text_input::text_input("#rrggbb", hex)
            .on_input(Message::Hex)
            .on_submit(Message::HexSubmit)
            .style(kit_text_input::style)
            .width(Length::Fixed(120.0)),
    ]
    .spacing(SPACE_MD)
    .align_y(iced::Alignment::Center)
    .into()
}

/// A labelled row of three small numeric fields (RGB or HSL).
fn triple_row<'a>(
    label: &'a str,
    values: &'a [String; 3],
    on_change: impl Fn(Channel, String) -> Message + Copy + 'a,
) -> Element<'a, Message, Theme> {
    let cell = move |ch: Channel, value: &'a str| {
        kit_text_input::text_input("", value)
            .on_input(move |s| on_change(ch, s))
            .style(kit_text_input::style)
            .width(Length::Fixed(52.0))
    };
    row![
        caption(label).style(muted).width(Length::Fixed(34.0)),
        cell(Channel::One, &values[0]),
        cell(Channel::Two, &values[1]),
        cell(Channel::Three, &values[2]),
    ]
    .spacing(SPACE_SM)
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
    fn hsl_primaries_and_grey() {
        // Pure red: hue 0, full sat, lightness 0.5.
        let (h, s, l) = rgb_to_hsl(Color::from_rgb(1.0, 0.0, 0.0));
        assert!(close(h, 0.0) && close(s, 1.0) && close(l, 0.5), "red hsl: {h},{s},{l}");
        // Mid grey: zero saturation, lightness 0.5.
        let (_, s, l) = rgb_to_hsl(Color::from_rgb(0.5, 0.5, 0.5));
        assert!(close(s, 0.0) && close(l, 0.5), "grey hsl s/l: {s},{l}");
        // White: lightness 1.
        let (_, _, l) = rgb_to_hsl(Color::from_rgb(1.0, 1.0, 1.0));
        assert!(close(l, 1.0), "white lightness: {l}");
    }

    #[test]
    fn hsl_round_trips_through_rgb() {
        let samples = [
            Color::from_rgb(0.1, 0.2, 0.3),
            Color::from_rgb(0.9, 0.4, 0.05),
            Color::from_rgb(0.0, 0.5, 0.5),
            Color::from_rgba(0.33, 0.66, 0.99, 0.5),
        ];
        for c in samples {
            let (h, s, l) = rgb_to_hsl(c);
            assert!(color_close(hsl_to_rgb(h, s, l, c.a), c), "hsl round-trip failed for {c:?}");
        }
    }

    #[test]
    fn picker_preserves_hue_across_zero_value() {
        // Seed a saturated orange, drag value to 0 via the SV field, then
        // back up: hue must survive (the whole point of storing HSV).
        let mut p = ColorPicker::new(Color::from_rgb(0.9, 0.5, 0.1));
        let (h0, s0) = (p.h, p.s);
        p.update(Message::Sv(s0, 0.0));
        p.update(Message::Sv(s0, 1.0));
        assert!(close(p.h, h0), "hue drifted: {} vs {h0}", p.h);
        assert!(close(p.s, s0), "sat drifted: {} vs {s0}", p.s);
    }

    #[test]
    fn hex_input_adopts_valid_and_ignores_partial() {
        let mut p = ColorPicker::new(Color::from_rgb(0.0, 0.0, 0.0));
        p.update(Message::Hex("#1a".to_string()));
        // Partial: buffer holds it, colour unchanged (still black).
        assert!(color_close(p.color(), Color::from_rgb(0.0, 0.0, 0.0)));
        p.update(Message::Hex("#ff8800".to_string()));
        assert!(color_close(p.color(), Color::from_rgb8(0xff, 0x88, 0x00)));
    }

    #[test]
    fn rgb_field_adopts_valid_and_ignores_invalid() {
        let mut p = ColorPicker::new(Color::from_rgb(0.0, 0.0, 0.0));
        // Set R=255 (G/B already "0" from seed), expect pure-ish red.
        p.update(Message::Rgb(Channel::One, "255".to_string()));
        let c = p.color();
        assert!(close(c.r, 1.0) && close(c.g, 0.0) && close(c.b, 0.0), "rgb adopt: {c:?}");
        // Out-of-range / non-numeric is ignored — colour unchanged.
        p.update(Message::Rgb(Channel::Two, "999".to_string()));
        assert!(color_close(p.color(), c), "invalid rgb should not change colour");
    }
}
