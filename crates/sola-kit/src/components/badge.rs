//! Badge — status pill with a soft tinted background and a short label.
//!
//! `badge(label, tone)` returns an `Element` ready to drop into a row
//! or inline run. Tones map to kit palette tiers so a `Success` badge
//! reads against any `card`/`sidebar` surface without per-call style
//! wiring.

use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Padding, Theme};

use crate::components::style::{RADIUS_PILL, mix_white};
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

/// Compact pill — medium-weight 10px uppercase label, soft tone fill +
/// matching border (OD soft badges, not solid slabs).
///
/// Iced has no letter-spacing API; OD uses `letter-spacing: 0.08em` —
/// medium weight + 10px + uppercase labels is the closest native match.
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
    // Soft fills + borders: bake tone into the raised surface (opaque)
    // so iced's linear border path doesn't inflate alpha edges.
    let raised = p.background.weaker.color;
    let soft = |c: Color, amt: f32| Color {
        r: raised.r * (1.0 - amt) + c.r * amt,
        g: raised.g * (1.0 - amt) + c.g * amt,
        b: raised.b * (1.0 - amt) + c.b * amt,
        a: 1.0,
    };
    let (bg, fg, border) = match tone {
        Tone::Neutral => (
            mix_white(raised, 0.06),
            p.secondary.base.text,
            mix_white(raised, 0.08),
        ),
        Tone::Accent => {
            // Neon stays neon — do not mix `#3dd6f5` into graphite
            // (that mix is the muddy dark cyan). Graphite fill, full
            // accent type + thin accent edge.
            let c = p.primary.base.color;
            (mix_white(raised, 0.06), c, c)
        }
        Tone::Success => {
            let c = p.success.base.color;
            (soft(c, 0.14), c, soft(c, 0.28))
        }
        Tone::Warning => {
            let c = p.warning.base.color;
            (soft(c, 0.14), c, soft(c, 0.28))
        }
        Tone::Danger => {
            let c = p.danger.base.color;
            (soft(c, 0.14), c, soft(c, 0.28))
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
