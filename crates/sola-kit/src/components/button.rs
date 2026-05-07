//! `button` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/button.{tsx,css}` and reference only
//! `--sola-button-*` scoped vars.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Shape (variant-agnostic).
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-md"));
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-md"));
    comp.slots.insert("gap".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-body"));
    comp.slots.insert("focus-ring".into(), Binding::new("accent", "accent"));
    // Default variant.
    comp.slots.insert("default-bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("default-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("default-text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("default-border".into(), Binding::new("border", "border"));
    // Primary variant — saturated accent fill.
    comp.slots.insert("primary-bg".into(), Binding::new("accent", "accent"));
    comp.slots.insert("primary-text".into(), Binding::new("text", "text-primary"));
    // Ghost variant — transparent at rest, surface tint on hover.
    comp.slots.insert("ghost-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("ghost-text".into(), Binding::new("text", "text-secondary"));
    // Danger variant — saturated status fill.
    comp.slots.insert("danger-bg".into(), Binding::new("status", "danger"));
    comp.slots.insert("danger-text".into(), Binding::new("text", "text-primary"));
    comp
}
