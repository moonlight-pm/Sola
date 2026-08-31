//! HTML/CSS kit (not iced).
//!
//! Storybook binary [`APP_ID`] / [`WINDOW_TITLE`]. Lab twins:
//! `sola-settings-lab` / `Settings (lab)`, `sola-monitor-lab` /
//! `Monitor (lab)`. **Do not install. Do not overwrite iced apps.**

pub const APP_ID: &str = "sola-kit-spike";
pub const WINDOW_TITLE: &str = "Kit (spike)";

pub mod app;
pub mod components;
pub mod css;
pub mod dom;
pub mod gpu;
pub mod host;
pub mod icons;
pub mod layout;
pub mod markup;
pub mod monitor;
pub mod paint;
pub mod palette;
pub mod settings;
