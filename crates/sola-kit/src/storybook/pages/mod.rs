//! Storybook pages — one per kit-shipped component (or concept).
//!
//! Convention: each module exports either
//! - `pub fn view() -> Element<'_, Msg>` for stateless pages, or
//! - `pub struct State` + `pub enum Msg` + `pub fn view(state) -> Element<'_, Msg>`
//!   when the showcase needs interaction.
//!
//! The parent `Storybook` is responsible for routing the page's `Msg`
//! into the shell's outer message via `.map(Msg::PageName)`. We keep
//! one message variant per stateful page; it's verbose but each page's
//! state is independent and the indirection helps when pages get
//! retired or reordered.

pub mod badge;
pub mod button;
pub mod card;
pub mod divider;
pub mod field;
pub mod popover;
pub mod sidebar;
pub mod split;
pub mod text;
pub mod theme;
pub mod toolbar;
