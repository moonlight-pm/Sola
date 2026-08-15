//! Shared protocol for `sat` ↔ `sola-agent-terminal`.
//!
//! The Wayland app is the other crate binary (`src/main.rs`). This
//! library is the JSON-over-UDS contract plus the `sat` client helper.

pub mod cli;
