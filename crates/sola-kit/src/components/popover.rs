//! `popover` component bindings + editor categories. The Tsx and
//! CSS siblings live at `web/lib/components/popover.{tsx,css}` and
//! reference only `--sola-popover-*` scoped vars. Popover is a
//! floating panel anchored to a trigger; editable Swatch's
//! ColorPicker and every PopoverSelect dropdown inherit its
//! surface chrome.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-secondary"));
    comp.slots.insert("border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-md"));
    comp.slots.insert("padding".into(), Binding::new("space", "space-md"));
    comp.slots.insert("offset".into(), Binding::new("space", "space-xs"));
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
                SlotEntry::new("radius", "Corner radius"),
            ],
        )
        .with_description("Panel chrome — fill, outline, and rounding."),
        Category::new(
            "layout",
            "Layout",
            vec![
                SlotEntry::new("padding", "Inner padding"),
                SlotEntry::new("offset", "Trigger gap"),
            ],
        )
        .with_description(
            "Inner padding around the panel content, and the gap between\
             the panel and its trigger so they don't visually merge.",
        ),
    ]
}
