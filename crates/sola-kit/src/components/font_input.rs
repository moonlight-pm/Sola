//! `font-input` component bindings. The Tsx and CSS siblings live
//! at `web/lib/components/font-input.{tsx,css}` and reference only
//! `--sola-font-input-*` scoped vars. Component key is hyphenated;
//! the Rust module name uses underscores per Rust conventions.
//!
//! FontInput is a trigger that opens a popover with a searchable
//! list of installed font families (each rendered in its own font
//! for instant recognition). Mirrors the editable Swatch →
//! ColorPicker pattern but for typography.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Trigger button chrome — surface tone + subtle border so it
    // reads as a clickable affordance without competing with the
    // sliders/swatches sharing the same row.
    comp.slots.insert("trigger-bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("trigger-border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("trigger-text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("trigger-radius".into(), Binding::new("radius", "radius-md"));
    comp.slots.insert("trigger-padding-block".into(), Binding::new("space", "space-xs"));
    comp.slots.insert("trigger-padding-inline".into(), Binding::new("space", "space-sm"));
    // Option row in the list — hover/selected tints reuse the
    // existing surface palette so the picker matches the rest of
    // the kit's interactive feedback.
    comp.slots.insert("option-text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("option-bg-hover".into(), Binding::new("surface", "bg-hover"));
    comp.slots.insert("option-bg-selected".into(), Binding::new("surface", "bg-tertiary"));
    // Secondary chrome — search placeholder + chevron / refresh
    // icon strokes.
    comp.slots.insert("muted-text".into(), Binding::new("text", "text-secondary"));
    comp
}
