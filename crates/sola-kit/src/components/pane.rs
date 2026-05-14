//! `pane` component bindings + editor categories. The Tsx and CSS
//! siblings live at `web/lib/components/pane.{tsx,css}` and
//! reference only `--sola-pane-*` scoped vars. Pane is the
//! scrollable padded content area used inside every kit app page —
//! the storybook's content column is the canonical example.

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
            "Inner spacing around the scrollable content. Affects every\
             kit app page since Pane is the standard content surface.",
        ),
    ]
}
