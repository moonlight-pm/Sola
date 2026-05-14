//! Per-component editor metadata.
//!
//! Where `ComponentBindings.slots` is the *data* the bus delivers
//! (slot → group + token), `Category` adds *human structure* on top
//! for the bindings editor in the kit's showcase pages: which slots
//! belong together, what to call them in the UI, and what description
//! to surface as help text.
//!
//! Lives entirely kit-side — not part of the theme schema and not
//! serialised over the bus. The JS side fetches it via the
//! `list_categories` IPC command on demand.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SlotEntry {
    /// Key that matches a `ComponentBindings.slots` entry.
    pub key: String,
    /// User-facing label in the editor (e.g. "Background").
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    /// Stable ID for the category. Lower-kebab.
    pub id: String,
    /// User-facing label (e.g. "Surface", "Section title").
    pub label: String,
    /// Optional one-line description. Editor renders as muted help
    /// text under the category heading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Slots in this category, in display order.
    pub slots: Vec<SlotEntry>,
}

impl SlotEntry {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

impl Category {
    pub fn new(id: impl Into<String>, label: impl Into<String>, slots: Vec<SlotEntry>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            slots,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Look up the editor categories for a component by name. Returns
/// an empty vector if the component has no editor metadata yet — the
/// JS side renders nothing in that case rather than a flat slot
/// dump.
pub fn for_component(name: &str) -> Vec<Category> {
    match name {
        "button" => crate::components::button::categories(),
        "color-input" => crate::components::color_input::categories(),
        "field" => crate::components::field::categories(),
        "pane" => crate::components::pane::categories(),
        "popover" => crate::components::popover::categories(),
        "sidebar" => crate::components::sidebar::categories(),
        "swatch" => crate::components::swatch::categories(),
        "text" => crate::components::text::categories(),
        "text-input" => crate::components::text_input::categories(),
        _ => Vec::new(),
    }
}
