//! `bindings-editor` component bindings. Layout-only chrome — the
//! per-slot pickers delegate to `PopoverSelect` (which owns its own
//! chrome bindings) and the value editors flow through the
//! `TokenValueEditor` (which references its own component slots).
//! Only the row-label color lives here.

use sola_core::theme::{Binding, ComponentBindings};

pub fn bindings() -> ComponentBindings {
    let mut comp = ComponentBindings::default();
    comp.slots
        .insert("label-color".into(), Binding::new("text", "text-secondary"));
    comp
}
