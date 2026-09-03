//! Badge — status pill with a soft tinted background and a short label.
//!
//! `badge(label, tone)` returns an `Element` ready to drop into a row
//! or inline run. Tones map to kit palette tiers so a `Success` badge
//! reads against any `card`/`sidebar` surface without per-call style
//! wiring.
//!
//! [`count_mark`] is the overlapping numeral on an app icon (switcher)
//! or a group header (notification pile) — filled accent, not a status
//! pill.

use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::components::style::{RADIUS_PILL, mix_white, primary_fill};
use crate::fonts;

/// Compact height of [`count_mark`] — sized to sit on a 72px switcher
/// icon without covering the face.
pub const COUNT_MARK_H: f32 = 18.0;

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

/// `1`…`99` or `99+`. Shared by menubar chips and [`count_mark`].
pub fn count_label(n: u32) -> String {
    if n > 99 { "99+".into() } else { n.to_string() }
}

/// Filled accent disc/pill with a dark numeral. Hidden when `n == 0`.
///
/// Graphite halo so the mark punches off a full-color app icon; on a
/// graphite plate the ring disappears into the face.
pub fn count_mark<'a, Message: 'a>(n: u32) -> Element<'a, Message, Theme> {
    if n == 0 {
        return container(text(""))
            .width(Length::Shrink)
            .height(Length::Fixed(0.0))
            .into();
    }
    let label = count_label(n);
    let wide = n > 9;
    let pad_x = if wide { 6.0 } else { 0.0 };
    let width = if wide {
        Length::Shrink
    } else {
        Length::Fixed(COUNT_MARK_H)
    };
    container(text(label).font(fonts::chrome()).size(10).line_height(1.0))
        .padding(Padding {
            top: 3.0,
            right: pad_x,
            bottom: 2.0,
            left: pad_x,
        })
        .width(width)
        .height(Length::Fixed(COUNT_MARK_H))
        .center_x(width)
        .center_y(Length::Fixed(COUNT_MARK_H))
        .style(count_mark_style)
        .into()
}

fn count_mark_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let accent = p.primary.base.color;
    container::Style {
        background: Some(primary_fill(accent)),
        text_color: Some(p.primary.base.text),
        border: Border {
            // Opaque mix of the canvas so iced does not inflate an alpha edge.
            color: mix_white(p.background.base.color, 0.14),
            width: 1.0,
            radius: RADIUS_PILL.into(),
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::count_label;

    #[test]
    fn count_label_caps_at_99_plus() {
        assert_eq!(count_label(1), "1");
        assert_eq!(count_label(99), "99");
        assert_eq!(count_label(100), "99+");
        assert_eq!(count_label(1400), "99+");
    }
}
