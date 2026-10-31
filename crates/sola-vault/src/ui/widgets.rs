//! Small view parts: icons, tools, hairlines, plates, the code's ring.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use iced::alignment::Horizontal;
use iced::widget::tooltip::Position;
use iced::widget::{Space, button, container, stack, svg, text, tooltip};
use iced::{Alignment, Color, Element, Length, Padding, Theme};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::popover;
use sola_kit::components::select::enamel;
use sola_kit::components::style::HAIRLINE_STRONG_A;
use sola_kit::components::style::mix_white;
use sola_kit::fonts;

use super::{Msg, style};
use crate::model::plate::Plate;

/// A pack icon (`"lucide/copy"`), loaded once.
pub fn handle(name: &'static str) -> svg::Handle {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, svg::Handle>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    cache.entry(name).or_insert_with(|| icon_handle(name)).clone()
}

/// An icon in `color` (from the theme at draw time).
pub fn glyph<'a>(
    name: &'static str,
    size: f32,
    color: impl Fn(&Theme) -> Color + 'a,
) -> Element<'a, Msg> {
    svg(handle(name))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |theme: &Theme, _| svg::Style {
            color: Some(color(theme)),
        })
        .into()
}

pub fn muted<'a>(name: &'static str, size: f32) -> Element<'a, Msg> {
    glyph(name, size, style::ink2)
}

pub fn faint<'a>(name: &'static str, size: f32) -> Element<'a, Msg> {
    glyph(name, size, style::ink3)
}

/// A 28 px tool: an icon, a tip, a press.
pub fn tool<'a>(name: &'static str, tip: &'static str, on_press: Option<Msg>) -> Element<'a, Msg> {
    tool_styled(name, tip, on_press, false)
}

/// A tool that shows it is on (a reveal, an open menu).
pub fn tool_toggle<'a>(
    name: &'static str,
    tip: &'static str,
    on: bool,
    on_press: Msg,
) -> Element<'a, Msg> {
    tool_styled(name, tip, Some(on_press), on)
}

fn tool_styled<'a>(
    name: &'static str,
    tip: &'static str,
    on_press: Option<Msg>,
    on: bool,
) -> Element<'a, Msg> {
    let enabled = on_press.is_some();
    let icon = glyph(name, 15.0, move |t: &Theme| {
        let c = style::ink2(t);
        if enabled { c } else { Color { a: c.a * 0.45, ..c } }
    });
    let btn = button(
        container(icon)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .center_x(Length::Fixed(28.0))
            .center_y(Length::Fixed(28.0)),
    )
    .padding(0)
    .style(if on { style::tool_on } else { style::tool })
    .on_press_maybe(on_press);
    if !enabled {
        return btn.into();
    }
    tooltip(btn, tip_card(tip), Position::Bottom)
        .gap(6)
        .delay(Duration::from_millis(400))
        .into()
}

fn tip_card<'a>(tip: &'static str) -> Element<'a, Msg> {
    container(text(tip).font(fonts::ui()).size(12))
        .padding(Padding::from([5.0, 8.0]))
        .style(popover::style)
        .into()
}

pub fn v_hairline<'a>() -> Element<'a, Msg> {
    container(Space::new().width(1).height(Length::Fill))
        .width(1)
        .height(Length::Fill)
        .style(style::hairline_fill)
        .into()
}

pub fn h_hairline<'a>() -> Element<'a, Msg> {
    container(Space::new().width(Length::Fill).height(1))
        .width(Length::Fill)
        .height(1)
        .style(style::hairline_fill)
        .into()
}

/// Sizes of a plate: the list's, the record head's.
#[derive(Debug, Clone, Copy)]
pub enum PlateSize {
    Row,
    Head,
    Small,
}

impl PlateSize {
    fn px(self) -> (f32, f32, f32, f32) {
        // side, radius, letter size, glyph size
        match self {
            PlateSize::Row => (28.0, 7.0, 12.0, 14.0),
            PlateSize::Head => (32.0, 8.0, 14.0, 16.0),
            PlateSize::Small => (22.0, 6.0, 10.5, 12.0),
        }
    }
}

