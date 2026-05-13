//! `sidebar` component bindings + editor categories. The Tsx and
//! CSS siblings live at `web/lib/components/sidebar.{tsx,css}` and
//! reference only `--sola-sidebar-*` scoped vars.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-secondary"));
    comp.slots.insert("border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("section-label-color".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("section-label-size".into(), Binding::new("text-size", "text-caption"));
    comp.slots.insert("item-text-idle".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("item-text-active".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("item-text-size".into(), Binding::new("text-size", "text-body"));
    comp.slots.insert("item-icon-idle".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("item-icon-active".into(), Binding::new("accent", "accent"));
    comp.slots.insert("item-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("item-bg-active".into(), Binding::new("accent-tint", "accent-dim"));
    comp.slots.insert("item-stripe".into(), Binding::new("accent", "accent"));
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-md"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("item-padding-block".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("item-padding-inline".into(), Binding::new("space", "space-md"));
    comp.slots.insert("gap".into(), Binding::new("space", "space-xs"));
    comp
}

/// Editor categories for the Sidebar component. Used by the
/// bindings editor on the Sidebar showcase page — see
/// `crates/sola-kit/src/categories.rs` for the data shape and how
/// it's served to the renderer.
pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "surface",
            "Surface",
            vec![
                SlotEntry::new("bg", "Background"),
                SlotEntry::new("border", "Right border"),
            ],
        )
        .with_description("Fill and the vertical separator anchoring the sidebar to the work area."),
        Category::new(
            "layout",
            "Layout",
            vec![
                SlotEntry::new("padding-block", "Outer padding (vertical)"),
                SlotEntry::new("padding-inline", "Outer padding (horizontal)"),
                SlotEntry::new("gap", "Item gap"),
            ],
        )
        .with_description("Outer padding around the rail and the vertical gap between items / titles."),
        Category::new(
            "section-title",
            "Section title",
            vec![
                SlotEntry::new("section-label-size", "Title size"),
                SlotEntry::new("section-label-color", "Title color"),
            ],
        )
        .with_description("Small all-caps headers grouping nav items."),
        Category::new(
            "item",
            "Item",
            vec![
                SlotEntry::new("item-text-size", "Label size"),
                SlotEntry::new("item-text-idle", "Label (idle)"),
                SlotEntry::new("item-text-active", "Label (active)"),
                SlotEntry::new("item-icon-idle", "Icon (idle)"),
                SlotEntry::new("item-icon-active", "Icon (active)"),
                SlotEntry::new("item-bg-hover", "Hover background"),
                SlotEntry::new("item-bg-active", "Active background"),
                SlotEntry::new("item-stripe", "Active stripe"),
                SlotEntry::new("item-padding-block", "Item padding (vertical)"),
                SlotEntry::new("item-padding-inline", "Item padding (horizontal)"),
            ],
        )
        .with_description("Typography and surface tints applied to individual nav items."),
    ]
}
