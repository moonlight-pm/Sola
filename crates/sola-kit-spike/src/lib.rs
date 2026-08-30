//! HTML/CSS kit (not iced).
//!
//! Storybook binary [`APP_ID`] / [`WINDOW_TITLE`]. Settings twin is
//! `sola-settings-lab` / `Settings (lab)`. **Do not install. Do not
//! overwrite iced apps.**

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
pub mod palette;
pub mod settings;
