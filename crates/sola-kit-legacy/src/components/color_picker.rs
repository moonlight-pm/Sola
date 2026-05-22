//! `color-picker` component bindings. The Tsx and CSS siblings
//! live at `web/lib/components/color-picker.{tsx,css}` and
//! reference only `--sola-color-picker-*` scoped vars. ColorPicker
//! is the HSL+alpha panel an editable Swatch shows inside its
//! Popover when the swatch is clicked.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("gap".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("preview-height".into(), Binding::new("space", "space-xxl"));
    comp.slots.insert("preview-border".into(), Binding::new("border", "border-subtle"));
    comp.slots.insert("preview-radius".into(), Binding::new("radius", "radius-sm"));
    comp.slots.insert("slider-track-bg".into(), Binding::new("surface", "bg-primary"));
    comp.slots.insert("slider-thumb-bg".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("slider-thumb-border".into(), Binding::new("surface", "bg-primary"));
    comp.slots.insert("slider-thumb-shadow".into(), Binding::new("border", "border"));
    comp.slots.insert("label-color".into(), Binding::new("text", "text-secondary"));
    comp.slots.insert("label-size".into(), Binding::new("text-size", "text-caption"));
    comp.slots.insert("value-color".into(), Binding::new("text", "text-tertiary"));
    comp
}
