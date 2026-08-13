//! Reusable iced widgets and styles.
//!
//! Grows as real apps need shared pieces. Each component lives in its
//! own submodule and exports a small public surface — usually one
//! factory function plus named style fns that mirror iced's own
//! convention (`button::primary`, `text::body`, etc.).
//!
//! ## Conventions
//!
//! - **Return types:** container-shaped builders that callers may want
//!   to size or position (`card`, `popover`, `sidebar`, `field`) return
//!   an `iced::widget::Container` so the caller can chain `.width(..)`
//!   etc.; terminal/leaf widgets (`badge`, `swatch`, `vertical_divider`,
//!   `icon`) return an `Element`.
//! - **`Element` spelling:** public signatures write the theme param
//!   explicitly — `Element<'a, Message, Theme>` — rather than relying on
//!   the default, so the kit reads uniformly.
//! - **Re-exports:** the top-level *widget factory* fns are re-exported
//!   flat (`components::card`, `components::badge`, …). The *style-fn
//!   families* stay namespaced under their module (`button::primary`,
//!   `text::body`, `text_input::style`) — flattening them would collide
//!   (`button::danger` vs `text::danger` vs `badge::Tone::Danger`), so
//!   callers reach those through the module path on purpose.
//! - **Shared primitives:** common style fragments (`filled`, `hairline`,
//!   `dim`) and the `RADIUS_*` / `SPACE_*` scales live in [`style`].
//!
//! Style fns take `&iced::Theme` (and sometimes `Status`) and read
//! from `theme.extended_palette()`. The atom→slot bindings live in
//! [`crate::theme::build_theme`] — component code never references
//! `theme::hex::*` directly except for escape-hatch cases iced's
//! palette vocabulary can't carry (e.g. the popover drop shadow).
//!
//! Iced's `row!`/`column!` macros stay the canonical layout primitives
//! — the kit doesn't ship a `stack` wrapper. Padded layouts use
//! `column![...].spacing(N).padding(M)` directly.

pub mod badge;
pub mod button;
pub mod card;
pub mod color_picker;
pub mod divider;
pub mod field;
pub mod form;
pub mod icon;
pub mod number_input;
pub mod popover;
pub mod readable;
pub mod select;
pub mod sidebar;
pub mod spectrum;
pub mod split;
pub mod style;
pub mod swatch;
pub mod text;
pub mod text_input;
pub mod titlebar;
pub mod toolbar;

pub use badge::{Tone, badge};
pub use button::confirm_button;
pub use card::{accent_backplate, backplate, card, modal, plain};
pub use color_picker::ColorPicker;
pub use divider::{
    DIVIDER_HIT_PX, LINE_PX, DividerColors, horizontal_divider, horizontal_divider_drag,
    horizontal_divider_drag_with, vertical_divider, vertical_divider_with,
};
pub use field::field;
pub use form::{checkbox_style, form_row, toggle_style};
pub use icon::{icon, icon_colored, icon_handle, icon_svg, icon_svg_colored};
pub use number_input::number_input;
pub use popover::{popover, popover_anchored, Placement};
pub use select::{SelectOption, identity_mark, select, select_sized};
pub use readable::readable;
pub use sidebar::{
    PANEL_HEADER_H, PANEL_REORDER_ANIM_MS, PANEL_REORDER_LIFT_SCALE, PANEL_REORDER_THRESHOLD,
    PANEL_ROW_H, PANEL_ROW_STRIDE, PANEL_W_DEFAULT, PANEL_W_MAX, PANEL_W_MIN, ReorderAnim,
    ReorderCfg, SIDEBAR_WIDTH, SectionScroll, SidebarHoverAction, SidebarIndicator,
    SidebarItem, SidebarItemChrome, SidebarPanel, SidebarSection, TabDescriptor, TabSize,
    panel_dragged_width, panel_drop_index, panel_drop_index_relative, panel_renumber_changed,
    panel_reordered, panel_sibling_offset, section_overflow_counts, sidebar, sidebar_with_header,
    vertical_tabs, vertical_tabs_sized,
};
pub use spectrum::{GradientStrip, SvSquare, alpha_strip, hue_strip, sv_square};
pub use split::{split, split_with};
pub use swatch::{swatch, swatch_sized};
pub use titlebar::{floating_frame, titlebar};
pub use toolbar::{toolbar_button, toolbar_button_msg};
