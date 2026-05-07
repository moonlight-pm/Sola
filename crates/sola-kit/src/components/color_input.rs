//! `color-input` component bindings. The Tsx and CSS siblings live
//! at `web/lib/components/color-input.{tsx,css}` and reference only
//! `--sola-color-input-*` scoped vars. Component key is hyphenated;
//! the Rust module name uses underscores per Rust conventions.
//!
//! ColorInput is a thin composition (Swatch + TextInput) — its own
//! theme footprint is just spacing.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("gap".into(), Binding::new("space", "space-xs"));
    // Leading swatch edge length. space-xxl (24px) sits well next
    // to the default-themed TextInput (~28px tall) without towering
    // over it. Exposed as a slot so theme tweaks (larger text-size,
    // denser padding) can rebalance the visual.
    comp.slots.insert("swatch-size".into(), Binding::new("space", "space-xxl"));
    comp
}
