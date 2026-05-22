//! `card` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/card.{tsx,css}` and reference only
//! `--sola-card-*` scoped vars. Card is a panel with subtle surface
//! chrome used to group related rows in editor surfaces (Tokens
//! page, BindingsEditor).

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-secondary"));
    comp.slots.insert("border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("radius".into(), Binding::new("radius", "radius-lg"));
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-lg"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-xl"));
    comp
}
