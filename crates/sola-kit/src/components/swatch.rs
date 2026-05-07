//! `swatch` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/swatch.{tsx,css}` and reference only
//! `--sola-swatch-*` scoped vars. The transparency-checker pattern
//! is hardcoded in CSS (structural visual, not a brand element); a
//! future light theme can mint a `checker-color` slot if needed.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-sm"));
    comp
}
