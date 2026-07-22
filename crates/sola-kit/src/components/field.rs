//! Field — labelled form row. A `column!` of label + input + optional
//! help text with kit-standard spacing.
//!
//! The legacy kit's `field.tsx` lived inside its design-token editor
//! and used Remix's slot composition. Here it's a function that
//! takes the label and the input element; the caller assembles the
//! input via iced's standard `text_input(...)` (or any other widget).

use iced::widget::{Container, column, container};
use iced::{Element, Length, Theme};

use crate::components::style::SPACE_SM;
use crate::components::text::{body, caption, muted};

/// Wrap an input with a label above and optional help text below.
/// Returns a `Container` (defaulting to full width, overridable via
/// `.width(..)`) so callers can fold it into wider layouts.
///
/// Label is body (13) + muted — macOS form-label weight, not caption.
/// Help stays caption + muted.
///
/// ```ignore
/// field("Username",
///     text_input("alice", &state.username).on_input(Msg::Username),
///     Some("3–20 characters")
/// )
/// ```
pub fn field<'a, Message: 'a>(
    label: impl Into<String>,
    input: impl Into<Element<'a, Message, Theme>>,
    help: Option<&'a str>,
) -> Container<'a, Message, Theme> {
    let label_el = body(label.into()).style(muted);
    let mut col = column![label_el, input.into()].spacing(SPACE_SM);
    if let Some(help_text) = help {
        col = col.push(caption(help_text).style(muted));
    }
    container(col).width(Length::Fill)
}
