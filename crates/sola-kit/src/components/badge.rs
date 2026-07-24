//! Badge — status pill with a soft tinted background and a short label.
//!
//! `badge(label, tone)` returns an `Element` ready to drop into a row
//! or inline run. Tones map to kit palette tiers so a `Success` badge
//! reads against any `card`/`sidebar` surface without per-call style
//! wiring.

use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Padding, Theme};

use crate::components::style::{RADIUS_PILL, alpha};
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

/// Compact pill — medium-weight 10px label, letterspaced uppercase feel,
/// soft tone fill + matching border (OD soft badges, not solid slabs).
pub fn badge<'a, Message: 'a>(
    label: impl text::IntoFragment<'a>,
    tone: Tone,
) -> Element<'a, Message, Theme> {
    container(text(label).font(fonts::ui_medium()).size(10))
        .padding(Padding::from([3, 8]))
        .style(move |t| style(t, tone))
        .into()
}

/// Style fn for the badge container. Exposed so callers building their
/// own labeled chrome can pick up the same palette mapping.
///
/// Status tones use ~14% tone fill + ~28% tone border + tone-coloured
/// text. Neutral is quiet hover-grey + hairline.
pub fn style(theme: &Theme, tone: Tone) -> container::Style {
    let p = theme.extended_palette();
    let (bg, fg, border) = match tone {
        Tone::Neutral => (
            alpha(p.background.strong.color, 0.80),
            p.secondary.base.text,
            Color::from_rgba(1.0, 1.0, 1.0, 0.07),
        ),
        Tone::Accent => {
            let c = p.primary.base.color;
            (alpha(c, 0.14), c, alpha(c, 0.28))
        }
        Tone::Success => {
            let c = p.success.base.color;
            (alpha(c, 0.14), c, alpha(c, 0.28))
        }
        Tone::Warning => {
            let c = p.warning.base.color;
            (alpha(c, 0.14), c, alpha(c, 0.28))
        }
        Tone::Danger => {
            let c = p.danger.base.color;
            (alpha(c, 0.14), c, alpha(c, 0.28))
        }
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
