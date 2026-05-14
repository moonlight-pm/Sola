//! `swatch` component bindings + editor categories. The Tsx and
//! CSS siblings live at `web/lib/components/swatch.{tsx,css}` and
//! reference only `--sola-swatch-*` scoped vars. The transparency-
//! checker pattern is hardcoded in CSS (structural visual, not a
//! brand element); a future light theme can mint a `checker-color`
//! slot if needed.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-sm"));
    comp
}

pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "chrome",
            "Chrome",
            vec![
                SlotEntry::new("border", "Border"),
                SlotEntry::new("radius", "Corner radius"),
            ],
        )
        .with_description(
            "Outline + rounding wrapped around the color fill. Affects\
             every consumer (ColorInput trigger, palette displays, etc).",
        ),
    ]
}
