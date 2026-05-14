//! `text` component bindings + editor categories. The Tsx and CSS
//! siblings live at `web/lib/components/text.{tsx,css}` and
//! reference only `--sola-text-*` scoped vars. Text is the
//! typography primitive: one slot per `kind` (display / heading /
//! body-lg / body / caption / label) drives the font-size, and two
//! tone slots provide muted and subtle color treatments layered on
//! top.

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Sizes — one per kind, mapped to the palette's text-size atoms.
    comp.slots.insert("display-size".into(), Binding::new("text-size", "text-display"));
    comp.slots.insert("heading-size".into(), Binding::new("text-size", "text-heading"));
    comp.slots.insert("body-lg-size".into(), Binding::new("text-size", "text-body-lg"));
    comp.slots.insert("body-size".into(), Binding::new("text-size", "text-body"));
    comp.slots.insert("caption-size".into(), Binding::new("text-size", "text-caption"));
    // Label re-uses the smallest size; its uppercase + letter-spacing
    // styling is what distinguishes it visually.
    comp.slots.insert("label-size".into(), Binding::new("text-size", "text-caption"));
    // Label color — secondary so labels read as headers without
    // shouting; matches Field's own label-color choice.
    comp.slots.insert("label-color".into(), Binding::new("text", "text-secondary"));
    // Tone colors — overlaid on `kind` via class composition.
    comp.slots.insert("muted-color".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("subtle-color".into(), Binding::new("text", "text-tertiary"));
    comp
}

pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "sizes",
            "Sizes",
            vec![
                SlotEntry::new("display-size", "Display"),
                SlotEntry::new("heading-size", "Heading"),
                SlotEntry::new("body-lg-size", "Body (large)"),
                SlotEntry::new("body-size", "Body"),
                SlotEntry::new("caption-size", "Caption"),
                SlotEntry::new("label-size", "Label"),
            ],
        )
        .with_description("One slot per `kind` — picks the font-size token used by that variant."),
        Category::new(
            "tone",
            "Tone & label",
            vec![
                SlotEntry::new("label-color", "Label color"),
                SlotEntry::new("muted-color", "Muted tone"),
                SlotEntry::new("subtle-color", "Subtle tone"),
            ],
        )
        .with_description(
            "Label color is dedicated to `kind=\"label\"`; muted and\
             subtle tones overlay any kind via the `tone` prop.",
        ),
    ]
}
