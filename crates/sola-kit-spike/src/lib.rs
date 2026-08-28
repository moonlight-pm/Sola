//! Experimental HTML/CSS kit (not iced).
//!
//! Library + storybook binary. **Do not install. Do not merge.**
//! Wayland / bus identity is always [`APP_ID`] / [`WINDOW_TITLE`] — never
//! `sola-kit` / `Kit`.

pub const APP_ID: &str = "sola-kit-spike";
pub const WINDOW_TITLE: &str = "Kit (spike)";

pub mod app;
pub mod css;
pub mod dom;
pub mod gpu;
pub mod host;
pub mod icons;
pub mod layout;
pub mod markup;
pub mod paint;
