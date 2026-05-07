//! `field` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/field.{tsx,css}` and reference only
//! `--sola-field-*` scoped vars. Field is the labeled wrapper used
//! around form controls (TextInput, ColorInput, …).

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("label-color".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("label-size".into(), Binding::new("text-size", "text-caption"));
    comp.slots.insert("gap".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("help-color".into(), Binding::new("text", "text-tertiary"));
    comp.slots.insert("error-color".into(), Binding::new("status", "danger"));
    comp
}
