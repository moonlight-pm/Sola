//! `split` component bindings + editor categories. The Tsx and CSS
//! siblings live at `web/lib/components/split.{tsx,css}` and
//! reference only `--sola-split-*` scoped vars. Split is the kit's
//! two-child resizable layout primitive — the divider line between
//! the panes is the only themed surface; the panes themselves are
//! transparent.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("divider".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("divider-hover".into(), Binding::new("border", "border"));
    comp
}

pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "divider",
            "Divider",
            vec![
                SlotEntry::new("divider", "Line color"),
                SlotEntry::new("divider-hover", "Hover color"),
            ],
        )
        .with_description(
            "Thin line drawn between the two panes. Brightens on hover\
             and during a drag to cue draggability.",
        ),
    ]
}
