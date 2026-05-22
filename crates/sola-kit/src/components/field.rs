//! Field — labelled form row. A `column!` of label + input + optional
//! help text with kit-standard spacing.
//!
//! The legacy kit's `field.tsx` lived inside its design-token editor
//! and used Remix's slot composition. Here it's a function that
//! takes the label and the input element; the caller assembles the
//! input via iced's standard `text_input(...)` (or any other widget).

use iced::widget::{column, container};
use iced::{Element, Length, Theme};

use crate::components::text::{caption, muted};

/// Wrap an input with a label above and optional help text below.
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
) -> Element<'a, Message, Theme> {
    let label_el = caption(label.into()).style(muted);
    let mut col = column![label_el, input.into()].spacing(4);
    if let Some(help_text) = help {
        col = col.push(caption(help_text).style(muted));
    }
    container(col).width(Length::Fill).into()
}
