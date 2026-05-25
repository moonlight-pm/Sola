//! Reusable iced widgets and styles.
//!
//! Grows as real apps need shared pieces. Each component lives in its
//! own submodule and exports a small public surface — usually one
//! factory function returning `Element<'a, Message>` plus named style
//! fns that mirror iced's own convention (`button::primary`,
//! `text::body`, etc.).
//!
//! Style fns take `&iced::Theme` (and sometimes `Status`) and read
//! from `theme.extended_palette()`. The atom→slot bindings live in
//! [`crate::theme::sola_extended`] — component code never references
//! `theme::hex::*` directly except for escape-hatch cases iced's
//! palette vocabulary can't carry.
//!
//! Iced's `row!`/`column!` macros stay the canonical layout primitives
//! — the kit doesn't ship a `stack` wrapper. Padded layouts use
//! `column![...].spacing(N).padding(M)` directly.

pub mod badge;
pub mod button;
pub mod card;
pub mod divider;
pub mod field;
pub mod icon;
pub mod popover;
pub mod sidebar;
pub mod split;
pub mod swatch;
pub mod text;
pub mod text_input;
pub mod toolbar;

pub use badge::{Tone, badge};
pub use card::card;
pub use divider::vertical_divider;
pub use field::field;
pub use icon::icon;
pub use icon::icon_colored;
pub use popover::popover;
pub use sidebar::{SIDEBAR_WIDTH, SidebarItem, SidebarSection, sidebar};
pub use split::split;
pub use swatch::{swatch, swatch_sized};
pub use toolbar::{toolbar_button, toolbar_button_msg};
