//! Shared core primitives for Sola.
//!
//! This crate centralizes low-level key primitives used across apps and the
//! compositor so we avoid scattered magic numbers.

pub mod applications;
pub mod config;
pub mod encrypted;
pub mod env;
pub mod keys;
pub mod log;
pub mod open_image;
pub mod open_url;
pub mod process;
pub mod theme;
pub mod watcher;

pub use encrypted::Encrypted;
pub use keys::{KeyChord, KeyCode};
pub use open_image::{open as open_image, open_logged as open_image_logged};
pub use open_url::{open as open_url, open_logged as open_url_logged};
