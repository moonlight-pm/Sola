//! `field` component bindings + editor categories. The Tsx and
//! CSS siblings live at `web/lib/components/field.{tsx,css}` and
//! reference only `--sola-field-*` scoped vars. Field is the
//! labeled wrapper used around form controls (TextInput,
//! ColorInput, …).

use sola_core::theme::{Binding, ComponentBindings};

use crate::categories::{Category, SlotEntry};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("label-color".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("label-size".into(), Binding::new("text-size", "text-caption"));
    comp.slots.insert("gap".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("help-color".into(), Binding::new("text", "text-tertiary"));
    comp.slots.insert("error-color".into(), Binding::new("status", "danger"));
    comp
}

pub fn categories() -> Vec<Category> {
    vec![
        Category::new(
            "label",
            "Label",
            vec![
                SlotEntry::new("label-size", "Label size"),
                SlotEntry::new("label-color", "Label color"),
            ],
        )
        .with_description("The header above each form control."),
        Category::new(
            "layout",
            "Layout",
            vec![SlotEntry::new("gap", "Row gap")],
        )
        .with_description("Vertical gap between label, control, and help/error."),
        Category::new(
            "messages",
            "Help & error",
            vec![
                SlotEntry::new("help-color", "Help color"),
                SlotEntry::new("error-color", "Error color"),
            ],
        )
        .with_description(
            "Tones used for the help text below the control and the\
             error message that replaces it when `error` is set.",
        ),
    ]
}
