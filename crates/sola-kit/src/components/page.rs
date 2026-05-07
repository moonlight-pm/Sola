//! `page` component bindings — globals applied at `body { … }` so
//! children inherit `--sola-page-bg/text/font/text-size`. The DOM
//! sibling lives in `web/lib/base.css`.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-primary"));
    comp.slots.insert("text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("font".into(), Binding::new("font-family", "font-sans"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-body"));
    comp
}
