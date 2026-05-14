//! `text-input` component bindings + editor categories. The Tsx
//! and CSS siblings live at `web/lib/components/text-input.{tsx,
//! css}` and reference only `--sola-text-input-*` scoped vars.
//! (Component key is hyphenated; the Rust module name uses
//! underscores per Rust conventions.)

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("placeholder-color".into(), Binding::new("text", "text-muted"));
    comp.slots.insert("border".into(), Binding::new("border", "border"));
    comp.slots.insert("border-focus".into(), Binding::new("accent", "accent"));
    comp.slots.insert("border-invalid".into(), Binding::new("status", "danger"));
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-md"));
    // Inputs run denser than buttons — narrower vertical padding so
    // a Field row doesn't tower over its label.
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-body"));
    comp
}

pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "surface",
            "Surface",
            vec![
                SlotEntry::new("bg", "Background"),
                SlotEntry::new("border", "Border"),
                SlotEntry::new("border-focus", "Border (focus)"),
                SlotEntry::new("border-invalid", "Border (invalid)"),
                SlotEntry::new("radius", "Corner radius"),
            ],
        )
        .with_description("Background fill, outline, and the focus / invalid border swaps."),
        Category::new(
            "text",
            "Text",
            vec![
                SlotEntry::new("text", "Value color"),
                SlotEntry::new("placeholder-color", "Placeholder color"),
                SlotEntry::new("text-size", "Text size"),
            ],
        )
        .with_description("Typed-value tone, placeholder tone, and the input's font size."),
        Category::new(
            "padding",
            "Padding",
            vec![
                SlotEntry::new("padding-block", "Padding (vertical)"),
                SlotEntry::new("padding-inline", "Padding (horizontal)"),
            ],
        )
        .with_description("Inner spacing inside the box. Denser than buttons by default."),
    ]
}
