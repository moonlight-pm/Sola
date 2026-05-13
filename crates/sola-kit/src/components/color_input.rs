//! `color-input` component bindings. The Tsx and CSS siblings live
//! at `web/lib/components/color-input.{tsx,css}` and reference only
//! `--sola-color-input-*` scoped vars. Component key is hyphenated;
//! the Rust module name uses underscores per Rust conventions.
//!
//! ColorInput is a Swatch trigger that opens a ColorPicker popover.
//! Its only theme footprint is the swatch edge length.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Swatch edge length when used as the picker trigger. space-xxl
    // (24px) reads as "clickable affordance" without dominating the
    // surrounding form row.
    comp.slots.insert("swatch-size".into(), Binding::new("space", "space-xxl"));
    comp
}
