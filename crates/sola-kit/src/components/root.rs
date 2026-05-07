//! `root` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/root.{tsx,css}` and reference only
//! `--sola-root-*` scoped vars. Root is the top-of-tree wrapper
//! every kit app's `Main` should return; its slots set the page
//! background, text color, font family, and base text size.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-primary"));
    comp.slots.insert("text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("font".into(), Binding::new("font-family", "font-sans"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-body"));
    comp
}
