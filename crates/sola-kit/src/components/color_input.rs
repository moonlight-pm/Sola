//! `color-input` component bindings + editor categories. The Tsx
//! and CSS siblings live at `web/lib/components/color-input.{tsx,
//! css}` and reference only `--sola-color-input-*` scoped vars.
//! Component key is hyphenated; the Rust module name uses
//! underscores per Rust conventions.
//!
//! ColorInput is a Swatch trigger that opens a ColorPicker popover.
//! Its only theme footprint is the swatch edge length.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Swatch edge length when used as the picker trigger. space-xxl
    // (24px) reads as "clickable affordance" without dominating the
    // surrounding form row.
    comp.slots.insert("swatch-size".into(), Binding::new("space", "space-xxl"));
    comp
}

/// Editor categories for ColorInput. The component is almost
/// chrome-free — its visual personality lives in the embedded
/// Swatch and ColorPicker — so the editor surfaces only the
/// trigger size.
pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "trigger",
            "Trigger",
            vec![SlotEntry::new("swatch-size", "Swatch edge length")],
        )
        .with_description(
            "Size of the clickable color preview that opens the picker.",
        ),
    ]
}
