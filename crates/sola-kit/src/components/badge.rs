//! Badge — status pill with a colored background and a short label.
//!
//! `badge(label, tone)` returns an `Element` ready to drop into a row
//! or inline run. Tones map to kit palette tiers so a `Success` badge
//! reads against any `card`/`sidebar` surface without per-call style
//! wiring.

use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Padding, Theme};

use crate::components::style::RADIUS_PILL;
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

/// Compact pill — medium-weight 10px label, 2×8 padding, fully
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
///
/// Soft tinted pills (graphite): ~12% tone fill + ~28% tone border +
/// solid tone text — not solid filled slabs (except Neutral quiet grey).
pub fn style(theme: &Theme, tone: Tone) -> container::Style {
    let p = theme.extended_palette();
    let (fg, base) = match tone {
        Tone::Neutral => (p.secondary.base.text, p.background.strong.color),
        Tone::Accent => (p.primary.base.color, p.primary.base.color),
        Tone::Success => (p.success.base.color, p.success.base.color),
        Tone::Warning => (p.warning.base.color, p.warning.base.color),
        Tone::Danger => (p.danger.base.color, p.danger.base.color),
    };
    let (bg, border) = if matches!(tone, Tone::Neutral) {
        (
            Color {
                a: 0.85,
                ..base
            },
            p.background.stronger.color,
        )
    } else {
        (
            Color { a: 0.12, ..base },
            Color { a: 0.28, ..base },
        )
    };
    container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(fg),
        border: Border {
            color: border,
            width: 1.0,
            radius: RADIUS_PILL.into(),
        },
        ..container::Style::default()
    }
}
