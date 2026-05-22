//! `popover-select` component bindings. The Tsx and CSS siblings
//! live at `web/lib/components/popover-select.{tsx,css}` and
//! reference only `--sola-popover-select-*` scoped vars.
//!
//! PopoverSelect is a typed dropdown that mirrors native HTML
//! `<select>` sizing semantics via @chenglou/pretext measurement.
//! See the Tsx header for the sizing model and prop API.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-tertiary"));
    comp.slots.insert("text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("border".into(), Binding::new("border", "border"));
    comp.slots.insert("border-focus".into(), Binding::new("accent", "accent"));
    comp.slots
        .insert("chevron-color".into(), Binding::new("text", "text-secondary"));
    comp.slots
        .insert("option-text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert(
        "option-bg-hover".into(),
        Binding::new("surface", "bg-hover"),
    );
    comp.slots.insert(
        "option-bg-selected".into(),
        Binding::new("surface", "bg-tertiary"),
    );
    comp
}
