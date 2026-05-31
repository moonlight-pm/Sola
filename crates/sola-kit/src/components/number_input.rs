//! NumberInput — a unit-aware stepper. A `[−] value unit [+]` row for
//! editing a numeric token (radius px, spacing px, text size pt, …).
//!
//! Stateless: the caller owns the value; the buttons emit the clamped
//! next value via `on_change`. Targets integer-ish tokens, so the value
//! is shown rounded to whole units. Free-text entry is deliberately left
//! out for v0 — the steppers cover the token-editing case; add a typed
//! field later if a consumer needs fine values.

use std::ops::RangeInclusive;

use iced::widget::{button, container, row, text};
use iced::{Element, Length, Theme};

use crate::components::button as kit_button;
use crate::components::style::SPACE_SM;

/// `value + delta`, clamped to `range`. The whole of NumberInput's
/// logic; the rest is layout.
fn step_clamped(value: f32, delta: f32, range: &RangeInclusive<f32>) -> f32 {
    (value + delta).clamp(*range.start(), *range.end())
}

/// A `[−] value unit [+]` stepper. `step` is the increment per button
/// press; `range` clamps both ends; `unit` is an optional trailing
/// label (`"px"`, `"pt"`, or `""`).
pub fn number_input<'a, Message: Clone + 'a>(
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    unit: &'a str,
    on_change: impl Fn(f32) -> Message,
) -> Element<'a, Message, Theme> {
    let dec = on_change(step_clamped(value, -step, &range));
    let inc = on_change(step_clamped(value, step, &range));
    let label = if unit.is_empty() {
        format!("{value:.0}")
    } else {
        format!("{value:.0} {unit}")
    };
    row![
        button(text("−"))
            .style(kit_button::secondary)
            .padding([2, 10])
            .on_press(dec),
        container(text(label)).center_x(Length::Fixed(64.0)),
        button(text("+"))
            .style(kit_button::secondary)
            .padding([2, 10])
            .on_press(inc),
    ]
    .spacing(SPACE_SM)
    .align_y(iced::Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_clamps_to_range() {
        let r = 0.0..=10.0;
        assert_eq!(step_clamped(5.0, 1.0, &r), 6.0);
        assert_eq!(step_clamped(5.0, -1.0, &r), 4.0);
        assert_eq!(step_clamped(10.0, 1.0, &r), 10.0, "must clamp at the top");
        assert_eq!(step_clamped(0.0, -1.0, &r), 0.0, "must clamp at the bottom");
    }
}
