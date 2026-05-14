//! `container` component bindings + editor categories. The Tsx and
//! CSS siblings live at `web/lib/components/container.{tsx,css}` and
//! reference only `--sola-container-*` scoped vars. Container is the
//! centered max-width readable column used inside every kit app's
//! content area — the storybook's page body is the canonical example.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-xl"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-xxl"));
    comp
}

pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "padding",
            "Padding",
            vec![
                SlotEntry::new("padding-block", "Padding (vertical)"),
                SlotEntry::new("padding-inline", "Padding (horizontal)"),
            ],
        )
        .with_description(
            "Inner spacing around the readable column. Affects every\
             kit app page since Container is the standard content\
             wrapper.",
        ),
    ]
}
