//! Reusable iced widgets and styles.
//!
//! Grows as real apps need shared pieces. Each component lives in
//! its own submodule and exports a small public surface — usually
//! one factory function returning `Element<'a, Message>` and any
//! related style helpers.
//!
//! Naming convention: lowercase function names that match iced's own
//! widget-constructor style (`button`, `row`, …). Avoid wrapping
//! constructors in structs unless the component carries non-trivial
//! state.

pub mod divider;
pub mod toolbar;

pub use divider::{vertical_divider, divider_style};
pub use toolbar::toolbar_button;
