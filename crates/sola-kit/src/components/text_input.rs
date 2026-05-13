//! `text-input` component bindings. The Tsx and CSS siblings live
//! at `web/lib/components/text-input.{tsx,css}` and reference only
//! `--sola-text-input-*` scoped vars. (Component key is hyphenated;
//! the Rust module name uses underscores per Rust conventions.)

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("placeholder-color".into(), Binding::new("text", "text-muted"));
    comp.slots.insert("border".into(), Binding::new("border", "border"));
    comp.slots.insert("border-focus".into(), Binding::new("accent", "accent"));
    comp.slots.insert("border-invalid".into(), Binding::new("status", "danger"));
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-md"));
    // Inputs run denser than buttons — narrower vertical padding so
    // a Field row doesn't tower over its label.
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-body"));
    comp
}
