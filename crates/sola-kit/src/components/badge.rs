//! Badge — status pill with a colored background and a short label.
//!
//! `badge(label, tone)` returns an `Element` ready to drop into a row
//! or inline run. Tones map to kit palette tiers so a `Success` badge
//! reads against any `card`/`sidebar` surface without per-call style
//! wiring.

use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Padding, Theme};

use crate::fonts;

/// Visual flavors of badge. Each maps to a palette tier in [`style`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

/// Compact pill — condensed-bold 10px label, 4×8 padding, fully
/// rounded corners. Pass a [`Tone`] for the color story.
pub fn badge<'a, Message: 'a>(
    label: impl text::IntoFragment<'a>,
    tone: Tone,
) -> Element<'a, Message, Theme> {
    container(text(label).font(fonts::ui_medium()).size(10))
        .padding(Padding::from([2, 8]))
        .style(move |t| style(t, tone))
        .into()
}

/// Style fn for the badge container. Exposed so callers building their
/// own labeled chrome can pick up the same palette mapping.
pub fn style(theme: &Theme, tone: Tone) -> container::Style {
    let p = theme.extended_palette();
    let pair = match tone {
        Tone::Neutral => p.secondary.base,
        Tone::Accent => p.primary.base,
        Tone::Success => p.success.base,
        Tone::Warning => p.warning.base,
        Tone::Danger => p.danger.base,
    };
    container::Style {
        background: Some(Background::Color(pair.color)),
        text_color: Some(pair.text),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 999.0.into(),
        },
        ..container::Style::default()
    }
}
