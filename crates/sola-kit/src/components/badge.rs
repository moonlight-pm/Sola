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
/// **Neutral** is quiet chrome: hover-grey fill + muted text — not a
/// solid border-coloured slab. Status tones keep scanable fills.
pub fn style(theme: &Theme, tone: Tone) -> container::Style {
    let p = theme.extended_palette();
    let (bg, fg) = match tone {
        // Quiet secondary surface — background.strong (hover grey) + muted
        // text. Avoid secondary.base (bound to BORDER), which read as a
        // solid border-coloured slab next to calm chrome.
        Tone::Neutral => (p.background.strong.color, p.secondary.base.text),
        Tone::Accent => (p.primary.base.color, p.primary.base.text),
        Tone::Success => (p.success.base.color, p.success.base.text),
        Tone::Warning => (p.warning.base.color, p.warning.base.text),
        Tone::Danger => (p.danger.base.color, p.danger.base.text),
    };
    container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(fg),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_PILL.into(),
        },
        ..container::Style::default()
    }
}
