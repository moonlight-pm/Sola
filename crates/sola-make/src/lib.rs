//! sola-make also exposes a library surface so other crates' `build.rs`
//! scripts (notably sola-kit-legacy) can call `sola_make::cef::ensure_cef()` to
//! download the CEF binary distribution.

pub mod cef;
