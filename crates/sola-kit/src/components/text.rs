//! Typography helpers — kit-flavored variants of `iced::widget::text`.
//!
//! Each helper returns an `iced::widget::Text` so callers can chain
//! further iced methods (`.width(Fill)`, `.color(...)`, etc.). The
//! style fns (`muted`, `accent`, `danger`, ...) match iced's own
//! `text::primary` / `text::warning` shape and are passed via
//! `.style(sola_kit::components::text::muted)`.

use iced::widget::text::{self, Text};
use iced::{Theme, widget::text as iced_text};

use crate::fonts;

/// 24px condensed-bold — page titles and dialog headers.
pub fn heading<'a>(content: impl text::IntoFragment<'a>) -> Text<'a, Theme> {
    iced_text(content).font(fonts::CONDENSED_BOLD).size(24)
}

/// 18px condensed-bold — section dividers within a page.
pub fn subheading<'a>(content: impl text::IntoFragment<'a>) -> Text<'a, Theme> {
    iced_text(content).font(fonts::CONDENSED_BOLD).size(18)
}

/// 14px normal — default body text. Use plain `iced::widget::text` if
/// you need a different size; this is the canonical body size only.
pub fn body<'a>(content: impl text::IntoFragment<'a>) -> Text<'a, Theme> {
    iced_text(content).font(fonts::NORMAL).size(14)
}

/// 11px normal — timestamps, secondary labels, helper copy. Pair with
/// [`muted`] when the caption should also deemphasize visually.
pub fn caption<'a>(content: impl text::IntoFragment<'a>) -> Text<'a, Theme> {
    iced_text(content).font(fonts::NORMAL).size(11)
}

/// 12px JetBrains Mono — inline code, hex values, JSON snippets.
pub fn code<'a>(content: impl text::IntoFragment<'a>) -> Text<'a, Theme> {
    iced_text(content).font(fonts::MONO).size(12)
}

/// Muted variant — lower-contrast text for timestamps, captions, and
/// deemphasized chrome. Pulls `secondary.base.text` which is bound to
/// `FG_MUTED` in [`crate::theme::sola_extended`].
pub fn muted(theme: &Theme) -> text::Style {
    text::Style { color: Some(theme.extended_palette().secondary.base.text) }
}

/// Accent-colored text — links, active selections, called-out values.
pub fn accent(theme: &Theme) -> text::Style {
    text::Style { color: Some(theme.extended_palette().primary.base.color) }
}

/// Success-colored — confirmation messages, "ok" status pills.
pub fn success(theme: &Theme) -> text::Style {
    text::Style { color: Some(theme.extended_palette().success.base.color) }
}

/// Warning-colored — soft "heads up" copy, non-blocking issues.
pub fn warning(theme: &Theme) -> text::Style {
    text::Style { color: Some(theme.extended_palette().warning.base.color) }
}

/// Danger-colored — error messages, destructive-action labels.
pub fn danger(theme: &Theme) -> text::Style {
    text::Style { color: Some(theme.extended_palette().danger.base.color) }
}
