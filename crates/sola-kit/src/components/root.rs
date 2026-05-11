//! `root` component bindings. The Tsx and CSS siblings live at
//! `web/lib/components/root.{tsx,css}` and reference only
//! `--sola-root-*` scoped vars. Root is the top-of-tree wrapper
//! every kit app's `Main` should return; its slots set the page
//! background, text color, font family, base text size — and the
//! scrollbar styling that descendants inherit via `--sola-root-*`
//! references in their own CSS.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    // Page-level look.
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-primary"));
    comp.slots.insert("text".into(), Binding::new("text", "text-primary"));
    comp.slots.insert("font".into(), Binding::new("font-family", "font-sans"));
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-body"));
    // Scrollbars. Track blends with the page bg; idle thumb is a
    // subtle border tone; hover thumb is the brighter `text-muted`
    // (now dual-group for exactly this kind of usage).
    comp.slots.insert("scrollbar-size".into(), Binding::new("space", "space-sm"));
    comp.slots.insert("scrollbar-track".into(), Binding::new("surface", "bg-primary"));
    comp.slots.insert("scrollbar-thumb".into(), Binding::new("border", "border"));
    comp.slots.insert("scrollbar-thumb-hover".into(), Binding::new("border", "text-muted"));
    comp
}
