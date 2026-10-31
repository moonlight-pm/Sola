//! Shared core primitives for Sola.
//!
//! This crate centralizes low-level key primitives used across apps and the
//! compositor so we avoid scattered magic numbers.

pub mod applications;
pub mod background;
pub mod config;
pub mod encrypted;
pub mod env;
pub mod host_color;
pub mod keys;
pub mod log;
pub mod login;
pub mod omnibox;
pub mod open_calendar;
pub mod open_image;
pub mod open_url;
pub mod paths;
pub mod process;
pub mod ssh;
pub mod theme;
pub mod watcher;
pub mod window_label;

pub use encrypted::Encrypted;
pub use keys::{KeyChord, KeyCode};
pub use open_calendar::{open as open_calendar, open_logged as open_calendar_logged};
pub use open_image::{open as open_image, open_logged as open_image_logged};
pub use open_url::{
    chrome_is_running as sola_browser_is_running, open as open_url, open_logged as open_url_logged,
};
