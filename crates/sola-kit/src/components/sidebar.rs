//! `sidebar` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/sidebar.{tsx,css}` and reference only
//! `--sola-sidebar-*` scoped vars.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-secondary"));
    comp.slots.insert("border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("section-label-color".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("section-label-size".into(), Binding::new("text-size", "text-caption"));
    comp.slots.insert("item-text-idle".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("item-text-active".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("item-text-size".into(), Binding::new("text-size", "text-body"));
    comp.slots.insert("item-icon-idle".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("item-icon-active".into(), Binding::new("accent", "accent"));
    comp.slots.insert("item-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("item-bg-active".into(), Binding::new("accent-tint", "accent-dim"));
    comp.slots.insert("item-stripe".into(), Binding::new("accent", "accent"));
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-md"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("item-padding-block".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("item-padding-inline".into(), Binding::new("space", "space-md"));
    comp.slots.insert("gap".into(), Binding::new("space", "space-xs"));
    comp
}
