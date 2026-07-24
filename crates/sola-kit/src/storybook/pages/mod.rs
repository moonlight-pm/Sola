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
//!
//! Not every component gets its own page: `swatch` is dogfooded by the
//! Theme page's atom grid + color picker, and `text_input` by the Field
//! page's inputs. They're intentionally folded there rather than given
//! standalone pages.

pub mod badge;
pub mod button;
pub mod card;
pub mod color_picker;
pub mod divider;
pub mod field;
pub mod form;
pub mod icon;
pub mod number_input;
pub mod overview;
pub mod popover;
pub mod shell;
pub mod readable;
pub mod sidebar;
pub mod split;
pub mod text;
pub mod theme;
pub mod titlebar;
pub mod toolbar;
