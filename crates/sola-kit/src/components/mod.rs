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
pub mod root;
pub mod sidebar;

/// Compose every kit-shipped component's seed bindings into the
/// `Theme.components` map shape. Keys are the component names used as
/// `--sola-<component>-<slot>` prefixes in the rendered CSS.
pub fn all_bindings() -> BTreeMap<String, ComponentBindings> {
    let mut map = BTreeMap::new();
    map.insert("button".into(), button::bindings());
    map.insert("root".into(), root::bindings());
    map.insert("sidebar".into(), sidebar::bindings());
    map
}
