//! Menu subsystem.
//!
//! `state` holds `MenuCache` and `synthesized_menu` — pure data, no window
//! logic. Window management (opening, closing, rendering the dropdown) lands
//! in Task 7.
pub mod state;
