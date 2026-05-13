//! `number-input` component bindings. The Tsx and CSS siblings live
//! at `web/lib/components/number-input.{tsx,css}` and reference only
//! `--sola-number-input-*` scoped vars. Component key is hyphenated;
//! the Rust module name uses underscores per Rust conventions.
//!
//! NumberInput is a numeric editor with a trailing unit hint and
//! −/+ step buttons. Used by the Tokens page for TextSize / Space /
//! Radius tokens, and exposed to any app that needs a stepper-style
//! number input.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Body chrome mirrors the TextInput slot scheme so a row mixing
    // TextInput, ColorInput, and NumberInput reads as one density.
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("border".into(), Binding::new("border", "border"));
    comp.slots.insert("border-focus".into(), Binding::new("accent", "accent"));
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-md"));
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-sm"));
    // Unit hint is muted so it reads as part of the chrome.
    comp.slots.insert("unit-color".into(), Binding::new("text", "text-secondary"));
    // Step buttons share the muted icon stroke; hover/active tints
    // reuse the surface palette.
    comp.slots.insert("step-color".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("step-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("step-bg-active".into(), Binding::new("surface", "bg-secondary"));
    comp
}