pub fn plate<'a>(plate: &Plate, size: PlateSize) -> Element<'a, Msg> {
    let (side, radius, letter_size, glyph_size) = size.px();
    let inner: Element<'a, Msg> = match plate {
        Plate::Letter { letter, .. } => text(letter.to_string())
            .font(fonts::ui_medium())
            .size(letter_size)
            .align_x(Horizontal::Center)
            .into(),
        Plate::Glyph(name) => muted(name, glyph_size),
    };
    let base = container(inner)
        .width(Length::Fixed(side))
        .height(Length::Fixed(side))
        .center_x(Length::Fixed(side))
        .center_y(Length::Fixed(side));
    match plate {
        Plate::Letter { seed, .. } => base.style(style::plate(enamel(seed), radius)).into(),
        Plate::Glyph(_) => base.style(style::kind_plate(radius)).into(),
    }
}

/// A plate with a lucide glyph of its own (the lock on Unlock).
pub fn glyph_plate<'a>(name: &'static str, size: PlateSize) -> Element<'a, Msg> {
    plate(&Plate::Glyph(name), size)
}

/// The dot by a changed field's label.
pub fn changed_dot<'a>() -> Element<'a, Msg> {
    container(Space::new().width(5).height(5))
        .width(5)
        .height(5)
        .style(style::changed_dot)
        .into()
}

const RING: f32 = 22.0;

/// The code's time left: an arc of its period, the seconds inside,
/// warning in the last five. Tinted SVG (not canvas: small canvases
/// misdraw on wgpu's GL backend, see the kit's status mark).
pub fn ring<'a>(left: u32, period: u32, warning: bool) -> Element<'a, Msg> {
    let tone = move |t: &Theme| {
        if warning {
            t.extended_palette().warning.base.color
        } else {
            style::ink2(t)
        }
    };
    let track = svg(ring_svg(None))
        .width(Length::Fixed(RING))
        .height(Length::Fixed(RING))
        .style(|t: &Theme, _| svg::Style {
            color: Some(mix_white(style::raised_color(t), HAIRLINE_STRONG_A + 0.06)),
        });
    let arc = svg(ring_svg(Some((left, period))))
        .width(Length::Fixed(RING))
        .height(Length::Fixed(RING))
        .style(move |t: &Theme, _| svg::Style {
            color: Some(tone(t)),
        });
    let number = container(
        text(left.to_string())
            .font(fonts::ui_medium())
            .size(9)
            .style(move |t: &Theme| text::Style {
                color: Some(tone(t)),
            }),
    )
    .width(Length::Fixed(RING))
    .height(Length::Fixed(RING))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center);
    stack![track, arc, number]
        .width(Length::Fixed(RING))
        .height(Length::Fixed(RING))
        .into()
}

/// The track (`None`) or the arc for `left` of `period`, cached.
fn ring_svg(arc: Option<(u32, u32)>) -> svg::Handle {
    static CACHE: OnceLock<Mutex<HashMap<Option<(u32, u32)>, svg::Handle>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    cache
        .entry(arc)
        .or_insert_with(|| svg::Handle::from_memory(ring_source(arc).into_bytes()))
        .clone()
}

fn ring_source(arc: Option<(u32, u32)>) -> String {
    const R: f32 = 9.0;
    let open = r#"<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 22 22">"#;
    match arc {
        None => format!(
            r##"{open}<circle cx="11" cy="11" r="{R}" fill="none" stroke="#000" stroke-width="2"/></svg>"##
        ),
        Some((left, period)) => {
            let c = 2.0 * std::f32::consts::PI * R;
            let frac = left as f32 / period.max(1) as f32;
            format!(
                r##"{open}<circle cx="11" cy="11" r="{R}" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-dasharray="{:.2} {:.2}" transform="rotate(-90 11 11)"/></svg>"##,
                c * frac.clamp(0.0, 1.0),
                c
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arc_is_a_share_of_the_circle() {
        let full = ring_source(Some((30, 30)));
        let half = ring_source(Some((15, 30)));
        assert!(full.contains("stroke-dasharray=\"56.55 56.55\""), "{full}");
        assert!(half.contains("stroke-dasharray=\"28.27 56.55\""), "{half}");
        assert!(!ring_source(None).contains("dasharray"));
    }
}
