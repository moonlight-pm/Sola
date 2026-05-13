//! `pane` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/pane.{tsx,css}` and reference only
//! `--sola-pane-*` scoped vars. Pane is the scrollable padded
//! content area used inside every kit app page — the storybook's
//! content column is the canonical example.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("padding-block".into(), Binding::new("space", "space-xl"));
    comp.slots.insert("padding-inline".into(), Binding::new("space", "space-xxl"));
    comp
}
