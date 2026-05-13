//! `popover` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/popover.{tsx,css}` and reference only
//! `--sola-popover-*` scoped vars. Popover is a floating panel
//! anchored to a trigger; the ColorInput's picker is its first
//! consumer.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-secondary"));
    comp.slots.insert("border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-md"));
    comp.slots.insert("padding".into(), Binding::new("space", "space-md"));
    comp.slots.insert("offset".into(), Binding::new("space", "space-xs"));
    comp
}
