//! `bindings-editor` component bindings. Layout-only chrome (label
//! color, picker chrome); the actual value editors flow through the
//! `TokenValueEditor` which references its own component slots.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("label-color".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("picker-bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("picker-text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("picker-border".into(), Binding::new("border", "border"));
    comp.slots.insert("picker-border-focus".into(), Binding::new("accent", "accent"));
    comp.slots.insert("picker-option-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("picker-option-bg-selected".into(), Binding::new("surface", "bg-tertiary"));
    comp
}
