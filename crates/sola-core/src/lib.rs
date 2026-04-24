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
pub mod process;
pub mod watcher;

pub use encrypted::Encrypted;
pub use keys::{KeyChord, KeyCode};
