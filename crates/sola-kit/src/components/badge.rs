//! `badge` component bindings + editor categories. The Tsx and
//! CSS siblings live at `web/lib/components/badge.{tsx,css}` and
//! reference only `--sola-badge-*` scoped vars. Shape slots
//! (radius, padding, text size) are shared across kinds; bg/text
//! vary per kind.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Shape (kind-agnostic).
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-sm"));
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-caption"));
    // Neutral kind — subtle surface tint.
    comp.slots.insert("neutral-bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("neutral-text".into(), Binding::new("text", "text-secondary"));
    // Info kind — accent-tinted.
    comp.slots.insert("info-bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("info-text".into(), Binding::new("text", "text-accent"));
    // Success kind — saturated status fill.
    comp.slots.insert("success-bg".into(), Binding::new("status", "success"));
    comp.slots.insert("success-text".into(), Binding::new("text", "text-primary"));
    // Warning kind — uses danger for visibility (no dedicated warning atom yet).
    comp.slots.insert("warning-bg".into(), Binding::new("status", "danger"));
    comp.slots.insert("warning-text".into(), Binding::new("text", "text-primary"));
    // Danger kind — saturated status fill.
    comp.slots.insert("danger-bg".into(), Binding::new("status", "danger"));
    comp.slots.insert("danger-text".into(), Binding::new("text", "text-primary"));
    comp
}

pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "shape",
            "Shape",
            vec![
                SlotEntry::new("radius", "Corner radius"),
                SlotEntry::new("padding-block", "Padding (vertical)"),
                SlotEntry::new("padding-inline", "Padding (horizontal)"),
                SlotEntry::new("text-size", "Label size"),
            ],
        )
        .with_description("Geometry shared by every kind."),
        Category::new(
            "neutral",
            "Neutral kind",
            vec![
                SlotEntry::new("neutral-bg", "Background"),
                SlotEntry::new("neutral-text", "Label"),
            ],
        )
        .with_description("Default unobtrusive variant."),
        Category::new(
            "info",
            "Info kind",
            vec![
                SlotEntry::new("info-bg", "Background"),
                SlotEntry::new("info-text", "Label"),
            ],
        )
        .with_description("Accent-tinted variant for neutral informational status."),
        Category::new(
            "success",
            "Success kind",
            vec![
                SlotEntry::new("success-bg", "Background"),
                SlotEntry::new("success-text", "Label"),
            ],
        )
        .with_description("Positive status (saved, validated, online)."),
        Category::new(
            "warning",
            "Warning kind",
            vec![
                SlotEntry::new("warning-bg", "Background"),
                SlotEntry::new("warning-text", "Label"),
            ],
        )
        .with_description("Caution status (missing, deprecated, attention needed)."),
        Category::new(
            "danger",
            "Danger kind",
            vec![
                SlotEntry::new("danger-bg", "Background"),
                SlotEntry::new("danger-text", "Label"),
            ],
        )
        .with_description("Error status (broken, unavailable, critical)."),
    ]
}
