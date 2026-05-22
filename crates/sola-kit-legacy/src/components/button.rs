//! `button` component bindings + editor categories. The Tsx and
//! CSS siblings live at `web/lib/components/button.{tsx,css}` and
//! reference only `--sola-button-*` scoped vars.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Shape (variant-agnostic).
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-md"));
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-md"));
    comp.slots.insert("gap".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-body"));
    comp.slots.insert("focus-ring".into(), Binding::new("accent", "accent"));
    // Default variant.
    comp.slots.insert("default-bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("default-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("default-text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("default-border".into(), Binding::new("border", "border"));
    // Primary variant — saturated accent fill.
    comp.slots.insert("primary-bg".into(), Binding::new("accent", "accent"));
    comp.slots.insert("primary-text".into(), Binding::new("text", "text-primary"));
    // Ghost variant — transparent at rest, surface tint on hover.
    comp.slots.insert("ghost-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("ghost-text".into(), Binding::new("text", "text-secondary"));
    // Danger variant — saturated status fill.
    comp.slots.insert("danger-bg".into(), Binding::new("status", "danger"));
    comp.slots.insert("danger-text".into(), Binding::new("text", "text-primary"));
    comp
}

/// Editor categories for the Button component. Variant-agnostic
/// shape slots first, then one category per variant — the same
/// grouping the CSS file uses (Shape → Default → Primary → Ghost →
/// Danger).
pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "shape",
            "Shape",
            vec![
                SlotEntry::new("radius", "Corner radius"),
                SlotEntry::new("padding-block", "Padding (vertical)"),
                SlotEntry::new("padding-inline", "Padding (horizontal)"),
                SlotEntry::new("gap", "Icon / label gap"),
                SlotEntry::new("text-size", "Label size"),
                SlotEntry::new("focus-ring", "Focus ring color"),
            ],
        )
        .with_description("Geometry and typography shared by every variant."),
        Category::new(
            "default",
            "Default variant",
            vec![
                SlotEntry::new("default-bg", "Background"),
                SlotEntry::new("default-bg-hover", "Hover background"),
                SlotEntry::new("default-text", "Label"),
                SlotEntry::new("default-border", "Border"),
            ],
        )
        .with_description("Neutral filled button — the everyday call-to-action."),
        Category::new(
            "primary",
            "Primary variant",
            vec![
                SlotEntry::new("primary-bg", "Background"),
                SlotEntry::new("primary-text", "Label"),
            ],
        )
        .with_description("Saturated accent fill for the leading action on a page."),
        Category::new(
            "ghost",
            "Ghost variant",
            vec![
                SlotEntry::new("ghost-text", "Label"),
                SlotEntry::new("ghost-bg-hover", "Hover background"),
            ],
        )
        .with_description("Chromeless at rest; surface tint appears on hover."),
        Category::new(
            "danger",
            "Danger variant",
            vec![
                SlotEntry::new("danger-bg", "Background"),
                SlotEntry::new("danger-text", "Label"),
            ],
        )
        .with_description("Destructive action — saturated status fill."),
    ]
}
