//! Rust-side per-component metadata for kit-shipped UI components.
//! Each submodule owns one component's default theme bindings; the
//! frontend (Tsx/CSS) lives under `web/lib/components/<name>.*` and
//! is registered in `assets::platform_assets()`. Every entry here
//! has a real DOM-rendering counterpart.
//!
//! Adding a new kit component is a single-crate change:
//!   1. Drop `web/lib/components/<name>.{tsx,css}`.
//!   2. Add `<name>.rs` here exposing `pub fn bindings() ->
//!      ComponentBindings`.
//!   3. Register the Tsx/CSS in `assets.rs::platform_assets`, the
//!      .tsx in `lib.rs::build_importmap`, and `<name>` in
//!      `all_bindings()` below.

use std::collections::BTreeMap;

use sola_core::theme::ComponentBindings;

pub mod button;
pub mod color_input;
pub mod color_picker;
pub mod field;
pub mod font_input;
pub mod number_input;
pub mod pane;
pub mod popover;
pub mod root;
pub mod sidebar;
pub mod swatch;
pub mod text;
pub mod text_input;

/// Compose every kit-shipped component's seed bindings into the
/// `Theme.components` map shape. Keys are the component names used as
/// `--sola-<component>-<slot>` prefixes in the rendered CSS.
///
/// Note: keys may use kebab-case (e.g. `"text-input"`) even when the
/// Rust module name is snake_case — the key is what appears in CSS.
pub fn all_bindings() -> BTreeMap<String, ComponentBindings> {
    let mut map = BTreeMap::new();
    map.insert("button".into(), button::bindings());
    map.insert("color-input".into(), color_input::bindings());
    map.insert("color-picker".into(), color_picker::bindings());
    map.insert("field".into(), field::bindings());
    map.insert("font-input".into(), font_input::bindings());
    map.insert("number-input".into(), number_input::bindings());
    map.insert("pane".into(), pane::bindings());
    map.insert("popover".into(), popover::bindings());
    map.insert("root".into(), root::bindings());
    map.insert("sidebar".into(), sidebar::bindings());
    map.insert("swatch".into(), swatch::bindings());
    map.insert("text".into(), text::bindings());
    map.insert("text-input".into(), text_input::bindings());
    map
}
